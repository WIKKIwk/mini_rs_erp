use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use tokio::task::JoinSet;
use tokio::time::sleep;

use super::{
    ChatConversation, ChatConversationPage, ChatError, ChatHub, ChatMessagePage, ChatPrincipal,
    ChatPrincipalInput, ChatSendResult, ChatStorePort, ChatSyncPage, can_participate_in_chat,
};
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::push::service::PushService;

include!("service_parts/part_01.rs");
include!("service_parts/part_02.rs");
include!("service_parts/part_03.rs");
