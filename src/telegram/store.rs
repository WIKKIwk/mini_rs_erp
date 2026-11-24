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

#[path = "store_alerts.rs"]
mod alerts;

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
    alert_settings: super::alerts::TelegramAlertSettings,
    #[serde(default)]
    alert_jobs: BTreeMap<String, super::alerts::models::AlertJob>,
    #[serde(default)]
    bot_username: String,
    #[serde(default)]
    bot_token: String,
    #[serde(default)]
    user_api_id: Option<i32>,
    #[serde(default)]
    user_api_hash: Option<String>,
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

include!("store_parts/persistent_state.rs");

include!("store_parts/derive_key.rs");

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
