use super::*;
use crate::core::chat::*;

#[derive(Default)]
pub(super) struct RecordingChatStore {
    pub recipients: Mutex<BTreeMap<String, PrincipalRole>>,
    pub messages: Mutex<BTreeMap<(String, String), ChatMessage>>,
    pub fail_recipient: Mutex<Option<String>>,
}

#[async_trait]
impl ChatStorePort for RecordingChatStore {
    async fn ensure_principal(
        &self,
        _principal: ChatPrincipalInput,
    ) -> Result<ChatPrincipal, ChatError> {
        Ok(ChatPrincipal {
            principal_id: _principal.ref_.clone(),
            role: _principal.role,
            ref_: _principal.ref_,
            display_name: _principal.display_name,
            avatar_url: _principal.avatar_url,
        })
    }

    async fn create_or_get_dm(
        &self,
        _actor: &ChatPrincipal,
        target: &ChatPrincipal,
    ) -> Result<ChatConversation, ChatError> {
        self.recipients
            .lock()
            .await
            .insert(target.ref_.clone(), target.role);
        Ok(ChatConversation {
            conversation_id: target.ref_.clone(),
            kind: "dm".into(),
            title: String::new(),
            peer: Some(target.clone()),
            last_message: None,
            last_message_sequence: 0,
            unread_count: 0,
            updated_at_unix: 0,
        })
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
        principal: &Principal,
        conversation_id: &str,
        client_message_id: &str,
        body: &str,
    ) -> Result<ChatSendResult, ChatError> {
        if self.fail_recipient.lock().await.as_deref() == Some(conversation_id) {
            return Err(ChatError::StoreFailed);
        }
        let mut messages = self.messages.lock().await;
        let key = (conversation_id.to_string(), client_message_id.to_string());
        if let Some(message) = messages.get(&key) {
            assert_eq!(message.body, body);
            return Ok(ChatSendResult {
                message: message.clone(),
                created: false,
            });
        }
        let message = ChatMessage {
            message_id: format!("message-{}", messages.len()),
            conversation_id: conversation_id.into(),
            sender_principal_id: principal.ref_.clone(),
            sender_role: principal.role,
            sender_ref: principal.ref_.clone(),
            sender_display_name: principal.display_name.clone(),
            client_message_id: client_message_id.into(),
            sequence: 1,
            message_type: "text".into(),
            body: body.into(),
            metadata: serde_json::json!({}),
            attachment: None,
            created_at_unix: 0,
            edited_at_unix: None,
            deleted_at_unix: None,
        };
        messages.insert(key, message.clone());
        Ok(ChatSendResult {
            message,
            created: true,
        })
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
    ) -> Result<(Vec<ChatRealtimeEvent>, i64, bool), ChatError> {
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
    ) -> Result<Vec<ChatPushDelivery>, ChatError> {
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

    async fn claim_outbox(&self, _limit: usize) -> Result<Vec<ChatOutboxEvent>, ChatError> {
        Err(ChatError::Unavailable)
    }

    async fn mark_outbox_published(&self, _event_id: &str) -> Result<(), ChatError> {
        Err(ChatError::Unavailable)
    }
}
