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
    pub created_at_unix: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_at_unix: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at_unix: Option<i64>,
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
    pub event: String,
    pub conversation_id: String,
    pub sequence: i64,
    pub message: ChatMessage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatOutboxEvent {
    pub event_id: String,
    pub recipient_keys: Vec<String>,
    pub payload: ChatRealtimeEvent,
}
