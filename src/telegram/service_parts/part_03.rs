
fn map_user_account(error: UserAccountError) -> TelegramError {
    match error {
        UserAccountError::NotConfigured => TelegramError::UserAccountNotConfigured,
        UserAccountError::NotAuthorized | UserAccountError::LoginNotPending => {
            TelegramError::UserAccountNotAuthorized
        }
        UserAccountError::InvalidCode => TelegramError::UserAccountInvalidCode,
        UserAccountError::FloodWait { seconds } => {
            TelegramError::UserAccountFloodWait { seconds }
        }
        UserAccountError::SendCodeUnavailable => {
            TelegramError::UserAccountSendCodeUnavailable
        }
        UserAccountError::ResendTooSoon { wait_seconds } => {
            TelegramError::UserAccountResendTooSoon { wait_seconds }
        }
        UserAccountError::SignUpRequired => TelegramError::UserAccountSignUpRequired,
        UserAccountError::AccountMismatch => TelegramError::UserAccountAccountMismatch,
        UserAccountError::GroupNotWritable => TelegramError::UserAccountGroupNotWritable,
        UserAccountError::Transport(error) | UserAccountError::Store(error) => {
            TelegramError::UserAccount(error)
        }
    }
}

fn map_store(error: TelegramStoreError) -> TelegramError {
    match error {
        TelegramStoreError::InviteNotFound => TelegramError::InviteNotFound,
        TelegramStoreError::InviteAlreadyUsed => TelegramError::InviteAlreadyUsed,
        TelegramStoreError::InviteExpired => TelegramError::InviteExpired,
        TelegramStoreError::UserNotFound => TelegramError::UserAccountNotAuthorized,
        TelegramStoreError::Read
        | TelegramStoreError::Write
        | TelegramStoreError::SessionCrypto => TelegramError::Store,
    }
}

fn normalize_bot_username(value: &str) -> String {
    let mut value = value.trim().trim_start_matches('@').trim_end_matches('/');
    for prefix in ["https://t.me/", "http://t.me/", "t.me/"] {
        if let Some(rest) = value.strip_prefix(prefix) {
            value = rest.trim_start_matches('@').trim_end_matches('/');
            break;
        }
    }
    value.to_string()
}

fn token_hint(token: &str) -> String {
    let token = token.trim();
    if token.is_empty() {
        return String::new();
    }
    if token.len() <= 8 {
        return "••••••••".to_string();
    }
    format!("••••{}", &token[token.len() - 4..])
}

fn create_invite_token() -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(rand::random::<[u8; 18]>())
}

#[cfg(test)]
mod tests {
    use super::{TelegramService, normalize_bot_username};
    use crate::telegram::{
        TelegramAccountRole, TelegramBotSettingsUpdate, TelegramInviteRequest, TelegramStartRequest,
    };

    #[test]
    fn bot_username_is_normalized_for_deep_links() {
        assert_eq!(
            normalize_bot_username("https://t.me/@accord_bot/"),
            "accord_bot"
        );
    }

    #[tokio::test]
    async fn telegram_qr_new_user_session_and_bot_start_preserve_identity_and_role() {
        use crate::telegram::{TelegramDeliveryMode, TelegramError, TelegramUserAccount};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("telegram.json");
        let service = TelegramService::new(path.clone());
        let account = TelegramUserAccount {
            telegram_user_id: "123".into(),
            telegram_chat_id: String::new(),
            username: "new_user".into(),
            display_name: "New User".into(),
            role: TelegramAccountRole::SalesManager,
            invite_token: String::new(),
            joined_at_unix: 1,
            phone_number: "998123456789".into(),
            delivery_mode: TelegramDeliveryMode::UserProfile,
            user_profile_connected: true,
            selected_chat_id: None,
            selected_chat_title: None,
            selected_chat_type: None,
        };
        service
            .store
            .register_qr_user(account.clone(), "qr-session-secret".into())
            .await
            .unwrap()
            .unwrap();
        let mut duplicate = account.clone();
        duplicate.role = TelegramAccountRole::Admin;
        assert!(
            service
                .store
                .register_qr_user(duplicate, "replacement-session".into())
                .await
                .unwrap()
                .is_none()
        );
        let start = || TelegramStartRequest {
            invite_token: String::new(),
            telegram_user_id: "123".into(),
            telegram_chat_id: "123".into(),
            username: "new_user".into(),
            display_name: "New User".into(),
        };
        // The unauthenticated HTTP start route cannot hijack a QR-linked user's chat.
        assert!(matches!(
            service.register_start(start()).await,
            Err(TelegramError::InviteTokenRequired)
        ));
        let recognized = service.register_bot_start(start()).await.unwrap();
        assert_eq!(recognized.role, TelegramAccountRole::SalesManager);
        assert_eq!(recognized.telegram_chat_id, "123");
        assert!(recognized.user_profile_connected);
        let mut unknown = start();
        unknown.telegram_user_id = "999".into();
        assert!(matches!(
            service.register_bot_start(unknown).await,
            Err(TelegramError::InviteTokenRequired)
        ));
        let persisted = std::fs::read_to_string(&path).unwrap();
        assert!(!persisted.contains("qr-session-secret"));
        assert!(!persisted.contains("replacement-session"));
        let reloaded = TelegramService::new(path);
        assert_eq!(
            reloaded.store.user_session("123").await.unwrap().as_deref(),
            Some("qr-session-secret")
        );
        assert_eq!(
            reloaded.admin_overview().await.unwrap().users,
            vec![recognized]
        );
        let deleted = reloaded.delete_user_account("123").await.unwrap();
        assert_eq!(deleted.telegram_user_id, "123");
        assert!(
            reloaded
                .store
                .user_by_telegram_id("123")
                .await
                .unwrap()
                .is_none()
        );
        assert!(reloaded.store.user_session("123").await.unwrap().is_none());
        assert!(reloaded.admin_overview().await.unwrap().users.is_empty());
    }

    #[tokio::test]
    async fn invite_can_be_claimed_once_and_returns_role() {
        let dir = tempfile::tempdir().expect("tempdir");
        let service = TelegramService::new(dir.path().join("telegram.json"));
        service
            .update_bot_settings(TelegramBotSettingsUpdate {
                bot_username: "accord_bot".to_string(),
                bot_token: "token".to_string(),
            })
            .await
            .expect("settings");
        let invite = service
            .create_invite(TelegramInviteRequest {
                role: TelegramAccountRole::SalesManager,
            })
            .await
            .expect("invite");
        let token = invite
            .invite_url
            .split("start=")
            .nth(1)
            .expect("invite token")
            .to_string();
        let user = service
            .register_start(TelegramStartRequest {
                invite_token: token,
                telegram_user_id: "123".to_string(),
                telegram_chat_id: "456".to_string(),
                username: "manager".to_string(),
                display_name: "Sales Manager".to_string(),
            })
            .await
            .expect("start");
        assert_eq!(user.role, TelegramAccountRole::SalesManager);
        assert_eq!(
            service
                .admin_overview()
                .await
                .expect("overview")
                .users
                .len(),
            1
        );
    }
}
