//! Vault file broker.
//!
//! The vault is a plain folder:
//!
//! ```text
//! ZynkyVault/
//! ├── vault.json        # KDF salt/params + password verifier (no secrets)
//! ├── reminders.md      # timestamped tasks for the reminder engine
//! ├── Passwords/        # credential .md files (secure_blob in frontmatter)
//! ├── Notes/            # free-form markdown documents
//! ├── Tasks/            # markdown checklists
//! └── Media/            # arbitrary attachments
//! ```
//!
//! Every write goes through [`atomic_write`], which writes to a temporary
//! sibling file, flushes it to disk and then renames it over the target. A
//! peer running Syncthing therefore only ever observes complete files.

use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tauri::{AppHandle, Manager, State};
use zeroize::Zeroizing;

use crate::{
    crypto::{self, KdfParams, MasterKey},
    error::{AppError, AppResult},
    frontmatter,
    state::AppState,
    tasks::{self, TaskItem, TaskSummary},
    watcher::{self, VaultWatcher},
};

/// Name of the manifest file in the vault root.
pub const MANIFEST_FILE: &str = "vault.json";
/// Name of the reminders file in the vault root.
pub const REMINDERS_FILE: &str = "reminders.md";
/// Plaintext encrypted into the manifest to verify the master password.
const VERIFIER_PLAINTEXT: &[u8] = b"zynky-vault-verifier-v1";
/// Maximum size of a media file returned inline as a data URL (8 MiB).
const MAX_INLINE_MEDIA: u64 = 8 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Categories
// ---------------------------------------------------------------------------

/// The four top-level vault folders, plus `Root` for the handful of Markdown
/// files that live directly in the vault root (`reminders.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Category {
    Passwords,
    Notes,
    Tasks,
    Media,
    Root,
}

impl Category {
    /// The folders created inside a vault.
    pub const FOLDERS: [Category; 4] = [
        Category::Passwords,
        Category::Notes,
        Category::Tasks,
        Category::Media,
    ];

    pub fn folder(self) -> &'static str {
        match self {
            Category::Passwords => "Passwords",
            Category::Notes => "Notes",
            Category::Tasks => "Tasks",
            Category::Media => "Media",
            Category::Root => "",
        }
    }

    /// Markdown categories only list `.md` files; Media lists everything.
    pub fn is_markdown(self) -> bool {
        !matches!(self, Category::Media)
    }

    pub fn parse(name: &str) -> AppResult<Self> {
        match name.to_ascii_lowercase().as_str() {
            "passwords" => Ok(Category::Passwords),
            "notes" => Ok(Category::Notes),
            "tasks" => Ok(Category::Tasks),
            "media" => Ok(Category::Media),
            "root" | "" => Ok(Category::Root),
            other => Err(AppError::UnknownCategory(other.to_string())),
        }
    }
}

// ---------------------------------------------------------------------------
// Data transfer types (mirrored in src/types.ts)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultManifest {
    pub version: u32,
    pub kdf: KdfParams,
    /// Base64 Argon2 salt.
    pub salt: String,
    /// Base64 secure blob of [`VERIFIER_PLAINTEXT`].
    pub verifier: String,
    pub created: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    pub path: String,
    pub exists: bool,
    pub initialised: bool,
    pub unlocked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultInfo {
    pub path: String,
    pub name: String,
    pub unlocked: bool,
    pub created: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub category: Category,
    pub name: String,
    pub path: String,
    pub size: u64,
    pub modified: Option<DateTime<Utc>>,
    /// Document title from frontmatter (falls back to the file stem).
    pub title: String,
    pub tags: Vec<String>,
    pub tasks: Option<TaskSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentPayload {
    pub category: Category,
    pub name: String,
    pub path: String,
    /// Frontmatter as a JSON object (empty object if none).
    pub frontmatter: Map<String, Value>,
    /// Markdown body with the YAML block removed.
    pub body: String,
    pub tasks: Vec<TaskItem>,
    pub modified: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteDocumentInput {
    pub category: Category,
    pub name: String,
    pub body: String,
    /// When `None`, the frontmatter already on disk is preserved.
    #[serde(default)]
    pub frontmatter: Option<Map<String, Value>>,
}

/// The sensitive part of a credential. This struct is serialised to JSON,
/// encrypted and stored as `secure_blob`. It is never written in clear.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialSecret {
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub totp_secret: String,
    /// Arbitrary extra secret fields (security questions, API keys, ...).
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
}

/// Decrypted credential as shown in the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Credential {
    pub name: String,
    pub path: String,
    pub title: String,
    pub tags: Vec<String>,
    pub created: Option<String>,
    pub updated: Option<String>,
    pub secret: CredentialSecret,
    /// Plain human readable markdown notes (the file body).
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialInput {
    /// Existing file name, or `None` to create a new file from the title.
    pub name: Option<String>,
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub secret: CredentialSecret,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaPayload {
    pub name: String,
    pub path: String,
    pub mime: String,
    pub size: u64,
    /// `data:<mime>;base64,...` for files up to [`MAX_INLINE_MEDIA`].
    pub data_url: Option<String>,
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Reject names that could escape the category folder.
pub fn validate_name(name: &str) -> AppResult<()> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains('\0')
        || trimmed.contains(':')
        || trimmed.ends_with(watcher::TEMP_SUFFIX)
    {
        return Err(AppError::InvalidName(name.to_string()));
    }
    Ok(())
}

/// Ensure Markdown categories always use the `.md` extension.
fn normalise_markdown_name(category: Category, name: &str) -> AppResult<String> {
    validate_name(name)?;
    let name = name.trim();
    if category.is_markdown() && !name.to_ascii_lowercase().ends_with(".md") {
        Ok(format!("{name}.md"))
    } else {
        Ok(name.to_string())
    }
}

/// Turn a free-form title into a safe file stem.
pub fn slugify(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut last_dash = false;
    for ch in title.trim().chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    let slug = out.trim_end_matches('-').to_string();
    if slug.is_empty() {
        "untitled".to_string()
    } else {
        slug
    }
}

/// Pick a file name that does not collide with an existing one.
fn unique_markdown_name(dir: &Path, stem: &str) -> String {
    let candidate = format!("{stem}.md");
    if !dir.join(&candidate).exists() {
        return candidate;
    }
    for n in 2..10_000 {
        let candidate = format!("{stem}-{n}.md");
        if !dir.join(&candidate).exists() {
            return candidate;
        }
    }
    format!("{stem}-{}.md", Utc::now().timestamp_millis())
}

fn category_dir(root: &Path, category: Category) -> PathBuf {
    match category {
        Category::Root => root.to_path_buf(),
        other => root.join(other.folder()),
    }
}

fn file_path(root: &Path, category: Category, name: &str) -> AppResult<PathBuf> {
    validate_name(name)?;
    // Only Markdown files may be addressed in the vault root; this keeps the
    // manifest (`vault.json`) out of reach of rename/delete commands.
    if category == Category::Root && !name.trim().to_ascii_lowercase().ends_with(".md") {
        return Err(AppError::InvalidName(name.to_string()));
    }
    Ok(category_dir(root, category).join(name.trim()))
}

fn modified_time(meta: &fs::Metadata) -> Option<DateTime<Utc>> {
    meta.modified().ok().map(DateTime::<Utc>::from)
}

fn now_iso() -> String {
    Local::now().to_rfc3339()
}

// ---------------------------------------------------------------------------
// Atomic file I/O
// ---------------------------------------------------------------------------

/// Write `contents` to `target` atomically: temp file → fsync → rename.
pub fn atomic_write(target: &Path, contents: &[u8]) -> AppResult<()> {
    let dir = target
        .parent()
        .ok_or_else(|| AppError::InvalidName(target.display().to_string()))?;
    fs::create_dir_all(dir).map_err(|e| AppError::io(dir, e))?;

    let file_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| AppError::InvalidName(target.display().to_string()))?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = dir.join(format!(
        ".{file_name}.{}.{nonce}{}",
        std::process::id(),
        watcher::TEMP_SUFFIX
    ));

    let result = (|| -> AppResult<()> {
        let mut file = fs::File::create(&tmp).map_err(|e| AppError::io(&tmp, e))?;
        file.write_all(contents).map_err(|e| AppError::io(&tmp, e))?;
        file.sync_all().map_err(|e| AppError::io(&tmp, e))?;
        drop(file);
        fs::rename(&tmp, target).map_err(|e| AppError::io(target, e))?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn read_to_string(path: &Path) -> AppResult<String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(AppError::NotFound(path.display().to_string()))
        }
        Err(e) => Err(AppError::io(path, e)),
    }
}

// ---------------------------------------------------------------------------
// Vault lifecycle
// ---------------------------------------------------------------------------

fn manifest_path(root: &Path) -> PathBuf {
    root.join(MANIFEST_FILE)
}

fn read_manifest(root: &Path) -> AppResult<VaultManifest> {
    let path = manifest_path(root);
    if !path.exists() {
        return Err(AppError::NotInitialised(root.display().to_string()));
    }
    let text = read_to_string(&path)?;
    Ok(serde_json::from_str(&text)?)
}

/// Create the folder layout if it does not exist yet.
pub fn ensure_layout(root: &Path) -> AppResult<()> {
    fs::create_dir_all(root).map_err(|e| AppError::io(root, e))?;
    for category in Category::FOLDERS {
        let dir = category_dir(root, category);
        fs::create_dir_all(&dir).map_err(|e| AppError::io(&dir, e))?;
    }
    let reminders = root.join(REMINDERS_FILE);
    if !reminders.exists() {
        atomic_write(
            &reminders,
            b"# Reminders\n\nAdd a task with an ISO 8601 timestamp and Zynky will notify you:\n\n- [ ] Example reminder @ 2030-01-01T09:00\n",
        )?;
    }
    Ok(())
}

fn open_common(app: &AppHandle, state: &AppState, root: PathBuf, key: MasterKey) -> AppResult<VaultInfo> {
    let manifest = read_manifest(&root)?;
    state.set_watcher(None);
    let watcher = VaultWatcher::start(app.clone(), root.clone())?;
    state.set_vault_root(Some(root.clone()));
    state.set_key(Some(key));
    state.set_watcher(Some(watcher));
    settings::remember_vault(app, &root);
    Ok(VaultInfo {
        name: root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "ZynkyVault".into()),
        path: root.display().to_string(),
        unlocked: true,
        created: Some(manifest.created),
    })
}

/// Inspect a folder without opening it.
#[tauri::command]
pub fn vault_status(state: State<'_, AppState>, path: String) -> AppResult<VaultStatus> {
    let root = PathBuf::from(&path);
    let exists = root.is_dir();
    let initialised = exists && manifest_path(&root).is_file();
    let unlocked = state.is_unlocked() && state.vault_root().as_deref() == Some(root.as_path());
    Ok(VaultStatus {
        path,
        exists,
        initialised,
        unlocked,
    })
}

/// Create a brand new vault at `path` (a `ZynkyVault` folder is created if the
/// chosen folder does not already end with that name).
#[tauri::command]
pub fn create_vault(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    master_password: String,
) -> AppResult<VaultInfo> {
    let master_password = Zeroizing::new(master_password);
    if master_password.trim().is_empty() {
        return Err(AppError::Other("master password must not be empty".into()));
    }
    let mut root = PathBuf::from(path.trim());
    if root.file_name().map(|n| n != "ZynkyVault").unwrap_or(true) {
        root = root.join("ZynkyVault");
    }
    if manifest_path(&root).exists() {
        return Err(AppError::AlreadyInitialised(root.display().to_string()));
    }
    ensure_layout(&root)?;

    let kdf = KdfParams::default();
    let salt = crypto::generate_salt()?;
    let key = crypto::derive_key(&master_password, &salt, kdf)?;
    let verifier = crypto::encrypt_to_base64(&key, VERIFIER_PLAINTEXT)?;
    let manifest = VaultManifest {
        version: 1,
        kdf,
        salt: crypto::to_base64(&salt),
        verifier,
        created: Utc::now(),
    };
    atomic_write(
        &manifest_path(&root),
        serde_json::to_string_pretty(&manifest)?.as_bytes(),
    )?;
    open_common(&app, &state, root, key)
}

/// Unlock an existing vault with the master password.
#[tauri::command]
pub fn unlock_vault(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    master_password: String,
) -> AppResult<VaultInfo> {
    let master_password = Zeroizing::new(master_password);
    let root = PathBuf::from(path.trim());
    let manifest = read_manifest(&root)?;
    let salt = crypto::from_base64(&manifest.salt)?;
    let key = crypto::derive_key(&master_password, &salt, manifest.kdf)?;
    let verifier = crypto::decrypt_from_base64(&key, &manifest.verifier)?;
    if verifier.as_slice() != VERIFIER_PLAINTEXT {
        return Err(AppError::Crypto(crypto::CryptoError::Decrypt));
    }
    ensure_layout(&root)?;
    open_common(&app, &state, root, key)
}

/// Wipe the key from memory and stop watching the folder.
#[tauri::command]
pub fn lock_vault(state: State<'_, AppState>) -> AppResult<()> {
    state.close();
    Ok(())
}

/// Information about the currently open vault, if any.
#[tauri::command]
pub fn current_vault(state: State<'_, AppState>) -> AppResult<Option<VaultInfo>> {
    let Some(root) = state.vault_root() else {
        return Ok(None);
    };
    let created = read_manifest(&root).ok().map(|m| m.created);
    Ok(Some(VaultInfo {
        name: root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        path: root.display().to_string(),
        unlocked: state.is_unlocked(),
        created,
    }))
}

/// Change the master password: re-derive the key, re-encrypt every credential
/// and write a fresh manifest.
#[tauri::command]
pub fn change_master_password(
    state: State<'_, AppState>,
    current_password: String,
    new_password: String,
) -> AppResult<()> {
    let current_password = Zeroizing::new(current_password);
    let new_password = Zeroizing::new(new_password);
    if new_password.trim().is_empty() {
        return Err(AppError::Other("new master password must not be empty".into()));
    }
    let root = state.require_vault()?;
    let manifest = read_manifest(&root)?;
    let old_salt = crypto::from_base64(&manifest.salt)?;
    let old_key = crypto::derive_key(&current_password, &old_salt, manifest.kdf)?;
    crypto::decrypt_from_base64(&old_key, &manifest.verifier)?;

    let kdf = KdfParams::default();
    let salt = crypto::generate_salt()?;
    let new_key = crypto::derive_key(&new_password, &salt, kdf)?;

    // Re-encrypt every credential file.
    let dir = category_dir(&root, Category::Passwords);
    for entry in fs::read_dir(&dir).map_err(|e| AppError::io(&dir, e))? {
        let entry = entry.map_err(|e| AppError::io(&dir, e))?;
        let path = entry.path();
        if !path.extension().map(|e| e == "md").unwrap_or(false) {
            continue;
        }
        let raw = read_to_string(&path)?;
        let mut doc = frontmatter::parse(&raw)?;
        let Some(Value::String(blob)) = doc.frontmatter.get("secure_blob").cloned() else {
            continue;
        };
        let plaintext = crypto::decrypt_from_base64(&old_key, &blob)?;
        let reencrypted = crypto::encrypt_to_base64(&new_key, &plaintext)?;
        doc.frontmatter
            .insert("secure_blob".into(), Value::String(reencrypted));
        atomic_write(&path, frontmatter::render(&doc.frontmatter, &doc.body)?.as_bytes())?;
    }

    let new_manifest = VaultManifest {
        version: 1,
        kdf,
        salt: crypto::to_base64(&salt),
        verifier: crypto::encrypt_to_base64(&new_key, VERIFIER_PLAINTEXT)?,
        created: manifest.created,
    };
    atomic_write(
        &manifest_path(&root),
        serde_json::to_string_pretty(&new_manifest)?.as_bytes(),
    )?;
    state.set_key(Some(new_key));
    Ok(())
}

// ---------------------------------------------------------------------------
// Listing & documents
// ---------------------------------------------------------------------------

fn string_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .collect(),
        Some(Value::String(s)) => s
            .split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

fn entry_for(root: &Path, category: Category, path: &Path) -> AppResult<FileEntry> {
    let meta = fs::metadata(path).map_err(|e| AppError::io(path, e))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let stem = path
        .file_stem()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| name.clone());

    let (title, tags, task_summary) = if category.is_markdown() {
        match fs::read_to_string(path).ok().map(|raw| frontmatter::parse(&raw)) {
            Some(Ok(doc)) => {
                let title = doc
                    .frontmatter
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or(stem);
                let tags = string_list(doc.frontmatter.get("tags"));
                let summary = if category == Category::Tasks || category == Category::Notes {
                    Some(tasks::summarize(&doc.body))
                } else {
                    None
                };
                (title, tags, summary)
            }
            _ => (stem, Vec::new(), None),
        }
    } else {
        (stem, Vec::new(), None)
    };

    let _ = root;
    Ok(FileEntry {
        category,
        name,
        path: path.display().to_string(),
        size: meta.len(),
        modified: modified_time(&meta),
        title,
        tags,
        tasks: task_summary,
    })
}

/// List the files in one of the vault's category folders.
#[tauri::command]
pub fn list_files(state: State<'_, AppState>, category: Category) -> AppResult<Vec<FileEntry>> {
    let root = state.require_vault()?;
    let dir = category_dir(&root, category);
    fs::create_dir_all(&dir).map_err(|e| AppError::io(&dir, e))?;
    let mut entries = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|e| AppError::io(&dir, e))? {
        let entry = entry.map_err(|e| AppError::io(&dir, e))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if !path.is_file() || watcher::is_ignored_name(&name) || name.starts_with('.') {
            continue;
        }
        if category.is_markdown() && !name.to_ascii_lowercase().ends_with(".md") {
            continue;
        }
        entries.push(entry_for(&root, category, &path)?);
    }
    entries.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.name.cmp(&b.name)));
    Ok(entries)
}

/// Read a Markdown document, splitting frontmatter from the body and
/// extracting checkbox tasks.
#[tauri::command]
pub fn read_document(
    state: State<'_, AppState>,
    category: Category,
    name: String,
) -> AppResult<DocumentPayload> {
    let root = state.require_vault()?;
    let path = file_path(&root, category, &name)?;
    let raw = read_to_string(&path)?;
    let doc = frontmatter::parse(&raw)?;
    let meta = fs::metadata(&path).ok();
    Ok(DocumentPayload {
        category,
        name: name.trim().to_string(),
        path: path.display().to_string(),
        tasks: tasks::parse_tasks(&doc.body),
        frontmatter: doc.frontmatter,
        body: doc.body,
        modified: meta.as_ref().and_then(modified_time),
    })
}

/// Write a Markdown document atomically. Existing frontmatter is preserved
/// unless new frontmatter is supplied; `updated` is always refreshed.
#[tauri::command]
pub fn write_document(state: State<'_, AppState>, input: WriteDocumentInput) -> AppResult<FileEntry> {
    let root = state.require_vault()?;
    if input.category == Category::Media {
        return Err(AppError::UnknownCategory(
            "media files are binary; use import_media".into(),
        ));
    }
    let name = normalise_markdown_name(input.category, &input.name)?;
    let path = file_path(&root, input.category, &name)?;

    let mut fm = match input.frontmatter {
        Some(fm) => fm,
        None => match fs::read_to_string(&path) {
            Ok(raw) => frontmatter::parse(&raw)?.frontmatter,
            Err(_) => Map::new(),
        },
    };
    if input.category == Category::Passwords {
        // Credential bodies are edited through save_credential so the
        // secure_blob is never accidentally dropped; keep whatever is there.
        if let Ok(raw) = fs::read_to_string(&path) {
            if let Ok(existing) = frontmatter::parse(&raw) {
                if let Some(blob) = existing.frontmatter.get("secure_blob") {
                    fm.insert("secure_blob".into(), blob.clone());
                }
            }
        }
    }
    fm.entry("created".to_string())
        .or_insert_with(|| Value::String(now_iso()));
    fm.insert("updated".into(), Value::String(now_iso()));

    let text = frontmatter::render(&fm, &input.body)?;
    atomic_write(&path, text.as_bytes())?;
    entry_for(&root, input.category, &path)
}

/// Create a new, empty Markdown document from a title.
#[tauri::command]
pub fn create_document(
    state: State<'_, AppState>,
    category: Category,
    title: String,
) -> AppResult<FileEntry> {
    let root = state.require_vault()?;
    if !matches!(category, Category::Notes | Category::Tasks | Category::Passwords) {
        return Err(AppError::UnknownCategory(format!(
            "cannot create documents in {:?}",
            category
        )));
    }
    let dir = category_dir(&root, category);
    fs::create_dir_all(&dir).map_err(|e| AppError::io(&dir, e))?;
    let title = if title.trim().is_empty() {
        "Untitled".to_string()
    } else {
        title.trim().to_string()
    };
    let name = unique_markdown_name(&dir, &slugify(&title));
    let path = dir.join(&name);

    let mut fm = Map::new();
    fm.insert("title".into(), Value::String(title.clone()));
    fm.insert("created".into(), Value::String(now_iso()));
    fm.insert("updated".into(), Value::String(now_iso()));
    fm.insert("tags".into(), Value::Array(Vec::new()));
    let body = match category {
        Category::Tasks => format!("# {title}\n\n- [ ] First task\n"),
        _ => format!("# {title}\n\n"),
    };
    atomic_write(&path, frontmatter::render(&fm, &body)?.as_bytes())?;
    entry_for(&root, category, &path)
}

/// Rename a file inside its category folder.
#[tauri::command]
pub fn rename_file(
    state: State<'_, AppState>,
    category: Category,
    name: String,
    new_name: String,
) -> AppResult<FileEntry> {
    let root = state.require_vault()?;
    let from = file_path(&root, category, &name)?;
    let new_name = if category.is_markdown() {
        normalise_markdown_name(category, &new_name)?
    } else {
        validate_name(&new_name)?;
        new_name.trim().to_string()
    };
    let to = file_path(&root, category, &new_name)?;
    if !from.exists() {
        return Err(AppError::NotFound(from.display().to_string()));
    }
    if to.exists() {
        return Err(AppError::Other(format!("'{new_name}' already exists")));
    }
    fs::rename(&from, &to).map_err(|e| AppError::io(&from, e))?;
    entry_for(&root, category, &to)
}

/// Permanently delete a file from the vault.
#[tauri::command]
pub fn delete_file(state: State<'_, AppState>, category: Category, name: String) -> AppResult<()> {
    let root = state.require_vault()?;
    let path = file_path(&root, category, &name)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(AppError::NotFound(path.display().to_string()))
        }
        Err(e) => Err(AppError::io(&path, e)),
    }
}

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

/// Parse the checkbox items of a document.
#[tauri::command]
pub fn list_tasks(state: State<'_, AppState>, category: Category, name: String) -> AppResult<Vec<TaskItem>> {
    let root = state.require_vault()?;
    let path = file_path(&root, category, &name)?;
    let raw = read_to_string(&path)?;
    Ok(tasks::parse_tasks(&frontmatter::parse(&raw)?.body))
}

/// Flip a single checkbox and immediately rewrite the file on disk.
#[tauri::command]
pub fn toggle_task(
    state: State<'_, AppState>,
    category: Category,
    name: String,
    line: usize,
    checked: bool,
) -> AppResult<Vec<TaskItem>> {
    let root = state.require_vault()?;
    let path = file_path(&root, category, &name)?;
    let raw = read_to_string(&path)?;
    let mut doc = frontmatter::parse(&raw)?;
    doc.body = tasks::set_task_state(&doc.body, line, checked)?;
    if !doc.frontmatter.is_empty() {
        doc.frontmatter
            .insert("updated".into(), Value::String(now_iso()));
    }
    atomic_write(&path, frontmatter::render(&doc.frontmatter, &doc.body)?.as_bytes())?;
    Ok(tasks::parse_tasks(&doc.body))
}

// ---------------------------------------------------------------------------
// Credentials
// ---------------------------------------------------------------------------

fn decrypt_credential(key: &MasterKey, name: &str, path: &Path, raw: &str) -> AppResult<Credential> {
    let doc = frontmatter::parse(raw)?;
    let secret = match doc.frontmatter.get("secure_blob") {
        Some(Value::String(blob)) if !blob.trim().is_empty() => {
            let json = crypto::decrypt_string(key, blob)?;
            serde_json::from_str::<CredentialSecret>(&json)?
        }
        _ => CredentialSecret::default(),
    };
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| name.to_string());
    Ok(Credential {
        name: name.to_string(),
        path: path.display().to_string(),
        title: doc
            .frontmatter
            .get("title")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or(stem),
        tags: string_list(doc.frontmatter.get("tags")),
        created: doc
            .frontmatter
            .get("created")
            .and_then(Value::as_str)
            .map(str::to_string),
        updated: doc
            .frontmatter
            .get("updated")
            .and_then(Value::as_str)
            .map(str::to_string),
        secret,
        notes: doc.body,
    })
}

/// Read and decrypt one credential file.
#[tauri::command]
pub fn read_credential(state: State<'_, AppState>, name: String) -> AppResult<Credential> {
    let root = state.require_vault()?;
    let key = state.require_key()?;
    let name = normalise_markdown_name(Category::Passwords, &name)?;
    let path = file_path(&root, Category::Passwords, &name)?;
    let raw = read_to_string(&path)?;
    decrypt_credential(&key, &name, &path, &raw)
}

/// Encrypt and save a credential. Secrets go into `secure_blob`; everything
/// else stays human readable.
#[tauri::command]
pub fn save_credential(state: State<'_, AppState>, input: CredentialInput) -> AppResult<Credential> {
    let root = state.require_vault()?;
    let key = state.require_key()?;
    let dir = category_dir(&root, Category::Passwords);
    fs::create_dir_all(&dir).map_err(|e| AppError::io(&dir, e))?;

    let title = if input.title.trim().is_empty() {
        "Untitled credential".to_string()
    } else {
        input.title.trim().to_string()
    };
    let name = match input.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        Some(existing) => normalise_markdown_name(Category::Passwords, existing)?,
        None => unique_markdown_name(&dir, &slugify(&title)),
    };
    let path = dir.join(&name);

    let mut fm = match fs::read_to_string(&path) {
        Ok(raw) => frontmatter::parse(&raw)?.frontmatter,
        Err(_) => Map::new(),
    };
    let secret_json = Zeroizing::new(serde_json::to_string(&input.secret)?);
    let blob = crypto::encrypt_to_base64(&key, secret_json.as_bytes())?;

    fm.insert("title".into(), Value::String(title));
    fm.insert(
        "tags".into(),
        Value::Array(
            input
                .tags
                .iter()
                .map(|t| t.trim())
                .filter(|t| !t.is_empty())
                .map(|t| Value::String(t.to_string()))
                .collect(),
        ),
    );
    fm.entry("created".to_string())
        .or_insert_with(|| Value::String(now_iso()));
    fm.insert("updated".into(), Value::String(now_iso()));
    fm.insert("secure_blob".into(), Value::String(blob));

    let text = frontmatter::render(&fm, &input.notes)?;
    atomic_write(&path, text.as_bytes())?;
    decrypt_credential(&key, &name, &path, &text)
}

/// Generate a random password on the backend using the OS CSPRNG.
#[tauri::command]
pub fn generate_password(length: Option<usize>, symbols: Option<bool>) -> AppResult<String> {
    use rand::seq::SliceRandom;
    const LETTERS: &[u8] = b"abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    const SYMBOLS: &[u8] = b"!@#$%^&*()-_=+[]{};:,.?";
    let length = length.unwrap_or(20).clamp(8, 128);
    let mut alphabet: Vec<u8> = LETTERS.to_vec();
    if symbols.unwrap_or(true) {
        alphabet.extend_from_slice(SYMBOLS);
    }
    let mut rng = rand::rngs::OsRng;
    let password: String = (0..length)
        .map(|_| *alphabet.choose(&mut rng).unwrap_or(&b'x') as char)
        .collect();
    Ok(password)
}

// ---------------------------------------------------------------------------
// Media
// ---------------------------------------------------------------------------

fn mime_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("bmp") => "image/bmp",
        Some("pdf") => "application/pdf",
        Some("txt") | Some("md") => "text/plain",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        _ => "application/octet-stream",
    }
}

/// Copy an external file into the vault's Media folder.
#[tauri::command]
pub fn import_media(state: State<'_, AppState>, source_path: String) -> AppResult<FileEntry> {
    let root = state.require_vault()?;
    let source = PathBuf::from(source_path.trim());
    if !source.is_file() {
        return Err(AppError::NotFound(source.display().to_string()));
    }
    let dir = category_dir(&root, Category::Media);
    fs::create_dir_all(&dir).map_err(|e| AppError::io(&dir, e))?;
    let original = source
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| AppError::InvalidName(source.display().to_string()))?;
    validate_name(&original)?;

    let stem = source
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let ext = source
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let mut target = dir.join(&original);
    let mut n = 2;
    while target.exists() {
        target = dir.join(format!("{stem}-{n}{ext}"));
        n += 1;
    }
    let bytes = fs::read(&source).map_err(|e| AppError::io(&source, e))?;
    atomic_write(&target, &bytes)?;
    entry_for(&root, Category::Media, &target)
}

/// Read a media file, inlining small ones as a data URL for previews.
#[tauri::command]
pub fn read_media(state: State<'_, AppState>, name: String) -> AppResult<MediaPayload> {
    use base64::Engine as _;
    let root = state.require_vault()?;
    let path = file_path(&root, Category::Media, &name)?;
    let meta = fs::metadata(&path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => AppError::NotFound(path.display().to_string()),
        _ => AppError::io(&path, e),
    })?;
    let mime = mime_for(&path);
    let data_url = if meta.len() <= MAX_INLINE_MEDIA {
        let bytes = fs::read(&path).map_err(|e| AppError::io(&path, e))?;
        Some(format!(
            "data:{mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ))
    } else {
        None
    };
    Ok(MediaPayload {
        name: name.trim().to_string(),
        path: path.display().to_string(),
        mime: mime.to_string(),
        size: meta.len(),
        data_url,
    })
}

// ---------------------------------------------------------------------------
// Persistent app settings (last vault path)
// ---------------------------------------------------------------------------

pub mod settings {
    use super::*;

    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct AppSettings {
        pub last_vault_path: Option<String>,
        #[serde(default)]
        pub recent_vaults: Vec<String>,
    }

    fn settings_path(app: &AppHandle) -> Option<PathBuf> {
        app.path().app_config_dir().ok().map(|d| d.join("settings.json"))
    }

    pub fn load(app: &AppHandle) -> AppSettings {
        settings_path(app)
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(app: &AppHandle, settings: &AppSettings) -> AppResult<()> {
        let Some(path) = settings_path(app) else {
            return Ok(());
        };
        atomic_write(&path, serde_json::to_string_pretty(settings)?.as_bytes())
    }

    pub fn remember_vault(app: &AppHandle, root: &Path) {
        let mut settings = load(app);
        let display = root.display().to_string();
        settings.recent_vaults.retain(|p| p != &display);
        settings.recent_vaults.insert(0, display.clone());
        settings.recent_vaults.truncate(8);
        settings.last_vault_path = Some(display);
        if let Err(e) = save(app, &settings) {
            log::warn!("could not persist settings: {e}");
        }
    }

    #[tauri::command]
    pub fn get_settings(app: AppHandle) -> AppResult<AppSettings> {
        Ok(load(&app))
    }

    /// Default location suggested for a new vault: `<Documents>/ZynkyVault`.
    #[tauri::command]
    pub fn default_vault_path(app: AppHandle) -> AppResult<String> {
        let base = app
            .path()
            .document_dir()
            .or_else(|_| app.path().home_dir())
            .map_err(|e| AppError::Other(e.to_string()))?;
        Ok(base.join("ZynkyVault").display().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_titles() {
        assert_eq!(slugify("GitHub Account"), "github-account");
        assert_eq!(slugify("  Bank!! (Main) "), "bank-main");
        assert_eq!(slugify("***"), "untitled");
    }

    #[test]
    fn validates_names() {
        assert!(validate_name("ok.md").is_ok());
        assert!(validate_name("../etc/passwd").is_err());
        assert!(validate_name("a/b").is_err());
        assert!(validate_name("a\\b").is_err());
        assert!(validate_name("").is_err());
    }

    #[test]
    fn atomic_write_replaces_file() {
        let dir = std::env::temp_dir().join(format!("zynky-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("note.md");
        atomic_write(&target, b"one").unwrap();
        atomic_write(&target, b"two").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "two");
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(watcher::TEMP_SUFFIX))
            .collect();
        assert!(leftovers.is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn category_parse() {
        assert_eq!(Category::parse("passwords").unwrap(), Category::Passwords);
        assert_eq!(Category::parse("Media").unwrap(), Category::Media);
        assert!(Category::parse("other").is_err());
    }
}
