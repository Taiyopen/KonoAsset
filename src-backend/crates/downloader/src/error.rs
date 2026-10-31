use serde::{Deserialize, Serialize};

/// Error category sent to the frontend; the UI shows a localized message per kind.
#[derive(Serialize, Deserialize, specta::Type, Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownloadErrorKind {
    PasswordRequired,
    WrongPassword,
    NotFound,
    LoginRequired,
    RateLimited,
    NotAFile,
    ImportFailed,
    Other,
}

#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum DownloadError {
    #[error("password required")]
    PasswordRequired,
    #[error("wrong password")]
    WrongPassword,
    #[error("not found: {0}")]
    NotFound(String),
    #[error("login required: {0}")]
    LoginRequired(String),
    #[error("rate limited: {0}")]
    RateLimited(String),
    #[error("not a file: {0}")]
    NotAFile(String),
    /// Not worth retrying.
    #[error("{0}")]
    Fatal(String),
    /// Worth retrying (network hiccups, size/checksum mismatch, ...).
    #[error("{0}")]
    Retryable(String),
}

impl DownloadError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, DownloadError::Retryable(_))
    }

    pub fn kind(&self) -> DownloadErrorKind {
        match self {
            DownloadError::PasswordRequired => DownloadErrorKind::PasswordRequired,
            DownloadError::WrongPassword => DownloadErrorKind::WrongPassword,
            DownloadError::NotFound(_) => DownloadErrorKind::NotFound,
            DownloadError::LoginRequired(_) => DownloadErrorKind::LoginRequired,
            DownloadError::RateLimited(_) => DownloadErrorKind::RateLimited,
            DownloadError::NotAFile(_) => DownloadErrorKind::NotAFile,
            DownloadError::Fatal(_) | DownloadError::Retryable(_) => DownloadErrorKind::Other,
        }
    }
}

impl From<reqwest::Error> for DownloadError {
    fn from(e: reqwest::Error) -> Self {
        DownloadError::Retryable(e.to_string())
    }
}

impl From<std::io::Error> for DownloadError {
    fn from(e: std::io::Error) -> Self {
        DownloadError::Retryable(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_and_retryable() {
        assert!(DownloadError::Retryable("x".into()).is_retryable());
        assert!(!DownloadError::Fatal("x".into()).is_retryable());
        assert!(!DownloadError::WrongPassword.is_retryable());
        assert_eq!(DownloadError::Retryable("x".into()).kind(), DownloadErrorKind::Other);
        assert_eq!(DownloadError::Fatal("x".into()).kind(), DownloadErrorKind::Other);
        assert_eq!(DownloadError::NotAFile("x".into()).kind(), DownloadErrorKind::NotAFile);
        assert_eq!(DownloadError::PasswordRequired.kind(), DownloadErrorKind::PasswordRequired);
    }
}
