
fn map_user_account(error: UserAccountError) -> TelegramError {
    match error {
        UserAccountError::NotConfigured => TelegramError::UserAccountNotConfigured,
        UserAccountError::NotAuthorized | UserAccountError::LoginNotPending => {
            TelegramError::UserAccountNotAuthorized
        }
        UserAccountError::InvalidCode => TelegramError::UserAccountInvalidCode,
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
