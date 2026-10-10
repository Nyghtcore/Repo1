//! Unified error type returned from every Tauri command.
//!
//! Tauri serialises command errors to the frontend, so `AppError` implements
//! `Serialize` and surfaces a stable machine-readable `code` next to the human
//! readable `message`.

use serde::Serialize;
use thiserror::Error;

use crate::{crypto::CryptoError, frontmatter::FrontmatterError, tasks::TaskError};

#[derive(Debug, Error)]
pub enum AppError {
    #[error("no vault is open")]
    NoVault,
    #[error("vault is locked; unlock it with the master password first")]
    Locked,
    #[error("vault at '{0}' is not initialised")]
    NotInitialised(String),
    #[error("vault at '{0}' already exists")]
    AlreadyInitialised(String),
    #[error("invalid file name '{0}'")]
    InvalidName(String),
    #[error("unknown vault category '{0}'")]
    UnknownCategory(String),
    #[error("file not found: {0}")]
    NotFound(String),
    #[error("I/O error at '{path}': {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Crypto(#[from] CryptoError),
    #[error(transparent)]
    Frontmatter(#[from] FrontmatterError),
    #[error(transparent)]
    Task(#[from] TaskError),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("file watcher error: {0}")]
    Watcher(String),
    #[error("{0}")]
    Other(String),
}

impl AppError {
    pub fn io(path: impl AsRef<std::path::Path>, source: std::io::Error) -> Self {
        AppError::Io {
            path: path.as_ref().display().to_string(),
            source,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            AppError::NoVault => "no_vault",
            AppError::Locked => "locked",
            AppError::NotInitialised(_) => "not_initialised",
            AppError::AlreadyInitialised(_) => "already_initialised",
            AppError::InvalidName(_) => "invalid_name",
            AppError::UnknownCategory(_) => "unknown_category",
            AppError::NotFound(_) => "not_found",
            AppError::Io { .. } => "io",
            AppError::Crypto(CryptoError::Decrypt) => "bad_password",
            AppError::Crypto(_) => "crypto",
            AppError::Frontmatter(_) => "frontmatter",
            AppError::Task(_) => "task",
            AppError::Json(_) => "json",
            AppError::Watcher(_) => "watcher",
            AppError::Other(_) => "other",
        }
    }
}

#[derive(Serialize)]
struct ErrorPayload<'a> {
    code: &'a str,
    message: String,
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ErrorPayload {
            code: self.code(),
            message: self.to_string(),
        }
        .serialize(serializer)
    }
}

pub type AppResult<T> = Result<T, AppError>;
