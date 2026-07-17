use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore;
use tokio::sync::{Mutex, RwLock, broadcast};

use super::ChatRealtimeEvent;
use crate::core::auth::models::Principal;
use crate::core::profile::identity::ProfileIdentity;

const CHAT_EVENT_CAPACITY: usize = 512;
const SOCKET_TICKET_TTL: Duration = Duration::from_secs(30);

#[derive(Clone, Default)]
pub struct ChatHub {
    channels: Arc<RwLock<HashMap<String, broadcast::Sender<ChatRealtimeEvent>>>>,
    tickets: Arc<Mutex<HashMap<String, SocketTicket>>>,
}

struct SocketTicket {
    principal: Principal,
    expires_at: Instant,
}

impl ChatHub {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn subscribe(&self, principal: &Principal) -> broadcast::Receiver<ChatRealtimeEvent> {
        let key = principal_key(principal);
        let mut channels = self.channels.write().await;
        channels
            .entry(key)
            .or_insert_with(|| broadcast::channel(CHAT_EVENT_CAPACITY).0)
            .subscribe()
    }

    pub async fn publish(&self, event: ChatRealtimeEvent, recipient_keys: &[String]) {
        let channels = self.channels.read().await;
        for key in recipient_keys {
            if let Some(sender) = channels.get(key) {
                let _ = sender.send(event.clone());
            }
        }
    }

    pub async fn issue_ticket(&self, principal: Principal) -> String {
        let mut bytes = [0_u8; 24];
        rand::rng().fill_bytes(&mut bytes);
        let ticket = URL_SAFE_NO_PAD.encode(bytes);
        let mut tickets = self.tickets.lock().await;
        let now = Instant::now();
        tickets.retain(|_, value| value.expires_at > now);
        tickets.insert(
            ticket.clone(),
            SocketTicket {
                principal,
                expires_at: now + SOCKET_TICKET_TTL,
            },
        );
        ticket
    }

    pub async fn consume_ticket(&self, ticket: &str) -> Option<Principal> {
        let value = self.tickets.lock().await.remove(ticket.trim())?;
        (value.expires_at > Instant::now()).then_some(value.principal)
    }
}

pub fn principal_key(principal: &Principal) -> String {
    ProfileIdentity::from_principal(&principal.role, &principal.ref_)
        .map(|identity| identity.vault_key())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::auth::models::PrincipalRole;

    fn principal(ref_: &str) -> Principal {
        Principal {
            role: PrincipalRole::Aparatchi,
            display_name: ref_.to_string(),
            legal_name: ref_.to_string(),
            ref_: ref_.to_string(),
            phone: String::new(),
            avatar_url: String::new(),
        }
    }

    #[tokio::test]
    async fn socket_ticket_is_bound_and_single_use() {
        let hub = ChatHub::new();
        let expected = principal("worker-1");
        let ticket = hub.issue_ticket(expected.clone()).await;

        assert_eq!(hub.consume_ticket(&ticket).await, Some(expected));
        assert_eq!(hub.consume_ticket(&ticket).await, None);
    }

    #[tokio::test]
    async fn publish_only_reaches_selected_principal() {
        let hub = ChatHub::new();
        let first = principal("worker-1");
        let second = principal("worker-2");
        let mut first_events = hub.subscribe(&first).await;
        let mut second_events = hub.subscribe(&second).await;
        let event = ChatRealtimeEvent {
            event_id: "event-1".to_string(),
            event: "chat.message.created".to_string(),
            conversation_id: "conversation-1".to_string(),
            sequence: 1,
            message: crate::core::chat::ChatMessage {
                message_id: "message-1".to_string(),
                conversation_id: "conversation-1".to_string(),
                sender_principal_id: "principal-1".to_string(),
                sender_role: PrincipalRole::Aparatchi,
                sender_ref: "worker-1".to_string(),
                sender_display_name: "Worker 1".to_string(),
                client_message_id: "client-1".to_string(),
                sequence: 1,
                message_type: "text".to_string(),
                body: "Salom".to_string(),
                attachment: None,
                created_at_unix: 1,
                edited_at_unix: None,
                deleted_at_unix: None,
            },
        };

        hub.publish(event.clone(), &[principal_key(&first)]).await;

        assert_eq!(first_events.try_recv(), Ok(event));
        assert!(matches!(
            second_events.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    }
}
