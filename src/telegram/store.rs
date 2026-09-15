use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use aes::Aes256;
use base64::Engine;
use cbc::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use super::models::{
    TelegramAccountRole, TelegramChat, TelegramDeliveryMode, TelegramUserAccount, TelegramUserGroup,
};
use super::order::TelegramOrderDraft;

#[derive(Debug, thiserror::Error)]
pub enum TelegramStoreError {
    #[error("telegram store read failed")]
    Read,
    #[error("telegram store write failed")]
    Write,
    #[error("telegram invite not found")]
    InviteNotFound,
    #[error("telegram invite already used")]
    InviteAlreadyUsed,
    #[error("telegram invite expired")]
    InviteExpired,
    #[error("telegram user account not found")]
    UserNotFound,
    #[error("telegram user session encryption failed")]
    SessionCrypto,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TelegramStoreData {
    #[serde(default)]
    bot_username: String,
    #[serde(default)]
    bot_token: String,
    #[serde(default)]
    invites: BTreeMap<String, TelegramInviteRecord>,
    #[serde(default)]
    users: BTreeMap<String, TelegramUserAccount>,
    #[serde(default)]
    chats: BTreeMap<String, TelegramChat>,
    #[serde(default)]
    user_sessions: BTreeMap<String, String>,
    #[serde(default)]
    update_offset: i64,
    #[serde(default)]
    order_drafts: BTreeMap<String, TelegramOrderDraft>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TelegramInviteRecord {
    role: TelegramAccountRole,
    created_at_unix: i64,
    #[serde(default)]
    expires_at_unix: i64,
    #[serde(default)]
    claimed_by: Option<String>,
}

const INVITE_TTL_SECONDS: i64 = 15 * 60;
const MAX_STORED_INVITES: usize = 200;

fn purge_expired_invites(data: &mut TelegramStoreData, now_unix: i64) {
    // Muddatdagilarni o'chirish + limit: eng eskilardan boshlab tozalash.
    data.invites
        .retain(|_, invite| invite.expires_at_unix <= 0 || now_unix <= invite.expires_at_unix);
    while data.invites.len() > MAX_STORED_INVITES {
        let Some(oldest) = data
            .invites
            .iter()
            .min_by_key(|(_, invite)| invite.created_at_unix)
            .map(|(token, _)| token.clone())
        else {
            break;
        };
        data.invites.remove(&oldest);
    }
}

#[derive(Debug)]
pub struct TelegramStore {
    path: PathBuf,
    key_path: PathBuf,
    data: Mutex<TelegramStoreData>,
}

impl TelegramStore {
    pub fn new(path: PathBuf) -> Self {
        let data = read_data(&path).unwrap_or_else(|error| {
            tracing::warn!(%error, path = %path.display(), "telegram store unavailable; using empty state");
            TelegramStoreData::default()
        });
        // Sessiya kaliti data fayl yonida alohida 0600 faylda yashaydi
        // (mobile_telegram.json -> mobile_telegram.key). Operator hech narsa
        // kiritmaydi: fayl bo'lmasa birinchi talabda avtomatik yaratiladi.
        let key_path = path.with_extension("key");
        Self {
            path,
            key_path,
            data: Mutex::new(data),
        }
    }

    fn session_key(&self) -> Result<[u8; 32], TelegramStoreError> {
        if let Ok(value) = std::env::var("TELEGRAM_USER_SESSION_KEY") {
            let value = value.trim().to_string();
            if !value.is_empty() {
                return Ok(derive_key(&value));
            }
        }
        load_or_create_key_file(&self.key_path)
    }

    pub async fn bot_settings(&self) -> Result<(String, String), TelegramStoreError> {
        let data = self.data.lock().await;
        Ok((data.bot_username.clone(), data.bot_token.clone()))
    }

    pub async fn users(&self) -> Result<Vec<TelegramUserAccount>, TelegramStoreError> {
        let data = self.data.lock().await;
        Ok(data.users.values().cloned().collect())
    }

    pub async fn user_by_telegram_id(
        &self,
        telegram_user_id: &str,
    ) -> Result<Option<TelegramUserAccount>, TelegramStoreError> {
        let data = self.data.lock().await;
        Ok(data.users.get(telegram_user_id).cloned())
    }

    pub async fn delete_user_account(
        &self,
        telegram_user_id: &str,
    ) -> Result<TelegramUserAccount, TelegramStoreError> {
        let mut data = self.data.lock().await;
        let mut updated = data.clone();
        let user = updated
            .users
            .remove(telegram_user_id)
            .ok_or(TelegramStoreError::UserNotFound)?;
        updated.user_sessions.remove(telegram_user_id);
        updated.order_drafts.remove(telegram_user_id);
        self.persist(&updated).await?;
        *data = updated;
        Ok(user)
    }

    pub async fn user_by_phone(
        &self,
        phone_number: &str,
    ) -> Result<Option<TelegramUserAccount>, TelegramStoreError> {
        let normalized_phone = normalize_phone(phone_number);
        if normalized_phone.is_empty() {
            return Ok(None);
        }
        let data = self.data.lock().await;
        Ok(data
            .users
            .values()
            .find(|user| normalize_phone(&user.phone_number) == normalized_phone)
            .cloned())
    }

    pub async fn chats(&self) -> Result<Vec<TelegramChat>, TelegramStoreError> {
        let data = self.data.lock().await;
        Ok(data.chats.values().cloned().collect())
    }

    pub async fn update_offset(&self) -> Result<i64, TelegramStoreError> {
        let data = self.data.lock().await;
        Ok(data.update_offset)
    }

    pub async fn set_update_offset(&self, update_offset: i64) -> Result<(), TelegramStoreError> {
        let mut data = self.data.lock().await;
        if update_offset <= data.update_offset {
            return Ok(());
        }
        data.update_offset = update_offset;
        self.persist(&data).await
    }

    pub async fn set_bot_settings(
        &self,
        bot_username: String,
        bot_token: Option<String>,
    ) -> Result<(), TelegramStoreError> {
        let mut data = self.data.lock().await;
        data.bot_username = bot_username;
        if let Some(bot_token) = bot_token {
            data.bot_token = bot_token;
        }
        self.persist(&data).await
    }

    pub async fn set_delivery_mode(
        &self,
        telegram_user_id: &str,
        delivery_mode: TelegramDeliveryMode,
    ) -> Result<TelegramUserAccount, TelegramStoreError> {
        let mut data = self.data.lock().await;
        let user = data
            .users
            .get_mut(telegram_user_id)
            .ok_or(TelegramStoreError::UserNotFound)?;
        user.delivery_mode = delivery_mode;
        let user = user.clone();
        self.persist(&data).await?;
        Ok(user)
    }

    /// Only the authenticated MTProto identity may create a QR user. Never
    /// overwrite an existing user's role/session when another QR is scanned.
    pub(crate) async fn register_qr_user(
        &self,
        user: TelegramUserAccount,
        session: String,
    ) -> Result<Option<TelegramUserAccount>, TelegramStoreError> {
        let mut data = self.data.lock().await;
        if data.users.contains_key(&user.telegram_user_id) {
            return Ok(None);
        }
        let encrypted = encrypt_session(&session, &self.session_key()?)?;
        let mut updated = data.clone();
        updated
            .user_sessions
            .insert(user.telegram_user_id.clone(), encrypted);
        updated
            .users
            .insert(user.telegram_user_id.clone(), user.clone());
        self.persist(&updated).await?;
        *data = updated;
        Ok(Some(user))
    }

    pub(crate) async fn recognize_start(
        &self,
        id: &str,
        chat_id: &str,
    ) -> Result<Option<TelegramUserAccount>, TelegramStoreError> {
        let mut data = self.data.lock().await;
        let mut updated = data.clone();
        let Some(user) = updated.users.get_mut(id) else {
            return Ok(None);
        };
        user.telegram_chat_id = chat_id.to_string();
        let user = user.clone();
        self.persist(&updated).await?;
        *data = updated;
        Ok(Some(user))
    }

    pub async fn complete_user_profile_login(
        &self,
        telegram_user_id: &str,
        phone_number: String,
        session_string: String,
    ) -> Result<TelegramUserAccount, TelegramStoreError> {
        // New logins always use the dedicated session key (auto-created key
        // file, or TELEGRAM_USER_SESSION_KEY override). Legacy keys are
        // read-only fallbacks for previously stored sessions (see user_session).
        let key = self.session_key()?;
        let mut data = self.data.lock().await;
        let user = {
            let user = data
                .users
                .get_mut(telegram_user_id)
                .ok_or(TelegramStoreError::UserNotFound)?;
            user.phone_number = phone_number;
            user.user_profile_connected = true;
            user.clone()
        };
        let encrypted_session = encrypt_session(&session_string, &key)?;
        data.user_sessions
            .insert(telegram_user_id.to_string(), encrypted_session);
        self.persist(&data).await?;
        Ok(user)
    }

    pub async fn user_session(
        &self,
        telegram_user_id: &str,
    ) -> Result<Option<String>, TelegramStoreError> {
        let (session, legacy_keys) = {
            let data = self.data.lock().await;
            let session = data.user_sessions.get(telegram_user_id).cloned();
            let legacy_keys = legacy_session_keys(&data);
            (session, legacy_keys)
        };
        let Some(session) = session else {
            return Ok(None);
        };
        if !session.starts_with("v1:") {
            return Ok(Some(session));
        }
        // Preferred path: dedicated key (auto-created file or env override).
        // load_or_create_key_file essentially never fails here (it creates the
        // file on demand); the branch below is only a last-resort compat path.
        if let Ok(key) = self.session_key() {
            match decrypt_session(&session, &key) {
                Ok(plain) => return Ok(Some(plain)),
                Err(_) => {
                    // Fall through to legacy keys below (migration path).
                }
            }
            for legacy_key in &legacy_keys {
                if let Ok(plain) = decrypt_session(&session, legacy_key) {
                    tracing::warn!(
                        telegram_user_id = %telegram_user_id,
                        "telegram user session decrypted with legacy key; re-encrypting with dedicated session key"
                    );
                    let migrated = encrypt_session(&plain, &key)?;
                    let mut data = self.data.lock().await;
                    data.user_sessions
                        .insert(telegram_user_id.to_string(), migrated);
                    self.persist(&data).await?;
                    return Ok(Some(plain));
                }
            }
            return Err(TelegramStoreError::SessionCrypto);
        }
        // Last-resort compat path (key file unwritable and no env override):
        // old sessions keep working read-only so the userbot does not die.
        tracing::warn!(
            "telegram session key unavailable; trying legacy telegram session keys read-only"
        );
        for legacy_key in &legacy_keys {
            if let Ok(plain) = decrypt_session(&session, legacy_key) {
                return Ok(Some(plain));
            }
        }
        Err(TelegramStoreError::SessionCrypto)
    }

    pub async fn set_selected_user_group(
        &self,
        telegram_user_id: &str,
        group: TelegramUserGroup,
    ) -> Result<TelegramUserAccount, TelegramStoreError> {
        let mut data = self.data.lock().await;
        let user = data
            .users
            .get_mut(telegram_user_id)
            .ok_or(TelegramStoreError::UserNotFound)?;
        user.selected_chat_id = Some(group.chat_id);
        user.selected_chat_title = Some(group.title);
        user.selected_chat_type = Some(group.chat_type);
        let user = user.clone();
        self.persist(&data).await?;
        Ok(user)
    }

    pub async fn order_draft(
        &self,
        telegram_user_id: &str,
    ) -> Result<Option<TelegramOrderDraft>, TelegramStoreError> {
        let data = self.data.lock().await;
        Ok(data.order_drafts.get(telegram_user_id).cloned())
    }

    pub async fn save_order_draft(
        &self,
        telegram_user_id: &str,
        draft: TelegramOrderDraft,
    ) -> Result<(), TelegramStoreError> {
        let mut data = self.data.lock().await;
        data.order_drafts
            .insert(telegram_user_id.to_string(), draft);
        self.persist(&data).await
    }

    pub async fn clear_order_draft(
        &self,
        telegram_user_id: &str,
    ) -> Result<(), TelegramStoreError> {
        let mut data = self.data.lock().await;
        data.order_drafts.remove(telegram_user_id);
        self.persist(&data).await
    }

    pub async fn create_invite(
        &self,
        token: String,
        role: TelegramAccountRole,
        now_unix: i64,
    ) -> Result<(), TelegramStoreError> {
        let mut data = self.data.lock().await;
        purge_expired_invites(&mut data, now_unix);
        data.invites.insert(
            token,
            TelegramInviteRecord {
                role,
                created_at_unix: now_unix,
                expires_at_unix: now_unix + INVITE_TTL_SECONDS,
                claimed_by: None,
            },
        );
        self.persist(&data).await
    }

    pub async fn claim_invite(
        &self,
        token: &str,
        telegram_user_id: String,
        telegram_chat_id: String,
        username: String,
        display_name: String,
        now_unix: i64,
    ) -> Result<TelegramUserAccount, TelegramStoreError> {
        let mut data = self.data.lock().await;
        purge_expired_invites(&mut data, now_unix);
        let invite = data
            .invites
            .get(token)
            .ok_or(TelegramStoreError::InviteNotFound)?;
        if invite.expires_at_unix > 0 && now_unix > invite.expires_at_unix {
            return Err(TelegramStoreError::InviteExpired);
        }
        if let Some(claimed_by) = invite.claimed_by.as_deref()
            && claimed_by != telegram_user_id
        {
            return Err(TelegramStoreError::InviteAlreadyUsed);
        }
        let role = invite.role;
        let existing = data.users.get(&telegram_user_id).cloned();
        let joined_at_unix = existing
            .as_ref()
            .map(|existing| existing.joined_at_unix)
            .unwrap_or(now_unix);

        let user = TelegramUserAccount {
            telegram_user_id: telegram_user_id.clone(),
            telegram_chat_id,
            username,
            display_name,
            role,
            invite_token: token.to_string(),
            joined_at_unix,
            phone_number: existing
                .as_ref()
                .map(|existing| existing.phone_number.clone())
                .unwrap_or_default(),
            delivery_mode: existing
                .as_ref()
                .map(|existing| existing.delivery_mode)
                .unwrap_or_default(),
            user_profile_connected: existing
                .as_ref()
                .is_some_and(|existing| existing.user_profile_connected),
            selected_chat_id: existing
                .as_ref()
                .and_then(|existing| existing.selected_chat_id.clone()),
            selected_chat_title: existing
                .as_ref()
                .and_then(|existing| existing.selected_chat_title.clone()),
            selected_chat_type: existing
                .as_ref()
                .and_then(|existing| existing.selected_chat_type.clone()),
        };
        data.invites
            .get_mut(token)
            .ok_or(TelegramStoreError::InviteNotFound)?
            .claimed_by = Some(telegram_user_id.clone());
        data.users.insert(telegram_user_id, user.clone());
        self.persist(&data).await?;
        Ok(user)
    }

    pub async fn upsert_chat(&self, chat: TelegramChat) -> Result<(), TelegramStoreError> {
        let mut data = self.data.lock().await;
        let chat_id = chat.chat_id.clone();
        let connected_at_unix = data
            .chats
            .get(&chat_id)
            .map(|existing| existing.connected_at_unix)
            .unwrap_or(chat.connected_at_unix);
        data.chats.insert(
            chat_id,
            TelegramChat {
                connected_at_unix,
                ..chat
            },
        );
        self.persist(&data).await
    }

    async fn persist(&self, data: &TelegramStoreData) -> Result<(), TelegramStoreError> {
        let parent = self.path.parent().ok_or(TelegramStoreError::Write)?;
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|_| TelegramStoreError::Write)?;
        let raw = serde_json::to_vec_pretty(data).map_err(|_| TelegramStoreError::Write)?;
        let tmp_path = self.path.with_extension("json.tmp");
        tokio::fs::write(&tmp_path, raw)
            .await
            .map_err(|_| TelegramStoreError::Write)?;
        restrict_file_permissions(&tmp_path).await;
        tokio::fs::rename(tmp_path, &self.path)
            .await
            .map_err(|_| TelegramStoreError::Write)?;
        restrict_file_permissions(&self.path).await;
        Ok(())
    }
}

fn normalize_phone(value: &str) -> String {
    value.chars().filter(char::is_ascii_digit).collect()
}

type SessionEncryptor = cbc::Encryptor<Aes256>;
type SessionDecryptor = cbc::Decryptor<Aes256>;

fn derive_key(source: &str) -> [u8; 32] {
    let digest = Sha256::digest(source.as_bytes());
    let mut key = [0_u8; 32];
    key.copy_from_slice(&digest);
    key
}

fn load_or_create_key_file(path: &Path) -> Result<[u8; 32], TelegramStoreError> {
    // Nol manual qadam: fayl bo'lsa o'qiladi, bo'lmasa 32 bayt random
    // yaratilib 0600 bilan saqlanadi. Env override har doim ustun.
    if let Ok(raw) = std::fs::read_to_string(path)
        && let Some(key) = parse_key(&raw)
    {
        return Ok(key);
    }
    let key: [u8; 32] = rand::random();
    let hex: String = key.iter().map(|byte| format!("{byte:02x}")).collect();
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|_| TelegramStoreError::SessionCrypto)?;
    }
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            use std::io::Write;
            file.write_all(hex.as_bytes())
                .map_err(|_| TelegramStoreError::SessionCrypto)?;
            restrict_file_permissions_sync(path);
            tracing::info!(path = %path.display(), "telegram session key file created");
        }
        // Yonma-yon ikki process bir vaqtda yaratmoqchi bo'lsa: yutganining
        // kalitini o'qiymiz, o'zimiznikini ustiga yozmaymiz.
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let raw =
                std::fs::read_to_string(path).map_err(|_| TelegramStoreError::SessionCrypto)?;
            return parse_key(&raw).ok_or(TelegramStoreError::SessionCrypto);
        }
        Err(_) => return Err(TelegramStoreError::SessionCrypto),
    }
    Ok(key)
}

fn parse_key(raw: &str) -> Option<[u8; 32]> {
    let raw = raw.trim();
    if raw.len() != 64 {
        return None;
    }
    let mut key = [0_u8; 32];
    for (index, chunk) in raw.as_bytes().chunks(2).enumerate() {
        let text = std::str::from_utf8(chunk).ok()?;
        key[index] = u8::from_str_radix(text, 16).ok()?;
    }
    Some(key)
}

fn legacy_session_keys(data: &TelegramStoreData) -> Vec<[u8; 32]> {
    // Read-only compat for sessions encrypted before TELEGRAM_USER_SESSION_KEY
    // existed (key was API_HASH, then bot_token). Used only to migrate old
    // sessions forward; never used for new encryption.
    let mut sources = Vec::new();
    if let Ok(api_hash) = std::env::var("TELEGRAM_API_HASH")
        && !api_hash.trim().is_empty()
    {
        sources.push(api_hash);
    }
    if !data.bot_token.trim().is_empty() {
        sources.push(data.bot_token.clone());
    }
    sources
        .into_iter()
        .map(|source| {
            let digest = Sha256::digest(source.as_bytes());
            let mut key = [0_u8; 32];
            key.copy_from_slice(&digest);
            key
        })
        .collect()
}

async fn restrict_file_permissions(path: &Path) {
    // The store holds the bot token (plaintext) and encrypted MTProto
    // sessions: keep it owner-only. Best-effort; failures only warn.
    restrict_file_permissions_sync(path);
}

fn restrict_file_permissions_sync(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::Permissions::from_mode(0o600);
        if let Err(error) = std::fs::set_permissions(path, permissions) {
            tracing::warn!(%error, path = %path.display(), "telegram store chmod 0600 failed");
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

fn encrypt_session(session: &str, key: &[u8; 32]) -> Result<String, TelegramStoreError> {
    let iv = rand::random::<[u8; 16]>();
    let encrypted = SessionEncryptor::new_from_slices(key, &iv)
        .map_err(|_| TelegramStoreError::SessionCrypto)?
        .encrypt_padded_vec_mut::<Pkcs7>(session.as_bytes());
    let encoder = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    Ok(format!(
        "v1:{}:{}",
        encoder.encode(iv),
        encoder.encode(encrypted)
    ))
}

fn decrypt_session(value: &str, key: &[u8; 32]) -> Result<String, TelegramStoreError> {
    let mut parts = value.split(':');
    let (Some("v1"), Some(iv), Some(ciphertext), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(TelegramStoreError::SessionCrypto);
    };
    let decoder = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let iv = decoder
        .decode(iv)
        .map_err(|_| TelegramStoreError::SessionCrypto)?;
    let ciphertext = decoder
        .decode(ciphertext)
        .map_err(|_| TelegramStoreError::SessionCrypto)?;
    let decrypted = SessionDecryptor::new_from_slices(key, &iv)
        .map_err(|_| TelegramStoreError::SessionCrypto)?
        .decrypt_padded_vec_mut::<Pkcs7>(&ciphertext)
        .map_err(|_| TelegramStoreError::SessionCrypto)?;
    String::from_utf8(decrypted).map_err(|_| TelegramStoreError::SessionCrypto)
}

fn read_data(path: &Path) -> Result<TelegramStoreData, TelegramStoreError> {
    if !path.exists() {
        return Ok(TelegramStoreData::default());
    }
    let raw = std::fs::read(path).map_err(|_| TelegramStoreError::Read)?;
    if raw.is_empty() {
        return Ok(TelegramStoreData::default());
    }
    serde_json::from_slice(&raw).map_err(|_| TelegramStoreError::Read)
}

#[cfg(test)]
mod tests {
    use super::{load_or_create_key_file, parse_key};

    #[test]
    fn session_key_file_is_auto_created_and_reused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mobile_telegram.key");
        assert!(!path.exists());
        let first = load_or_create_key_file(&path).expect("create");
        assert!(path.exists());
        let second = load_or_create_key_file(&path).expect("reuse");
        assert_eq!(first, second);
        assert!(parse_key("  xyz  ").is_none());
    }
}
