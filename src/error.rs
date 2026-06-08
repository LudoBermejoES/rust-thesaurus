use thiserror::Error;

#[derive(Debug, Error)]
pub enum ThesaurusError {
    #[error("download failed: {0}")]
    Download(#[from] reqwest::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("SHA-256 mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("JSON parse error on line {line}: {source}")]
    Json { line: usize, source: serde_json::Error },

    #[error("data directory unavailable")]
    NoDataDir,
}
