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

#[derive(Debug)]
pub struct TelegramStore {
    path: PathBuf,
    data: Mutex<TelegramStoreData>,
}

impl TelegramStore {
    pub fn new(path: PathBuf) -> Self {
        let data = read_data(&path).unwrap_or_else(|error| {
            tracing::warn!(%error, path = %path.display(), "telegram store unavailable; using empty state");
            TelegramStoreData::default()
        });
        Self {
            path,
            data: Mutex::new(data),
        }
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

    pub async fn complete_user_profile_login(
        &self,
        telegram_user_id: &str,
        phone_number: String,
        session_string: String,
    ) -> Result<TelegramUserAccount, TelegramStoreError> {
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
        let key = session_key(&data)?;
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
        let data = self.data.lock().await;
        let Some(session) = data.user_sessions.get(telegram_user_id) else {
            return Ok(None);
        };
        if !session.starts_with("v1:") {
            return Ok(Some(session.clone()));
        }
        let key = session_key(&data)?;
        decrypt_session(session, &key).map(Some)
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

    pub async fn create_invite(
        &self,
        token: String,
        role: TelegramAccountRole,
        now_unix: i64,
    ) -> Result<(), TelegramStoreError> {
        let mut data = self.data.lock().await;
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
        tokio::fs::rename(tmp_path, &self.path)
            .await
            .map_err(|_| TelegramStoreError::Write)
    }
}

fn normalize_phone(value: &str) -> String {
    value.chars().filter(char::is_ascii_digit).collect()
}

type SessionEncryptor = cbc::Encryptor<Aes256>;
type SessionDecryptor = cbc::Decryptor<Aes256>;

fn session_key(data: &TelegramStoreData) -> Result<[u8; 32], TelegramStoreError> {
    let source = std::env::var("TELEGRAM_USER_SESSION_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("TELEGRAM_API_HASH")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .or_else(|| (!data.bot_token.trim().is_empty()).then(|| data.bot_token.clone()))
        .ok_or(TelegramStoreError::SessionCrypto)?;
    let digest = Sha256::digest(source.as_bytes());
    let mut key = [0_u8; 32];
    key.copy_from_slice(&digest);
    Ok(key)
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
