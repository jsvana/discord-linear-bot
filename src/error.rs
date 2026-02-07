#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Discord error: {0}")]
    Discord(#[from] serenity::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Linear API error: {0}")]
    LinearApi(String),

    #[error("Attachment upload failed: {0}")]
    AttachmentUpload(String),

    #[error("{0}")]
    Internal(String),
}
