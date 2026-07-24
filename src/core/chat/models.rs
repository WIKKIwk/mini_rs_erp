use serde::{Deserialize, Serialize};

use crate::core::auth::models::PrincipalRole;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatPrincipal {
    pub principal_id: String,
    pub role: PrincipalRole,
    #[serde(rename = "ref")]
    pub ref_: String,
    pub display_name: String,
    #[serde(default)]
    pub avatar_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatPrincipalInput {
    pub role: PrincipalRole,
    pub ref_: String,
    pub display_name: String,
    pub avatar_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatDirectoryEntry {
    pub role: PrincipalRole,
    #[serde(rename = "ref")]
    pub ref_: String,
    pub display_name: String,
    pub avatar_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatDirectoryPage {
    pub items: Vec<ChatDirectoryEntry>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub message_id: String,
    pub conversation_id: String,
    pub sender_principal_id: String,
    pub sender_role: PrincipalRole,
    #[serde(rename = "sender_ref")]
    pub sender_ref: String,
    pub sender_display_name: String,
    pub client_message_id: String,
    pub sequence: i64,
    #[serde(rename = "type")]
    pub message_type: String,
    pub body: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment: Option<ChatMessageAttachment>,
    pub created_at_unix: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_at_unix: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at_unix: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderFreezeChatEvent {
    pub event_sequence: i64,
    pub event_id: String,
    pub request_id: String,
    pub status: String,
    pub order_id: String,
    pub order_number: String,
    pub order_title: String,
    pub requester_role: String,
    pub requester_ref: String,
    pub requester_display_name: String,
    pub target_session_id: String,
    pub target_apparatus: String,
    pub target_worker_role: String,
    pub target_worker_ref: String,
    pub target_worker_display_name: String,
    pub requested_at_unix: i64,
    pub transitioned_at_unix: i64,
    pub attempts: i32,
}

impl OrderFreezeChatEvent {
    pub fn message_body(&self) -> String {
        format!(
            "{} buyurtmasini muzlatish so‘rovi",
            if self.order_number.trim().is_empty() {
                self.order_id.trim()
            } else {
                self.order_number.trim()
            }
        )
    }

    pub fn metadata(&self) -> serde_json::Value {
        serde_json::json!({
            "kind": "order_freeze_request",
            "event_sequence": self.event_sequence,
            "request_id": self.request_id,
            "status": self.status,
            "order_id": self.order_id,
            "order_number": self.order_number,
            "order_title": self.order_title,
            "requester_role": self.requester_role,
            "requester_ref": self.requester_ref,
            "requester_display_name": self.requester_display_name,
            "target_session_id": self.target_session_id,
            "target_apparatus": self.target_apparatus,
            "target_worker_role": self.target_worker_role,
            "target_worker_ref": self.target_worker_ref,
            "target_worker_display_name": self.target_worker_display_name,
            "requested_at_unix": self.requested_at_unix,
            "transitioned_at_unix": self.transitioned_at_unix,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessageAttachment {
    pub attachment_id: String,
    pub media_id: String,
    pub kind: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub width_pixels: i32,
    pub height_pixels: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    pub content_url: String,
    pub thumbnail_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatConversation {
    pub conversation_id: String,
    pub kind: String,
    pub title: String,
    pub peer: Option<ChatPrincipal>,
    pub last_message: Option<ChatMessage>,
    pub last_message_sequence: i64,
    pub unread_count: i64,
    pub updated_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatConversationPage {
    pub items: Vec<ChatConversation>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatMessagePage {
    pub items: Vec<ChatMessage>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatSendResult {
    pub message: ChatMessage,
    pub created: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatRealtimeEvent {
    pub event_id: String,
    #[serde(default)]
    pub cursor: i64,
    pub event: String,
    pub conversation_id: String,
    pub sequence: i64,
    pub message: ChatMessage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatOutboxEvent {
    pub event_id: String,
    pub cursor: i64,
    pub recipient_keys: Vec<String>,
    pub payload: ChatRealtimeEvent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatSyncPage {
    pub events: Vec<ChatRealtimeEvent>,
    pub next_cursor: i64,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatPushDelivery {
    pub event_id: String,
    pub recipient_key: String,
    pub attempts: i32,
    pub payload: ChatRealtimeEvent,
}
