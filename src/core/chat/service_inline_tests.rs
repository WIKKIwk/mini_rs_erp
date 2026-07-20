#[cfg(test)]
mod tests {
    use super::{
        ChatError, ChatPrincipalInput, ChatService, message_preview_text,
        push_retry_delay_seconds,
    };
    use crate::core::auth::models::{Principal, PrincipalRole};
    use crate::core::chat::can_participate_in_chat;

    #[test]
    fn media_notification_preview_uses_caption_or_kind_fallback() {
        assert_eq!(message_preview_text("image", ""), "Rasm");
        assert_eq!(message_preview_text("video", "  "), "Video");
        assert_eq!(message_preview_text("audio", "  "), "Ovozli xabar");
        assert_eq!(
            message_preview_text("image", "Mahsulot rasmi"),
            "Mahsulot rasmi"
        );
        assert_eq!(message_preview_text("text", "Salom"), "Salom");
    }

    #[test]
    fn push_retry_delay_is_exponential_and_capped() {
        assert_eq!(push_retry_delay_seconds(1), 2);
        assert_eq!(push_retry_delay_seconds(4), 16);
        assert_eq!(push_retry_delay_seconds(20), 900);
    }

    #[test]
    fn chat_policy_excludes_customers_and_keeps_other_roles_available() {
        assert!(!can_participate_in_chat(&PrincipalRole::Customer));
        for role in [
            PrincipalRole::Supplier,
            PrincipalRole::Werka,
            PrincipalRole::Aparatchi,
            PrincipalRole::Qolipchi,
            PrincipalRole::Boyoqchi,
            PrincipalRole::MaterialTaminotchi,
            PrincipalRole::Admin,
        ] {
            assert!(can_participate_in_chat(&role));
        }
    }

    #[tokio::test]
    async fn customer_cannot_create_or_list_chat() {
        let service = ChatService::unavailable();
        let customer = Principal {
            role: PrincipalRole::Customer,
            display_name: "Customer".to_string(),
            legal_name: "Customer".to_string(),
            ref_: "customer-1".to_string(),
            phone: String::new(),
            avatar_url: String::new(),
        };
        assert_eq!(
            service.conversations(&customer, 30, 0).await,
            Err(ChatError::Forbidden)
        );
        assert_eq!(
            service
                .create_or_get_dm(
                    ChatPrincipalInput {
                        role: PrincipalRole::Aparatchi,
                        ref_: "worker-1".to_string(),
                        display_name: "Worker".to_string(),
                        avatar_url: String::new(),
                    },
                    ChatPrincipalInput {
                        role: PrincipalRole::Customer,
                        ref_: "customer-1".to_string(),
                        display_name: "Customer".to_string(),
                        avatar_url: String::new(),
                    },
                )
                .await,
            Err(ChatError::Forbidden)
        );
    }
}

struct UnavailableChatStore;

#[async_trait::async_trait]
impl ChatStorePort for UnavailableChatStore {
    async fn ensure_principal(
        &self,
        _principal: ChatPrincipalInput,
    ) -> Result<ChatPrincipal, ChatError> {
        Err(ChatError::Unavailable)
    }

    async fn create_or_get_dm(
        &self,
        _actor: &ChatPrincipal,
        _target: &ChatPrincipal,
    ) -> Result<ChatConversation, ChatError> {
        Err(ChatError::Unavailable)
    }

    async fn conversations(
        &self,
        _principal: &Principal,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<ChatConversation>, ChatError> {
        Err(ChatError::Unavailable)
    }

    async fn messages(
        &self,
        _principal: &Principal,
        _conversation_id: &str,
        _before_sequence: Option<i64>,
        _after_sequence: Option<i64>,
        _limit: usize,
    ) -> Result<ChatMessagePage, ChatError> {
        Err(ChatError::Unavailable)
    }

    async fn send_message(
        &self,
        _principal: &Principal,
        _conversation_id: &str,
        _client_message_id: &str,
        _body: &str,
    ) -> Result<ChatSendResult, ChatError> {
        Err(ChatError::Unavailable)
    }

    async fn send_media_message(
        &self,
        _principal: &Principal,
        _conversation_id: &str,
        _client_message_id: &str,
        _caption: &str,
        _media_id: &str,
    ) -> Result<ChatSendResult, ChatError> {
        Err(ChatError::Unavailable)
    }

    async fn mark_read(
        &self,
        _principal: &Principal,
        _conversation_id: &str,
        _sequence: i64,
        _device_id: &str,
    ) -> Result<(), ChatError> {
        Err(ChatError::Unavailable)
    }

    async fn mark_delivered(
        &self,
        _principal: &Principal,
        _conversation_id: &str,
        _sequence: i64,
        _device_id: &str,
    ) -> Result<(), ChatError> {
        Err(ChatError::Unavailable)
    }

    async fn sync_events(
        &self,
        _principal: &Principal,
        _after_cursor: i64,
        _limit: usize,
    ) -> Result<(Vec<super::ChatRealtimeEvent>, i64, bool), ChatError> {
        Err(ChatError::Unavailable)
    }

    async fn issue_socket_ticket(
        &self,
        _principal: &Principal,
        _ticket: &str,
        _expires_at_unix: i64,
    ) -> Result<(), ChatError> {
        Err(ChatError::Unavailable)
    }

    async fn consume_socket_ticket(&self, _ticket: &str) -> Result<Principal, ChatError> {
        Err(ChatError::Unavailable)
    }

    async fn claim_push_deliveries(
        &self,
        _limit: usize,
    ) -> Result<Vec<super::ChatPushDelivery>, ChatError> {
        Err(ChatError::Unavailable)
    }

    async fn mark_push_delivered(
        &self,
        _event_id: &str,
        _recipient_key: &str,
    ) -> Result<(), ChatError> {
        Err(ChatError::Unavailable)
    }

    async fn reschedule_push_delivery(
        &self,
        _event_id: &str,
        _recipient_key: &str,
        _retry_after_seconds: i64,
        _dead_letter: bool,
        _error: &str,
    ) -> Result<(), ChatError> {
        Err(ChatError::Unavailable)
    }

    async fn claim_outbox(&self, _limit: usize) -> Result<Vec<super::ChatOutboxEvent>, ChatError> {
        Err(ChatError::Unavailable)
    }

    async fn mark_outbox_published(&self, _event_id: &str) -> Result<(), ChatError> {
        Err(ChatError::Unavailable)
    }
}
