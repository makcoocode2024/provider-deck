use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use chacha20poly1305::{
    aead::{rand_core::{OsRng, RngCore}, Aead, KeyInit},
    XChaCha20Poly1305, XNonce,
};
use chrono::Utc;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    activity,
    credentials,
    error::{AppError, AppResult},
    storage::atomic_replace,
};

const CACHE_VERSION: u32 = 3;
const BACKUP_VERSION: u32 = 3;
const BACKUP_FORMAT: &str = "provider-deck.codex-chat-backup";
const BACKUP_ALGORITHM: &str = "XChaCha20-Poly1305";
const LEGACY_BACKUP_MAGIC: &[u8; 8] = b"PDBCHAT2";
const NONCE_SIZE: usize = 24;
const KEY_SIZE: usize = 32;
const MAX_IMPORT_SIZE: u64 = 64 * 1024 * 1024;
const MAX_CONVERSATIONS: usize = 128;
const LEGACY_SESSION_ID: &str = "legacy";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatConversation {
    pub response_id: String,
    pub messages: Vec<Value>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default = "legacy_session_id")]
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChatCacheDocument {
    version: u32,
    conversations: Vec<ChatConversation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChatBackupEnvelope {
    version: u32,
    exported_at: String,
    conversations: Vec<ChatConversation>,
}

/// UTF-8 JSON outer container. The payload remains authenticated encrypted data,
/// so a damaged or modified file is rejected before its conversations are restored.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EncryptedChatBackupFile {
    format: String,
    version: u32,
    algorithm: String,
    nonce: String,
    ciphertext: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatBackupRecord {
    pub id: String,
    pub file_name: String,
    pub path: String,
    pub created_at: String,
    pub size: u64,
    pub conversation_count: usize,
    pub version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatCacheSummary {
    pub conversation_count: usize,
    pub current_session_count: usize,
    pub historical_conversation_count: usize,
    pub message_count: usize,
    pub cache_path: String,
    pub backup_directory: String,
    pub cache_status: String,
    pub cache_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRestoreResult {
    pub success: bool,
    pub message: String,
    pub imported_count: usize,
    pub total_count: usize,
    pub current_session_count: usize,
    pub historical_conversation_count: usize,
    pub rollback_snapshot_id: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum ChatRestoreMode {
    Merge,
    Replace,
}

impl ChatRestoreMode {
    pub fn parse(value: &str) -> AppResult<Self> {
        match value {
            "merge" => Ok(Self::Merge),
            "replace" => Ok(Self::Replace),
            _ => Err(AppError::InvalidInput("未知的聊天记录恢复方式。请重新选择合并或覆盖历史会话。".into())),
        }
    }
}

#[derive(Clone)]
pub struct ChatStore {
    inner: Arc<Mutex<HashMap<String, ChatConversation>>>,
    cache_path: PathBuf,
    backup_dir: PathBuf,
    snapshot_dir: PathBuf,
    current_session_id: String,
    cache_error: Arc<Mutex<Option<String>>>,
}

impl ChatStore {
    pub fn load() -> AppResult<Self> {
        let dirs = ProjectDirs::from("cn", "ProviderDeck", "Provider Deck")
            .ok_or_else(|| AppError::Config("无法确定聊天记录数据目录。".into()))?;
        let cache_dir = dirs.data_dir().join("chat-cache");
        let backup_dir = dirs.data_dir().join("chat-backups");
        let snapshot_dir = dirs.data_dir().join("chat-snapshots");
        fs::create_dir_all(&cache_dir)?;
        fs::create_dir_all(&backup_dir)?;
        fs::create_dir_all(&snapshot_dir)?;

        let cache_path = cache_dir.join("conversations.json");
        let (conversations, cache_error) = if cache_path.exists() {
            match fs::read(&cache_path).and_then(|bytes| {
                parse_chat_document(&bytes).map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))
            }) {
                Ok(records) => (records, None),
                Err(error) => (Vec::new(), Some(format!("本地缓存无法读取：{error}"))),
            }
        } else {
            (Vec::new(), None)
        };

        Ok(Self {
            inner: Arc::new(Mutex::new(to_map(conversations))),
            cache_path,
            backup_dir,
            snapshot_dir,
            current_session_id: format!("runtime_{}", Uuid::new_v4().simple()),
            cache_error: Arc::new(Mutex::new(cache_error)),
        })
    }

    pub fn get(&self, response_id: &str) -> Option<Vec<Value>> {
        self.inner.lock().ok()?.get(response_id).map(|record| record.messages.clone())
    }

    pub fn record(&self, response_id: String, messages: Vec<Value>) -> AppResult<()> {
        let now = Utc::now().to_rfc3339();
        {
            let mut guard = self.inner.lock().map_err(|_| AppError::Config("聊天缓存锁已损坏。".into()))?;
            let created_at = guard.get(&response_id).map(|record| record.created_at.clone()).unwrap_or_else(|| now.clone());
            guard.insert(response_id.clone(), ChatConversation {
                response_id,
                messages,
                created_at,
                updated_at: now,
                session_id: self.current_session_id.clone(),
            });
            trim_oldest(&mut guard, &self.current_session_id);
        }
        self.persist()
    }

    pub fn summary(&self) -> ChatCacheSummary {
        let guard = self.inner.lock().expect("chat store mutex poisoned");
        let current_session_count = guard.values().filter(|record| record.session_id == self.current_session_id).count();
        let cache_error = self.cache_error.lock().ok().and_then(|value| value.clone());
        ChatCacheSummary {
            conversation_count: guard.len(),
            current_session_count,
            historical_conversation_count: guard.len().saturating_sub(current_session_count),
            message_count: guard.values().map(|record| record.messages.len()).sum(),
            cache_path: self.cache_path.to_string_lossy().into_owned(),
            backup_directory: self.backup_dir.to_string_lossy().into_owned(),
            cache_status: if cache_error.is_some() { "damaged".into() } else if self.cache_path.exists() { "available".into() } else { "missing".into() },
            cache_message: cache_error,
        }
    }

    pub fn export_backup(&self) -> AppResult<ChatBackupRecord> {
        let conversations = self.current_conversations()?;
        let now = Utc::now();
        let id = Uuid::new_v4().to_string();
        let file_name = format!("provider-deck-codex-chats-{}-{id}.pdbchat.json", now.format("%Y%m%d-%H%M%S"));
        let path = self.backup_dir.join(&file_name);
        let envelope = ChatBackupEnvelope {
            version: BACKUP_VERSION,
            exported_at: now.to_rfc3339(),
            conversations,
        };
        atomic_replace(&path, &encrypt_envelope(&envelope)?)?;
        let record = backup_record(&path, &envelope, Some(id))?;
        activity::record("chat_backup_export", &format!("导出 {} 个聊天会话到 {}", record.conversation_count, record.file_name), true);
        Ok(record)
    }

    pub fn list_backups(&self) -> AppResult<Vec<ChatBackupRecord>> {
        let mut records = Vec::new();
        for entry in fs::read_dir(&self.backup_dir)? {
            let path = entry?.path();
            if !is_chat_backup_file(&path) {
                continue;
            }
            if let Ok(envelope) = read_backup_file(&path) {
                if let Ok(record) = backup_record(&path, &envelope, None) {
                    records.push(record);
                }
            }
        }
        records.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        Ok(records)
    }

    pub fn restore_from_file(&self, path: &Path, mode: ChatRestoreMode) -> ChatRestoreResult {
        self.restore_from_source("本地备份文件", mode, || {
            let metadata = fs::metadata(path).map_err(|_| AppError::InvalidInput("所选备份文件不存在或无法读取。".into()))?;
            if metadata.len() > MAX_IMPORT_SIZE {
                return Err(AppError::InvalidInput("聊天备份文件超过 64 MB 限制。请检查文件是否选择正确。".into()));
            }
            Ok(read_backup_file(path)?.conversations)
        })
    }

    pub fn restore_from_payload(&self, payload: &str, mode: ChatRestoreMode) -> ChatRestoreResult {
        self.restore_from_source("导入的备份文件", mode, || {
            if payload.len() as u64 > MAX_IMPORT_SIZE {
                return Err(AppError::InvalidInput("聊天备份文件超过 64 MB 限制。请检查文件是否选择正确。".into()));
            }
            Ok(read_backup_bytes(payload.as_bytes())?.conversations)
        })
    }

    pub fn restore_from_cache(&self, mode: ChatRestoreMode) -> ChatRestoreResult {
        self.restore_from_source("本地聊天缓存", mode, || {
            if !self.cache_path.exists() {
                return Err(AppError::InvalidInput("未找到本地聊天缓存。请先使用“手动备份导出”，或从已有 .pdbchat.json 备份文件导入。".into()));
            }
            if let Some(error) = self.cache_error.lock().ok().and_then(|value| value.clone()) {
                return Err(AppError::InvalidInput(format!("本地聊天缓存已损坏，无法自动恢复：{error}")));
            }
            parse_chat_document(&fs::read(&self.cache_path)?)
        })
    }

    pub fn rollback(&self, snapshot_id: &str) -> ChatRestoreResult {
        let parsed = match Uuid::parse_str(snapshot_id) {
            Ok(value) => value,
            Err(_) => return failed_result("回滚快照标识无效。".into(), None, self.summary()),
        };
        let path = self.snapshot_dir.join(format!("{parsed}.pdbchat.json"));
        let result = (|| -> AppResult<usize> {
            let envelope = read_backup_file(&path)?;
            self.apply_restore(envelope.conversations, ChatRestoreMode::Replace)
        })();
        match result {
            Ok(count) => {
                activity::record("chat_restore_rollback", &format!("已回滚到快照 {snapshot_id}，恢复 {count} 个会话。"), true);
                let summary = self.summary();
                ChatRestoreResult {
                    success: true,
                    message: format!("已回滚到恢复前状态，恢复 {count} 个会话。"),
                    imported_count: count,
                    total_count: summary.conversation_count,
                    current_session_count: summary.current_session_count,
                    historical_conversation_count: summary.historical_conversation_count,
                    rollback_snapshot_id: None,
                }
            }
            Err(error) => {
                activity::record("chat_restore_rollback", &error.to_string(), false);
                failed_result(format!("回滚失败：{error}"), Some(snapshot_id.into()), self.summary())
            }
        }
    }

    fn restore_from_source<F>(&self, source: &str, mode: ChatRestoreMode, read_conversations: F) -> ChatRestoreResult
    where
        F: FnOnce() -> AppResult<Vec<ChatConversation>>,
    {
        // Snapshot first so both merge and replace can always be undone from the UI.
        let snapshot_id = match self.create_snapshot() {
            Ok(id) => Some(id),
            Err(error) => {
                activity::record("chat_restore", &format!("创建恢复前快照失败：{error}"), false);
                return failed_result(format!("无法创建恢复前快照：{error}"), None, self.summary());
            }
        };
        let result = read_conversations().and_then(|conversations| self.apply_restore(conversations, mode));
        self.restore_outcome(result, snapshot_id, source)
    }

    fn current_conversations(&self) -> AppResult<Vec<ChatConversation>> {
        let guard = self.inner.lock().map_err(|_| AppError::Config("聊天缓存锁已损坏。".into()))?;
        let mut conversations = guard.values().cloned().collect::<Vec<_>>();
        conversations.sort_by(|left, right| left.created_at.cmp(&right.created_at));
        Ok(conversations)
    }

    fn persist(&self) -> AppResult<()> {
        let document = ChatCacheDocument {
            version: CACHE_VERSION,
            conversations: self.current_conversations()?,
        };
        let bytes = serde_json::to_vec_pretty(&document)
            .map_err(|error| AppError::Config(format!("聊天缓存序列化失败：{error}")))?;
        atomic_replace(&self.cache_path, &bytes)?;
        if let Ok(mut cache_error) = self.cache_error.lock() {
            *cache_error = None;
        }
        Ok(())
    }

    fn create_snapshot(&self) -> AppResult<String> {
        let id = Uuid::new_v4().to_string();
        let path = self.snapshot_dir.join(format!("{id}.pdbchat.json"));
        let envelope = ChatBackupEnvelope {
            version: BACKUP_VERSION,
            exported_at: Utc::now().to_rfc3339(),
            conversations: self.current_conversations()?,
        };
        atomic_replace(&path, &encrypt_envelope(&envelope)?)?;
        Ok(id)
    }

    fn apply_restore(&self, imported: Vec<ChatConversation>, mode: ChatRestoreMode) -> AppResult<usize> {
        let imported_count = imported.len();
        {
            let mut guard = self.inner.lock().map_err(|_| AppError::Config("聊天缓存锁已损坏。".into()))?;
            if matches!(mode, ChatRestoreMode::Replace) {
                // Never remove the conversations created by the currently running Provider Deck process.
                guard.retain(|_, record| record.session_id == self.current_session_id);
            }
            for mut record in imported {
                validate_conversation(&record)?;
                // Imported records are history. This is what protects the currently active session in replace mode.
                record.session_id = format!("restored_{}", Uuid::new_v4().simple());
                if let Some(existing) = guard.get(&record.response_id) {
                    if existing.messages == record.messages {
                        continue;
                    }
                    record.response_id = format!("imported_{}", Uuid::new_v4().simple());
                }
                guard.insert(record.response_id.clone(), record);
            }
            trim_oldest(&mut guard, &self.current_session_id);
        }
        self.persist()?;
        Ok(imported_count)
    }

    fn restore_outcome(&self, result: AppResult<usize>, snapshot_id: Option<String>, source: &str) -> ChatRestoreResult {
        match result {
            Ok(imported_count) => {
                let summary = self.summary();
                activity::record("chat_restore", &format!("从{source}恢复 {imported_count} 个会话，当前共 {} 个会话。", summary.conversation_count), true);
                ChatRestoreResult {
                    success: true,
                    message: format!("已从{source}恢复 {imported_count} 个会话。当前会话已保留，Provider 配置没有改动。"),
                    imported_count,
                    total_count: summary.conversation_count,
                    current_session_count: summary.current_session_count,
                    historical_conversation_count: summary.historical_conversation_count,
                    rollback_snapshot_id: snapshot_id,
                }
            }
            Err(error) => {
                let summary = self.summary();
                activity::record("chat_restore", &format!("从{source}恢复失败：{error}"), false);
                failed_result(
                    format!("恢复失败：{error}。已保留恢复前快照，可使用“一键回滚”。"),
                    snapshot_id,
                    summary,
                )
            }
        }
    }
}

fn legacy_session_id() -> String {
    LEGACY_SESSION_ID.into()
}

fn failed_result(message: String, snapshot_id: Option<String>, summary: ChatCacheSummary) -> ChatRestoreResult {
    ChatRestoreResult {
        success: false,
        message,
        imported_count: 0,
        total_count: summary.conversation_count,
        current_session_count: summary.current_session_count,
        historical_conversation_count: summary.historical_conversation_count,
        rollback_snapshot_id: snapshot_id,
    }
}

fn to_map(mut conversations: Vec<ChatConversation>) -> HashMap<String, ChatConversation> {
    for conversation in &mut conversations {
        if conversation.session_id.trim().is_empty() {
            conversation.session_id = legacy_session_id();
        }
    }
    conversations.into_iter().map(|record| (record.response_id.clone(), record)).collect()
}

fn trim_oldest(conversations: &mut HashMap<String, ChatConversation>, current_session_id: &str) {
    while conversations.len() > MAX_CONVERSATIONS {
        let candidate = conversations.values().min_by(|left, right| {
            let left_priority = usize::from(left.session_id == current_session_id);
            let right_priority = usize::from(right.session_id == current_session_id);
            left_priority.cmp(&right_priority).then_with(|| left.updated_at.cmp(&right.updated_at))
        }).map(|record| record.response_id.clone());
        if let Some(id) = candidate {
            conversations.remove(&id);
        } else {
            break;
        }
    }
}

fn validate_conversation(record: &ChatConversation) -> AppResult<()> {
    if record.response_id.trim().is_empty() {
        return Err(AppError::InvalidInput("聊天记录缺少 response_id。请重新导出完整备份。".into()));
    }
    if record.messages.len() > 10_000 {
        return Err(AppError::InvalidInput("单个聊天会话的消息数量异常，请检查备份文件。".into()));
    }
    Ok(())
}

fn parse_chat_document(bytes: &[u8]) -> AppResult<Vec<ChatConversation>> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|error| AppError::InvalidInput(format!("聊天数据格式错误或文件已损坏：{error}")))?;
    if let Some(version) = value.get("version").and_then(Value::as_u64) {
        if version > CACHE_VERSION as u64 {
            return Err(AppError::InvalidInput(format!("聊天数据版本 {version} 高于当前支持版本 {CACHE_VERSION}。请更新 Provider Deck 后重试。")));
        }
        let conversations = value.get("conversations").cloned().unwrap_or_else(|| Value::Array(Vec::new()));
        let records: Vec<ChatConversation> = serde_json::from_value(conversations)
            .map_err(|error| AppError::InvalidInput(format!("聊天记录结构无效：{error}")))?;
        return Ok(records);
    }
    if value.is_array() {
        return serde_json::from_value(value)
            .map_err(|error| AppError::InvalidInput(format!("旧版聊天记录结构无效：{error}")));
    }
    if let Some(object) = value.as_object() {
        let now = Utc::now().to_rfc3339();
        let mut conversations = Vec::new();
        for (response_id, messages) in object {
            let messages = messages.as_array().cloned()
                .ok_or_else(|| AppError::InvalidInput("旧版聊天缓存中的消息列表无效。".into()))?;
            conversations.push(ChatConversation {
                response_id: response_id.clone(),
                messages,
                created_at: now.clone(),
                updated_at: now.clone(),
                session_id: legacy_session_id(),
            });
        }
        return Ok(conversations);
    }
    Err(AppError::InvalidInput("无法识别聊天记录文件格式。".into()))
}

fn encrypt_envelope(envelope: &ChatBackupEnvelope) -> AppResult<Vec<u8>> {
    let key = credentials::chat_backup_key()?;
    encrypt_envelope_with_key(envelope, &key)
}

fn encrypt_envelope_with_key(envelope: &ChatBackupEnvelope, key: &[u8]) -> AppResult<Vec<u8>> {
    if key.len() != KEY_SIZE {
        return Err(AppError::Credential("聊天备份密钥长度无效。".into()));
    }
    let cipher = XChaCha20Poly1305::new_from_slice(key)
        .map_err(|_| AppError::Credential("无法初始化聊天备份加密器。".into()))?;
    let mut nonce_bytes = [0_u8; NONCE_SIZE];
    OsRng.fill_bytes(&mut nonce_bytes);
    let plaintext = serde_json::to_vec(envelope).map_err(|error| AppError::Config(error.to_string()))?;
    let ciphertext = cipher.encrypt(XNonce::from_slice(&nonce_bytes), plaintext.as_ref())
        .map_err(|_| AppError::Config("聊天备份加密失败。".into()))?;
    let encrypted = EncryptedChatBackupFile {
        format: BACKUP_FORMAT.into(),
        version: BACKUP_VERSION,
        algorithm: BACKUP_ALGORITHM.into(),
        nonce: hex::encode(nonce_bytes),
        ciphertext: hex::encode(ciphertext),
    };
    serde_json::to_vec_pretty(&encrypted)
        .map_err(|error| AppError::Config(format!("加密聊天备份序列化失败：{error}")))
}

fn read_backup_file(path: &Path) -> AppResult<ChatBackupEnvelope> {
    let bytes = fs::read(path).map_err(|_| AppError::InvalidInput("备份文件不存在或无法读取。".into()))?;
    if bytes.len() as u64 > MAX_IMPORT_SIZE {
        return Err(AppError::InvalidInput("聊天备份文件超过 64 MB 限制。请检查文件是否选择正确。".into()));
    }
    read_backup_bytes(&bytes)
}

fn read_backup_bytes(bytes: &[u8]) -> AppResult<ChatBackupEnvelope> {
    if bytes.starts_with(LEGACY_BACKUP_MAGIC) {
        return read_legacy_binary_backup(bytes);
    }

    if let Ok(encrypted) = serde_json::from_slice::<EncryptedChatBackupFile>(bytes) {
        return decrypt_json_backup(encrypted);
    }

    // Keeps compatibility with historical, unencrypted Provider Deck cache exports.
    let conversations = parse_chat_document(bytes)?;
    Ok(ChatBackupEnvelope {
        version: 1,
        exported_at: Utc::now().to_rfc3339(),
        conversations,
    })
}

fn decrypt_json_backup(encrypted: EncryptedChatBackupFile) -> AppResult<ChatBackupEnvelope> {
    if encrypted.format != BACKUP_FORMAT {
        return Err(AppError::InvalidInput("不是 Provider Deck 的聊天备份文件。请选择 .pdbchat.json 文件。".into()));
    }
    if encrypted.version == 0 || encrypted.version > BACKUP_VERSION {
        return Err(AppError::InvalidInput(format!("不支持的聊天备份版本：{}。请更新 Provider Deck 后重试。", encrypted.version)));
    }
    if encrypted.algorithm != BACKUP_ALGORITHM {
        return Err(AppError::InvalidInput(format!("不支持的聊天备份加密算法：{}。", encrypted.algorithm)));
    }
    let nonce = hex::decode(&encrypted.nonce)
        .map_err(|_| AppError::InvalidInput("备份文件的加密随机数无效或已损坏。".into()))?;
    let ciphertext = hex::decode(&encrypted.ciphertext)
        .map_err(|_| AppError::InvalidInput("备份文件的密文无效或已损坏。".into()))?;
    decrypt_backup_payload(&nonce, &ciphertext)
}

fn read_legacy_binary_backup(bytes: &[u8]) -> AppResult<ChatBackupEnvelope> {
    if bytes.len() <= LEGACY_BACKUP_MAGIC.len() + NONCE_SIZE {
        return Err(AppError::InvalidInput("旧版加密备份文件不完整或已损坏。".into()));
    }
    decrypt_backup_payload(
        &bytes[LEGACY_BACKUP_MAGIC.len()..LEGACY_BACKUP_MAGIC.len() + NONCE_SIZE],
        &bytes[LEGACY_BACKUP_MAGIC.len() + NONCE_SIZE..],
    )
}

fn decrypt_backup_payload(nonce: &[u8], ciphertext: &[u8]) -> AppResult<ChatBackupEnvelope> {
    if nonce.len() != NONCE_SIZE || ciphertext.is_empty() {
        return Err(AppError::InvalidInput("备份文件不完整或已损坏。".into()));
    }
    let key = credentials::chat_backup_key()?;
    decrypt_backup_payload_with_key(nonce, ciphertext, &key)
}

fn decrypt_backup_payload_with_key(nonce: &[u8], ciphertext: &[u8], key: &[u8]) -> AppResult<ChatBackupEnvelope> {
    if key.len() != KEY_SIZE {
        return Err(AppError::Credential("聊天备份密钥长度无效。".into()));
    }
    let cipher = XChaCha20Poly1305::new_from_slice(key)
        .map_err(|_| AppError::Credential("无法初始化聊天备份解密器。".into()))?;
    let plaintext = cipher.decrypt(XNonce::from_slice(nonce), ciphertext)
        .map_err(|_| AppError::InvalidInput("备份无法解密：文件已损坏、被修改，或不是由当前系统用户导出的备份。".into()))?;
    let envelope: ChatBackupEnvelope = serde_json::from_slice(&plaintext)
        .map_err(|error| AppError::InvalidInput(format!("备份内容格式错误：{error}")))?;
    validate_backup_version(envelope)
}

fn validate_backup_version(envelope: ChatBackupEnvelope) -> AppResult<ChatBackupEnvelope> {
    if envelope.version == 0 || envelope.version > BACKUP_VERSION {
        return Err(AppError::InvalidInput(format!("不支持的聊天备份版本：{}。请更新 Provider Deck 后重试。", envelope.version)));
    }
    Ok(envelope)
}

fn is_chat_backup_file(path: &Path) -> bool {
    path.file_name().and_then(|value| value.to_str())
        .map(|name| name.ends_with(".pdbchat.json") || name.ends_with(".pdbchat"))
        .unwrap_or(false)
}

fn backup_record(path: &Path, envelope: &ChatBackupEnvelope, id: Option<String>) -> AppResult<ChatBackupRecord> {
    let file_name = path.file_name().and_then(|value| value.to_str()).unwrap_or("chat-backup.pdbchat.json").to_string();
    let detected_id = id.or_else(|| file_name.strip_suffix(".pdbchat.json").or_else(|| file_name.strip_suffix(".pdbchat"))
        .and_then(|name| name.rsplit_once('-').map(|(_, tail)| tail.to_string())))
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    Ok(ChatBackupRecord {
        id: detected_id,
        file_name,
        path: path.to_string_lossy().into_owned(),
        created_at: envelope.exported_at.clone(),
        size: fs::metadata(path)?.len(),
        conversation_count: envelope.conversations.len(),
        version: envelope.version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conversation(id: &str) -> ChatConversation {
        ChatConversation {
            response_id: id.into(),
            messages: vec![serde_json::json!({"role": "user", "content": "hi"})],
            created_at: "2026-08-10T00:00:00Z".into(),
            updated_at: "2026-08-10T00:00:00Z".into(),
            session_id: legacy_session_id(),
        }
    }

    #[test]
    fn reads_legacy_response_map() {
        let conversations = parse_chat_document(br#"{"resp_old":[{"role":"user","content":"hi"}]}"#).unwrap();
        assert_eq!(conversations.len(), 1);
        assert_eq!(conversations[0].response_id, "resp_old");
        assert_eq!(conversations[0].session_id, LEGACY_SESSION_ID);
    }

    #[test]
    fn rejects_newer_cache_version() {
        let error = parse_chat_document(br#"{"version":99,"conversations":[]}"#).unwrap_err();
        assert!(error.to_string().contains("高于当前支持版本"));
    }

    #[test]
    fn encrypted_json_round_trips_and_detects_tampering() {
        let envelope = ChatBackupEnvelope {
            version: BACKUP_VERSION,
            exported_at: "2026-08-10T00:00:00Z".into(),
            conversations: vec![conversation("resp_test")],
        };
        let key = [7_u8; KEY_SIZE];
        let bytes = encrypt_envelope_with_key(&envelope, &key).unwrap();
        let encrypted: EncryptedChatBackupFile = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(encrypted.format, BACKUP_FORMAT);
        let nonce = hex::decode(encrypted.nonce).unwrap();
        let ciphertext = hex::decode(encrypted.ciphertext).unwrap();
        let decoded = decrypt_backup_payload_with_key(&nonce, &ciphertext, &key).unwrap();
        assert_eq!(decoded.conversations.len(), 1);

        let mut altered = ciphertext;
        altered[0] ^= 0x01;
        assert!(decrypt_backup_payload_with_key(&nonce, &altered, &key).is_err());
    }

    #[test]
    fn trim_preserves_current_session_before_history() {
        let current = "runtime_current";
        let mut conversations = HashMap::new();
        for index in 0..=MAX_CONVERSATIONS {
            let mut record = conversation(&format!("resp_{index}"));
            record.updated_at = format!("2026-08-10T00:00:{index:02}Z");
            record.session_id = if index == MAX_CONVERSATIONS { current.into() } else { "history".into() };
            conversations.insert(record.response_id.clone(), record);
        }
        trim_oldest(&mut conversations, current);
        assert_eq!(conversations.len(), MAX_CONVERSATIONS);
        assert!(conversations.values().any(|record| record.session_id == current));
    }
}
