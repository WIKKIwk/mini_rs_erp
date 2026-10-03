use super::{TelegramStore, TelegramStoreData, TelegramStoreError};
use crate::telegram::alerts::models::{AlertJob, AlertKind, AlertMember, TelegramAlertSettings};
use crate::telegram::models::TelegramUserGroup;

pub(super) fn activate_alert_sender(
    data: &mut TelegramStoreData,
    user: &crate::telegram::TelegramUserAccount,
) {
    if user.role == crate::telegram::TelegramAccountRole::AlertSender
        && user.user_profile_connected
        && data.alert_settings.sender_user_id.as_deref() != Some(&user.telegram_user_id)
    {
        data.alert_settings = TelegramAlertSettings {
            sender_user_id: Some(user.telegram_user_id.clone()),
            ..Default::default()
        };
    }
}

impl TelegramStore {
    pub(crate) async fn alert_settings(&self) -> TelegramAlertSettings {
        self.data.lock().await.alert_settings.clone()
    }

    pub(crate) async fn set_alert_sender(
        &self,
        sender: Option<String>,
    ) -> Result<(), TelegramStoreError> {
        let mut data = self.data.lock().await;
        let mut updated = data.clone();
        if updated.alert_settings.sender_user_id != sender {
            updated.alert_settings = TelegramAlertSettings {
                sender_user_id: sender,
                ..Default::default()
            };
        }
        self.persist(&updated).await?;
        *data = updated;
        Ok(())
    }

    pub(crate) async fn set_alert_group(
        &self,
        sender: &str,
        group: TelegramUserGroup,
    ) -> Result<(), TelegramStoreError> {
        let mut data = self.data.lock().await;
        if data.alert_settings.sender_user_id.as_deref() != Some(sender) {
            return Err(TelegramStoreError::UserNotFound);
        }
        let mut updated = data.clone();
        if updated.alert_settings.group.as_ref() != Some(&group) {
            updated.alert_settings.raw_material_members.clear();
            updated.alert_settings.qolip_members.clear();
        }
        updated.alert_settings.group = Some(group);
        self.persist(&updated).await?;
        *data = updated;
        Ok(())
    }

    pub(crate) async fn toggle_alert_member(
        &self,
        expected: &TelegramAlertSettings,
        kind: AlertKind,
        member: AlertMember,
    ) -> Result<(), TelegramStoreError> {
        let mut data = self.data.lock().await;
        if data.alert_settings.sender_user_id != expected.sender_user_id
            || data.alert_settings.group != expected.group
        {
            return Err(TelegramStoreError::UserNotFound);
        }
        let mut updated = data.clone();
        let members = updated.alert_settings.members_mut(kind);
        if let Some(index) = members.iter().position(|m| m.user_id == member.user_id) {
            members.remove(index);
        } else {
            if members.len() >= 30 {
                return Err(TelegramStoreError::Write);
            }
            members.push(member);
        }
        self.persist(&updated).await?;
        *data = updated;
        Ok(())
    }

    pub(crate) async fn enqueue_alert(&self, job: AlertJob) -> Result<(), TelegramStoreError> {
        let mut data = self.data.lock().await;
        if data.alert_jobs.contains_key(&job.id) {
            return Ok(());
        }
        let mut updated = data.clone();
        updated.alert_jobs.insert(job.id.clone(), job);
        self.persist(&updated).await?;
        *data = updated;
        Ok(())
    }

    pub(crate) async fn due_alert(&self, now: i64) -> Option<AlertJob> {
        self.data
            .lock()
            .await
            .alert_jobs
            .values()
            .filter(|job| !job.delivered && job.next_attempt_at <= now)
            .min_by_key(|job| job.next_attempt_at)
            .cloned()
    }

    pub(crate) async fn finish_alert(
        &self,
        id: &str,
        error: Option<String>,
        retry_at: i64,
    ) -> Result<(), TelegramStoreError> {
        let mut data = self.data.lock().await;
        let mut updated = data.clone();
        if let Some(job) = updated.alert_jobs.get_mut(id) {
            job.attempts = job.attempts.saturating_add(1);
            job.delivered = error.is_none();
            job.last_error = error;
            job.next_attempt_at = retry_at;
        }
        self.persist(&updated).await?;
        *data = updated;
        Ok(())
    }
}
