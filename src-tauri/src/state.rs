//! Process-wide application state shared between Tauri commands, the file
//! watcher and the background reminder thread.

use std::{
    path::PathBuf,
    sync::{Mutex, RwLock},
};

use crate::{
    crypto::MasterKey,
    error::{AppError, AppResult},
    watcher::VaultWatcher,
};

#[derive(Default)]
pub struct AppState {
    /// Root folder of the currently open vault (`.../ZynkyVault`).
    vault_root: RwLock<Option<PathBuf>>,
    /// Derived master key. `None` while the vault is locked.
    key: Mutex<Option<MasterKey>>,
    /// Filesystem watcher for the open vault.
    watcher: Mutex<Option<VaultWatcher>>,
}

impl AppState {
    /// Currently open vault root, if any.
    pub fn vault_root(&self) -> Option<PathBuf> {
        self.vault_root
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Vault root or `AppError::NoVault`.
    pub fn require_vault(&self) -> AppResult<PathBuf> {
        self.vault_root().ok_or(AppError::NoVault)
    }

    pub fn set_vault_root(&self, root: Option<PathBuf>) {
        *self
            .vault_root
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = root;
    }

    /// Clone of the master key or `AppError::Locked`.
    pub fn require_key(&self) -> AppResult<MasterKey> {
        self.key
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .ok_or(AppError::Locked)
    }

    pub fn is_unlocked(&self) -> bool {
        self.key
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some()
    }

    pub fn set_key(&self, key: Option<MasterKey>) {
        // Assigning drops the previous key, which zeroizes it.
        *self
            .key
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = key;
    }

    pub fn set_watcher(&self, watcher: Option<VaultWatcher>) {
        *self
            .watcher
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = watcher;
    }

    /// Forget the vault, wipe the key and stop watching the folder.
    pub fn close(&self) {
        self.set_watcher(None);
        self.set_key(None);
        self.set_vault_root(None);
    }
}
