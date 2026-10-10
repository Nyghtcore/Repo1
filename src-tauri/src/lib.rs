//! Zynky – local-first, serverless credential, document and task manager.
//!
//! Module map:
//! * [`crypto`]      – Argon2id key derivation + AES-256-GCM.
//! * [`frontmatter`] – YAML frontmatter split/merge for Markdown files.
//! * [`tasks`]       – Markdown checkbox parsing and in-place toggling.
//! * [`vault`]       – File broker commands (list / read / atomic write).
//! * [`watcher`]     – Filesystem change tracking for Syncthing peers.
//! * [`reminder`]    – 60-second background worker + Windows toasts.
//! * [`state`]       – Shared process state (vault path, master key).
//! * [`error`]       – Serialisable error type for every command.

pub mod crypto;
pub mod error;
pub mod frontmatter;
pub mod reminder;
pub mod state;
pub mod tasks;
pub mod vault;
pub mod watcher;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(AppState::default())
        .setup(|app| {
            reminder::spawn(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // vault lifecycle
            vault::vault_status,
            vault::create_vault,
            vault::unlock_vault,
            vault::lock_vault,
            vault::current_vault,
            vault::change_master_password,
            // documents
            vault::list_files,
            vault::read_document,
            vault::write_document,
            vault::create_document,
            vault::rename_file,
            vault::delete_file,
            // tasks
            vault::list_tasks,
            vault::toggle_task,
            // credentials
            vault::read_credential,
            vault::save_credential,
            vault::generate_password,
            // media
            vault::import_media,
            vault::read_media,
            // settings
            vault::settings::get_settings,
            vault::settings::default_vault_path,
            // reminders
            reminder::list_reminders,
            reminder::test_notification,
            reminder::reminder_engine_status,
            reminder::format_reminder_timestamp,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Zynky");
}
