use crate::error::{AppError, AppResult};

const SERVICE: &str = "cn.providerdeck.desktop";
const PROXY_SERVICE: &str = "cn.providerdeck.desktop.proxy";
const CHAT_BACKUP_SERVICE: &str = "cn.providerdeck.desktop.chat-backup";
const CHAT_BACKUP_ACCOUNT: &str = "encryption-key-v1";

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

pub fn chat_backup_key() -> AppResult<Vec<u8>> {
    let entry = keyring::Entry::new(CHAT_BACKUP_SERVICE, CHAT_BACKUP_ACCOUNT)
        .map_err(|error| AppError::Credential(error.to_string()))?;
    match entry.get_password() {
        Ok(encoded) => hex::decode(encoded)
            .map_err(|error| AppError::Credential(format!("聊天备份密钥格式无效：{error}"))),
        Err(keyring::Error::NoEntry) => {
            use chacha20poly1305::aead::rand_core::{OsRng, RngCore};
            let mut key = [0_u8; 32];
            OsRng.fill_bytes(&mut key);
            entry.set_password(&hex::encode(key))
                .map_err(|error| AppError::Credential(error.to_string()))?;
            Ok(key.to_vec())
        }
        Err(error) => Err(AppError::Credential(error.to_string())),
    }
}
