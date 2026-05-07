use thiserror::Error;

#[derive(Debug, Error)]
pub enum CodeBrainError {
    #[error("scan failed: {0}")]
    Scan(#[from] anyhow::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("project root does not exist: {0}")]
    InvalidRoot(std::path::PathBuf),
}
