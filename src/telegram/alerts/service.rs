use super::models::{AlertJob, AlertKind, AlertSenderUpdate};
use crate::telegram::useraccount::UserAccountError;
use crate::telegram::{TelegramAccountRole, TelegramError, TelegramService};
use sha2::{Digest, Sha256};
use std::sync::atomic::Ordering;
use std::time::Duration;
use time::OffsetDateTime;

impl TelegramService {
    pub async fn update_alert_sender(
        &self,
        input: AlertSenderUpdate,
    ) -> Result<crate::telegram::TelegramAdminOverview, TelegramError> {
        let sender = input
            .sender_user_id
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if let Some(id) = sender.as_deref() {
            let account = self
                .user_by_telegram_id(id)
                .await?
                .ok_or(TelegramError::UserAccountNotAuthorized)?;
            if !account.user_profile_connected
                || self
                    .store
                    .user_session(id)
                    .await
                    .map_err(|_| TelegramError::Store)?
                    .is_none()
            {
                return Err(TelegramError::UserAccountNotAuthorized);
            }
        }
        self.store
            .set_alert_sender(sender)
            .await
            .map_err(|_| TelegramError::Store)?;
        self.admin_overview().await
    }

    pub(crate) async fn authorize_alert_configuration(
        &self,
        actor: &str,
    ) -> Result<(), TelegramError> {
        let account = self
            .user_by_telegram_id(actor)
            .await?
            .ok_or(TelegramError::UserAccountNotAuthorized)?;
        let settings = self.store.alert_settings().await;
        if account.role != TelegramAccountRole::Admin
            && settings.sender_user_id.as_deref() != Some(actor)
        {
            return Err(TelegramError::UserAccount(
                "Ogohlantirishni faqat admin yoki tanlangan ogohlantiruvchi sozlaydi".into(),
            ));
        }
        Ok(())
    }

    pub async fn enqueue_order_alert(
        &self,
        id: String,
        kind: AlertKind,
        message: String,
    ) -> Result<bool, TelegramError> {
        let settings = self.store.alert_settings().await;
        let (Some(sender), Some(group)) = (&settings.sender_user_id, &settings.group) else {
            return Ok(false);
        };
        let members = settings.members(kind).to_vec();
        if members.is_empty() {
            return Ok(false);
        }
        let key = format!("{sender}:{}:{}:{id}", group.chat_type, group.chat_id);
        let hash = Sha256::digest(key.as_bytes());
        let random_id =
            (i64::from_le_bytes(hash[..8].try_into().expect("SHA-256 prefix")) & i64::MAX).max(1);
        self.store
            .enqueue_alert(AlertJob {
                id,
                sender_user_id: sender.clone(),
                group: group.clone(),
                message,
                members,
                random_id,
                attempts: 0,
                next_attempt_at: OffsetDateTime::now_utc().unix_timestamp(),
                delivered: false,
                last_error: None,
            })
            .await
            .map_err(|_| TelegramError::Store)?;
        self.start_alert_worker();
        Ok(true)
    }

    pub(crate) fn start_alert_worker(&self) {
        if cfg!(test) || self.alert_worker_started.swap(true, Ordering::AcqRel) {
            return;
        }
        let service = self.clone();
        tokio::spawn(async move {
            loop {
                let now = OffsetDateTime::now_utc().unix_timestamp();
                let Some(job) = service.store.due_alert(now).await else {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    continue;
                };
                let result = tokio::time::timeout(
                    Duration::from_secs(45),
                    service.useraccount.send_alert(&job),
                )
                .await;
                let (error, delay) = match result {
                    Ok(Ok(())) => (None, 0),
                    Ok(Err(error)) => {
                        let delay = match &error {
                            UserAccountError::FloodWait { seconds } => (*seconds).max(5),
                            _ => (5_u64 * 2_u64.pow(job.attempts.min(9))).min(3600),
                        };
                        tracing::warn!(job_id = %job.id, %error, delay, "Telegram alert will retry");
                        (Some(error.to_string()), delay)
                    }
                    Err(_) => (Some("Telegram alert timeout".into()), 30),
                };
                let retry_at = OffsetDateTime::now_utc()
                    .unix_timestamp()
                    .saturating_add(delay.min(i64::MAX as u64) as i64);
                if let Err(error) = service.store.finish_alert(&job.id, error, retry_at).await {
                    tracing::warn!(%error, "Telegram alert delivery status was not persisted");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        });
    }
}
