use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("configuration error: {0}")]
    Configuration(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("authorization error: {0}")]
    Authorization(String),
    #[error("service error: {0}")]
    Service(String),
    #[error("audio error: {0}")]
    Audio(String),
    #[error("database error: {0}")]
    Database(String),
    #[error("platform permission error: {0}")]
    PlatformPermission(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl AppError {
    pub fn database(message: impl Into<String>) -> Self {
        Self::Database(message.into())
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub category: ErrorCategory,
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCategory {
    Configuration,
    Network,
    Authorization,
    Service,
    Audio,
    Database,
    PlatformPermission,
    Internal,
}

impl From<AppError> for CommandError {
    fn from(error: AppError) -> Self {
        let (category, code, retryable) = match &error {
            AppError::Configuration(_) => (ErrorCategory::Configuration, "CONFIGURATION", false),
            AppError::Network(_) => (ErrorCategory::Network, "NETWORK", true),
            AppError::Authorization(_) => (ErrorCategory::Authorization, "AUTHORIZATION", false),
            AppError::Service(_) => (ErrorCategory::Service, "SERVICE", true),
            AppError::Audio(_) => (ErrorCategory::Audio, "AUDIO", true),
            AppError::Database(_) => (ErrorCategory::Database, "DATABASE", true),
            AppError::PlatformPermission(_) => (
                ErrorCategory::PlatformPermission,
                "PLATFORM_PERMISSION",
                false,
            ),
            AppError::Internal(_) => (ErrorCategory::Internal, "INTERNAL", false),
        };

        Self {
            category,
            code: code.to_owned(),
            message: error.to_string(),
            retryable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AppError, CommandError, ErrorCategory};

    #[test]
    fn network_errors_are_retryable() {
        let error = CommandError::from(AppError::Network("offline".to_owned()));

        assert!(matches!(error.category, ErrorCategory::Network));
        assert!(error.retryable);
        assert_eq!(error.code, "NETWORK");
    }

    #[test]
    fn authorization_errors_are_not_retryable() {
        let error = CommandError::from(AppError::Authorization("invalid key".to_owned()));

        assert!(matches!(error.category, ErrorCategory::Authorization));
        assert!(!error.retryable);
    }
}
