use crate::error::{AppError, AppResult};

const SERVICE: &str = "cn.providerdeck.desktop";
const PROXY_SERVICE: &str = "cn.providerdeck.desktop.proxy";

pub fn set(provider_id: &str, secret: &str) -> AppResult<()> {
    if secret.trim().is_empty() { return Err(AppError::InvalidInput("API Key 不能为空".into())); }
    keyring::Entry::new(SERVICE, provider_id)
        .map_err(|e| AppError::Credential(e.to_string()))?
        .set_password(secret)
        .map_err(|e| AppError::Credential(e.to_string()))
}

pub fn get(provider_id: &str) -> AppResult<String> {
    keyring::Entry::new(SERVICE, provider_id)
        .map_err(|e| AppError::Credential(e.to_string()))?
        .get_password()
        .map_err(|e| AppError::Credential(format!("无法读取系统凭据：{e}")))
}

pub fn delete(provider_id: &str) -> AppResult<()> {
    let entry = keyring::Entry::new(SERVICE, provider_id)
        .map_err(|e| AppError::Credential(e.to_string()))?;
    match entry.delete_credential() { Ok(()) | Err(keyring::Error::NoEntry) => {}, Err(e) => return Err(AppError::Credential(e.to_string())) }
    let proxy_entry = keyring::Entry::new(PROXY_SERVICE, provider_id).map_err(|e| AppError::Credential(e.to_string()))?;
    match proxy_entry.delete_credential() { Ok(()) | Err(keyring::Error::NoEntry) => Ok(()), Err(e) => Err(AppError::Credential(e.to_string())) }
}

pub fn proxy_token(provider_id: &str) -> AppResult<String> {
    let entry = keyring::Entry::new(PROXY_SERVICE, provider_id).map_err(|e| AppError::Credential(e.to_string()))?;
    match entry.get_password() {
        Ok(token) if !token.is_empty() => Ok(token),
        Ok(_) | Err(keyring::Error::NoEntry) => {
            let token = format!("pd_local_{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
            entry.set_password(&token).map_err(|e| AppError::Credential(e.to_string()))?;
            Ok(token)
        }
        Err(error) => Err(AppError::Credential(format!("无法读取本地代理令牌：{error}"))),
    }
}
