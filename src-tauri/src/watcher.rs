//! Background tracking of the vault folder.
//!
//! Syncthing (or any other peer-to-peer file transport) writes directly into
//! the vault directory. This module watches that directory with the `notify`
//! crate, coalesces bursts of events and forwards a clean, debounced
//! `vault://changed` event to the webview so lists and open documents refresh
//! automatically.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::mpsc::{self, RecvTimeoutError},
    thread,
    time::Duration,
};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::error::{AppError, AppResult};

/// Name of the event emitted to the frontend.
pub const CHANGE_EVENT: &str = "vault://changed";
/// Suffix of temporary files produced by our own atomic writes.
pub const TEMP_SUFFIX: &str = ".zynky-tmp";
/// How long to wait for the burst of events to settle before emitting.
const DEBOUNCE: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultChange {
    /// Top-level vault folder the file belongs to (`Passwords`, `Notes`, ...),
    /// or `null` for files in the vault root such as `reminders.md`.
    pub category: Option<String>,
    /// File name without directories.
    pub name: String,
    /// Path relative to the vault root using `/` separators.
    pub relative_path: String,
    /// `created`, `modified`, `removed` or `other`.
    pub kind: String,
}

/// Owns the OS watcher. Dropping it stops watching and ends the debounce thread.
pub struct VaultWatcher {
    _watcher: RecommendedWatcher,
}

impl VaultWatcher {
    pub fn start(app: AppHandle, root: PathBuf) -> AppResult<Self> {
        let (tx, rx) = mpsc::channel::<Event>();

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            if let Ok(event) = res {
                // The receiver only disappears when the debounce thread exits,
                // which happens after this watcher is dropped; ignore errors.
                let _ = tx.send(event);
            }
        })
        .map_err(|e| AppError::Watcher(e.to_string()))?;

        watcher
            .watch(&root, RecursiveMode::Recursive)
            .map_err(|e| AppError::Watcher(e.to_string()))?;

        thread::Builder::new()
            .name("zynky-vault-watcher".into())
            .spawn(move || debounce_loop(app, root, rx))
            .map_err(|e| AppError::Watcher(e.to_string()))?;

        Ok(Self { _watcher: watcher })
    }
}

fn debounce_loop(app: AppHandle, root: PathBuf, rx: mpsc::Receiver<Event>) {
    let mut pending: BTreeMap<String, VaultChange> = BTreeMap::new();
    loop {
        let timeout = if pending.is_empty() {
            Duration::from_secs(3600)
        } else {
            DEBOUNCE
        };
        match rx.recv_timeout(timeout) {
            Ok(event) => {
                let kind = classify(&event.kind);
                for path in event.paths {
                    if let Some(change) = describe(&root, &path, kind) {
                        pending.insert(change.relative_path.clone(), change);
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                for (_, change) in std::mem::take(&mut pending) {
                    log::debug!("vault change: {:?}", change);
                    if let Err(e) = app.emit(CHANGE_EVENT, &change) {
                        log::warn!("failed to emit vault change event: {e}");
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                log::debug!("vault watcher stopped for {}", root.display());
                break;
            }
        }
    }
}

fn classify(kind: &EventKind) -> &'static str {
    match kind {
        EventKind::Create(_) => "created",
        EventKind::Modify(_) => "modified",
        EventKind::Remove(_) => "removed",
        _ => "other",
    }
}

/// Returns true for files that must never trigger a UI refresh: Syncthing's
/// own bookkeeping and partially written temp files.
pub fn is_ignored_name(name: &str) -> bool {
    name.starts_with(".syncthing.")
        || name.starts_with("~syncthing~")
        || name == ".stfolder"
        || name == ".stignore"
        || name == ".stversions"
        || name.ends_with(TEMP_SUFFIX)
        || name.ends_with(".tmp")
        || name.ends_with('~')
}

fn describe(root: &Path, path: &Path, kind: &'static str) -> Option<VaultChange> {
    let rel = path.strip_prefix(root).ok()?;
    let mut components = rel.components().map(|c| c.as_os_str().to_string_lossy().to_string());
    let first = components.next()?;
    let rest: Vec<String> = components.collect();

    // Anything inside Syncthing's hidden folders is noise.
    if is_ignored_name(&first) {
        return None;
    }
    let name = rest.last().cloned().unwrap_or_else(|| first.clone());
    if is_ignored_name(&name) {
        return None;
    }
    let category = if rest.is_empty() { None } else { Some(first.clone()) };
    let relative_path = if rest.is_empty() {
        first
    } else {
        format!("{}/{}", first, rest.join("/"))
    };
    Some(VaultChange {
        category,
        name,
        relative_path,
        kind: kind.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_syncthing_and_temp_files() {
        assert!(is_ignored_name(".syncthing.notes.md.tmp"));
        assert!(is_ignored_name("~syncthing~notes.md.tmp"));
        assert!(is_ignored_name(".stfolder"));
        assert!(is_ignored_name("abc.zynky-tmp"));
        assert!(!is_ignored_name("notes.md"));
    }

    #[test]
    fn describes_relative_paths() {
        let root = PathBuf::from("/vault");
        let c = describe(&root, Path::new("/vault/Notes/todo.md"), "modified").unwrap();
        assert_eq!(c.category.as_deref(), Some("Notes"));
        assert_eq!(c.name, "todo.md");
        assert_eq!(c.relative_path, "Notes/todo.md");

        let r = describe(&root, Path::new("/vault/reminders.md"), "created").unwrap();
        assert_eq!(r.category, None);
        assert_eq!(r.relative_path, "reminders.md");

        assert!(describe(&root, Path::new("/vault/.stfolder/x"), "other").is_none());
        assert!(describe(&root, Path::new("/elsewhere/x.md"), "other").is_none());
    }
}
