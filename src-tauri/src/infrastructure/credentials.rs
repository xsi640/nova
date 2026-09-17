use keyring::{Entry, Error as KeyringError};

use crate::error::AppError;

const SERVICE_NAME: &str = "app.nova.companion";

pub fn set_secret(reference: &str, secret: &str) -> Result<(), AppError> {
    entry(reference)?
        .set_password(secret)
        .map_err(|error| credential_error("保存 API Key", error))
}

pub fn get_secret(reference: &str) -> Result<String, AppError> {
    entry(reference)?
        .get_password()
        .map_err(|error| credential_error("读取 API Key", error))
}

pub fn secret_exists(reference: &str) -> Result<bool, AppError> {
    match entry(reference)?.get_password() {
        Ok(_) => Ok(true),
        Err(KeyringError::NoEntry) => Ok(false),
        Err(error) => Err(credential_error("检查 API Key", error)),
    }
}

fn entry(reference: &str) -> Result<Entry, AppError> {
    Entry::new(SERVICE_NAME, reference).map_err(|error| credential_error("访问系统凭据存储", error))
}

fn credential_error(action: &str, error: KeyringError) -> AppError {
    AppError::PlatformPermission(format!("{action}失败：{error}"))
}
