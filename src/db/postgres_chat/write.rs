use sqlx::{PgPool, Postgres, Transaction};

use sha2::{Digest, Sha256};

use super::rows::{MessageRow, PrincipalRow, parse_role, role_key};
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::chat::{
    ChatConversation, ChatError, ChatMessage, ChatMessageAttachment, ChatOutboxEvent,
    ChatPrincipal, ChatPrincipalInput, ChatPushDelivery, ChatRealtimeEvent, ChatSendResult,
    InventoryTransferChatEvent, OrderFreezeChatEvent,
};

include!("write_sql.rs");

async fn lock_conversation_sequence(
    tx: &mut Transaction<'_, Postgres>,
    conversation_id: &str,
) -> Result<i64, ChatError> {
    sqlx::query_scalar(
        "SELECT last_message_sequence FROM mini_chat_conversations WHERE conversation_id = $1 FOR UPDATE",
    )
    .bind(conversation_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ChatError::StoreFailed)?
    .ok_or(ChatError::NotFound)
}

include!("write_parts/part_01.rs");
include!("write_parts/part_02.rs");
