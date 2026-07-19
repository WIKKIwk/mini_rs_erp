use crate::core::auth::models::PrincipalRole;
use crate::core::chat::{
    ChatConversation, ChatError, ChatMessage, ChatMessageAttachment, ChatPrincipal,
};

#[derive(sqlx::FromRow)]
pub(super) struct PrincipalRow {
    pub principal_id: String,
    pub principal_role: String,
    pub principal_ref: String,
    pub display_name: String,
    pub avatar_url: String,
}

impl PrincipalRow {
    pub fn into_model(self) -> Result<ChatPrincipal, ChatError> {
        Ok(ChatPrincipal {
            principal_id: self.principal_id,
            role: parse_role(&self.principal_role)?,
            ref_: self.principal_ref,
            display_name: self.display_name,
            avatar_url: self.avatar_url,
        })
    }
}

#[derive(sqlx::FromRow)]
pub(super) struct MessageRow {
    pub message_id: String,
    pub conversation_id: String,
    pub sender_principal_id: String,
    pub sender_role: String,
    pub sender_ref: String,
    pub sender_display_name: String,
    pub client_message_id: String,
    pub message_sequence: i64,
    pub message_type: String,
    pub body: String,
    pub attachment_id: Option<String>,
    pub media_id: Option<String>,
    pub media_kind: Option<String>,
    pub media_content_type: Option<String>,
    pub media_size_bytes: Option<i64>,
    pub media_width_pixels: Option<i32>,
    pub media_height_pixels: Option<i32>,
    pub media_duration_ms: Option<i64>,
    pub created_at_unix: i64,
    pub edited_at_unix: Option<i64>,
    pub deleted_at_unix: Option<i64>,
}

impl MessageRow {
    pub fn into_model(self) -> Result<ChatMessage, ChatError> {
        Ok(ChatMessage {
            message_id: self.message_id,
            conversation_id: self.conversation_id,
            sender_principal_id: self.sender_principal_id,
            sender_role: parse_role(&self.sender_role)?,
            sender_ref: self.sender_ref,
            sender_display_name: self.sender_display_name,
            client_message_id: self.client_message_id,
            sequence: self.message_sequence,
            message_type: self.message_type,
            body: self.body,
            attachment: attachment(AttachmentFields {
                attachment_id: self.attachment_id,
                media_id: self.media_id,
                kind: self.media_kind,
                content_type: self.media_content_type,
                size_bytes: self.media_size_bytes,
                width_pixels: self.media_width_pixels,
                height_pixels: self.media_height_pixels,
                duration_ms: self.media_duration_ms,
            }),
            created_at_unix: self.created_at_unix,
            edited_at_unix: self.edited_at_unix,
            deleted_at_unix: self.deleted_at_unix,
        })
    }
}

#[derive(sqlx::FromRow)]
pub(super) struct ConversationRow {
    pub conversation_id: String,
    pub kind: String,
    pub title: String,
    pub last_message_sequence: i64,
    pub unread_count: i64,
    pub updated_at_unix: i64,
    pub peer_principal_id: Option<String>,
    pub peer_role: Option<String>,
    pub peer_ref: Option<String>,
    pub peer_display_name: Option<String>,
    pub peer_avatar_url: Option<String>,
    pub message_id: Option<String>,
    pub sender_principal_id: Option<String>,
    pub sender_role: Option<String>,
    pub sender_ref: Option<String>,
    pub sender_display_name: Option<String>,
    pub client_message_id: Option<String>,
    pub message_sequence: Option<i64>,
    pub message_type: Option<String>,
    pub body: Option<String>,
    pub attachment_id: Option<String>,
    pub media_id: Option<String>,
    pub media_kind: Option<String>,
    pub media_content_type: Option<String>,
    pub media_size_bytes: Option<i64>,
    pub media_width_pixels: Option<i32>,
    pub media_height_pixels: Option<i32>,
    pub media_duration_ms: Option<i64>,
    pub message_created_at_unix: Option<i64>,
    pub edited_at_unix: Option<i64>,
    pub deleted_at_unix: Option<i64>,
}

impl ConversationRow {
    pub fn into_model(self) -> Result<ChatConversation, ChatError> {
        let peer = match (
            self.peer_principal_id,
            self.peer_role,
            self.peer_ref,
            self.peer_display_name,
        ) {
            (Some(principal_id), Some(role), Some(ref_), Some(display_name)) => {
                Some(ChatPrincipal {
                    principal_id,
                    role: parse_role(&role)?,
                    ref_,
                    display_name,
                    avatar_url: self.peer_avatar_url.unwrap_or_default(),
                })
            }
            _ => None,
        };
        let last_message = match (
            self.message_id,
            self.sender_principal_id,
            self.sender_role,
            self.sender_ref,
            self.sender_display_name,
            self.client_message_id,
            self.message_sequence,
            self.message_type,
            self.body,
            self.message_created_at_unix,
        ) {
            (
                Some(message_id),
                Some(sender_principal_id),
                Some(sender_role),
                Some(sender_ref),
                Some(sender_display_name),
                Some(client_message_id),
                Some(sequence),
                Some(message_type),
                Some(body),
                Some(created_at_unix),
            ) => Some(ChatMessage {
                message_id,
                conversation_id: self.conversation_id.clone(),
                sender_principal_id,
                sender_role: parse_role(&sender_role)?,
                sender_ref,
                sender_display_name,
                client_message_id,
                sequence,
                message_type,
                body,
                attachment: attachment(AttachmentFields {
                    attachment_id: self.attachment_id,
                    media_id: self.media_id,
                    kind: self.media_kind,
                    content_type: self.media_content_type,
                    size_bytes: self.media_size_bytes,
                    width_pixels: self.media_width_pixels,
                    height_pixels: self.media_height_pixels,
                    duration_ms: self.media_duration_ms,
                }),
                created_at_unix,
                edited_at_unix: self.edited_at_unix,
                deleted_at_unix: self.deleted_at_unix,
            }),
            _ => None,
        };
        Ok(ChatConversation {
            conversation_id: self.conversation_id,
            kind: self.kind,
            title: self.title,
            peer,
            last_message,
            last_message_sequence: self.last_message_sequence,
            unread_count: self.unread_count,
            updated_at_unix: self.updated_at_unix,
        })
    }
}

struct AttachmentFields {
    attachment_id: Option<String>,
    media_id: Option<String>,
    kind: Option<String>,
    content_type: Option<String>,
    size_bytes: Option<i64>,
    width_pixels: Option<i32>,
    height_pixels: Option<i32>,
    duration_ms: Option<i64>,
}

fn attachment(fields: AttachmentFields) -> Option<ChatMessageAttachment> {
    let (
        Some(attachment_id),
        Some(media_id),
        Some(kind),
        Some(content_type),
        Some(size_bytes),
        Some(width_pixels),
        Some(height_pixels),
    ) = (
        fields.attachment_id,
        fields.media_id,
        fields.kind,
        fields.content_type,
        fields.size_bytes,
        fields.width_pixels,
        fields.height_pixels,
    )
    else {
        return None;
    };
    Some(ChatMessageAttachment {
        attachment_id,
        content_url: format!("/v1/mobile/chat/media/{media_id}/content"),
        thumbnail_url: format!("/v1/mobile/chat/media/{media_id}/thumbnail"),
        media_id,
        kind,
        content_type,
        size_bytes,
        width_pixels,
        height_pixels,
        duration_ms: fields.duration_ms,
    })
}

pub(super) fn role_key(role: &PrincipalRole) -> &'static str {
    match role {
        PrincipalRole::Supplier => "supplier",
        PrincipalRole::Werka => "werka",
        PrincipalRole::Customer => "customer",
        PrincipalRole::Aparatchi => "aparatchi",
        PrincipalRole::Qolipchi => "qolipchi",
        PrincipalRole::Boyoqchi => "boyoqchi",
        PrincipalRole::MaterialTaminotchi => "material_taminotchi",
        PrincipalRole::Admin => "admin",
    }
}

pub(super) fn parse_role(value: &str) -> Result<PrincipalRole, ChatError> {
    match value.trim() {
        "supplier" => Ok(PrincipalRole::Supplier),
        "werka" => Ok(PrincipalRole::Werka),
        "customer" => Ok(PrincipalRole::Customer),
        "aparatchi" => Ok(PrincipalRole::Aparatchi),
        "qolipchi" => Ok(PrincipalRole::Qolipchi),
        "boyoqchi" => Ok(PrincipalRole::Boyoqchi),
        "material_taminotchi" => Ok(PrincipalRole::MaterialTaminotchi),
        "admin" => Ok(PrincipalRole::Admin),
        _ => Err(ChatError::StoreFailed),
    }
}
