use serde::Serialize;
use std::fmt;

/// Unified application error type.
///
/// Serializes to a human-readable message string over the Tauri IPC bridge,
/// so the frontend always receives a presentable message.
#[derive(Debug)]
pub enum AppError {
    /// Invalid or malformed user input (GeoJSON, base64, SQL...).
    Parse(String),
    /// A geoprocessing operation failed.
    Analysis(String),
    /// Persistence layer failure.
    Database(String),
    /// Filesystem / storage failure.
    Storage(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Parse(m) => write!(f, "Invalid input: {m}"),
            AppError::Analysis(m) => write!(f, "Geoprocessing failed: {m}"),
            AppError::Database(m) => write!(f, "Database error: {m}"),
            AppError::Storage(m) => write!(f, "Storage error: {m}"),
        }
    }
}

impl std::error::Error for AppError {}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        Self::Database(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        Self::Parse(e.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self::Storage(e.to_string())
    }
}

impl From<base64::DecodeError> for AppError {
    fn from(e: base64::DecodeError) -> Self {
        Self::Parse(format!("base64 decoding failed: {e}"))
    }
}
