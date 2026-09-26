//! Application error type shared by every layer.
//!
//! Errors cross the Tauri boundary as `{ kind, message }` so the frontend can
//! branch on a stable `kind` and show `message` to the user.

use serde::{Serialize, Serializer, ser::SerializeStruct};

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("File not found: {0}")]
    NotFound(String),

    #[error("Unsupported file type: {0}")]
    Unsupported(String),

    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("{0}")]
    Audio(String),

    #[error("Could not read metadata: {0}")]
    Metadata(String),

    #[error("Artwork error: {0}")]
    Artwork(String),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Database migration failed: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("The audio player is not running")]
    PlayerUnavailable,
}

impl AppError {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "notFound",
            Self::Unsupported(_) => "unsupported",
            Self::InvalidArgument(_) => "invalidArgument",
            Self::Audio(_) => "audio",
            Self::Metadata(_) => "metadata",
            Self::Artwork(_) => "artwork",
            Self::Database(_) | Self::Migration(_) => "database",
            Self::Io(_) => "io",
            Self::PlayerUnavailable => "playerUnavailable",
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("AppError", 2)?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}

impl From<gstreamer::glib::BoolError> for AppError {
    fn from(err: gstreamer::glib::BoolError) -> Self {
        Self::Audio(err.to_string())
    }
}

impl From<gstreamer::StateChangeError> for AppError {
    fn from(_: gstreamer::StateChangeError) -> Self {
        Self::Audio("The audio pipeline refused to change state".into())
    }
}
