
async fn send_image_to_selected_group(
    client: &Client,
    selected_chat_id: &str,
    selected_chat_type: &str,
    text: &str,
    image: &CalculateOrderImage,
) -> Result<(), UserAccountError> {
    let peer = selected_group_peer(client, selected_chat_id, selected_chat_type).await?;
    let file_name = if image.image_name.trim().is_empty() {
        "order-image.jpg"
    } else {
        image.image_name.trim()
    };
    let uploaded = client
        .upload(Cursor::new(image.body.clone()), file_name)
        .await
        .map_err(map_transport)?;
    let input_media = if matches!(
        image.image_mime.trim().to_ascii_lowercase().as_str(),
        "image/jpeg" | "image/jpg" | "image/png"
    ) {
        uploaded.as_auto_media()
    } else {
        uploaded.as_document_media()
    };
    client
        .send_message(peer, InputMessage::text(text).copy_media(input_media))
        .await
        .map_err(map_transport)?;
    Ok(())
}

async fn selected_group_peer(
    client: &Client,
    selected_chat_id: &str,
    selected_chat_type: &str,
) -> Result<ferogram::tl::enums::Peer, UserAccountError> {
    for folder_id in [0, 1] {
        let mut iter = client.iter_dialogs().folder_id(Some(folder_id));
        while let Some(dialog) = iter.next(client).await.map_err(map_transport)? {
            let Some(group) = writable_group_from_dialog(&dialog) else {
                continue;
            };
            if group.chat_id != selected_chat_id || group.chat_type != selected_chat_type {
                continue;
            }
            let peer = dialog
                .peer()
                .cloned()
                .ok_or(UserAccountError::GroupNotWritable)?;
            return Ok(peer);
        }
    }
    Err(UserAccountError::GroupNotWritable)
}

fn writable_group_from_dialog(dialog: &ferogram::Dialog) -> Option<TelegramUserGroup> {
    let raw_chat = dialog.chat.clone()?;
    let chat = ferogram::types::Chat::from_raw(raw_chat)?;
    match chat {
        ferogram::types::Chat::Group(group) => {
            if group.raw.left
                || group.raw.deactivated
                || banned_send_messages(group.raw.default_banned_rights.as_ref())
            {
                return None;
            }
            Some(TelegramUserGroup {
                chat_id: group.id().to_string(),
                title: group.title().to_string(),
                chat_type: "group".to_string(),
                username: String::new(),
            })
        }
        ferogram::types::Chat::Channel(channel) => {
            let admin_can_post = channel
                .admin_rights()
                .is_some_and(|rights| rights.post_messages);
            if !channel.megagroup()
                || channel.raw.left
                || (!admin_can_post
                    && (banned_send_messages(channel.raw.banned_rights.as_ref())
                        || banned_send_messages(channel.raw.default_banned_rights.as_ref())))
            {
                return None;
            }
            Some(TelegramUserGroup {
                chat_id: channel.id().to_string(),
                title: channel.title().to_string(),
                chat_type: "supergroup".to_string(),
                username: channel.username().unwrap_or_default().to_string(),
            })
        }
        ferogram::types::Chat::Community(_) => None,
    }
}

fn banned_send_messages(rights: Option<&ferogram::tl::enums::ChatBannedRights>) -> bool {
    matches!(
        rights,
        Some(ferogram::tl::enums::ChatBannedRights::ChatBannedRights(rights))
            if rights.send_messages
    )
}

fn normalize_phone(value: &str) -> String {
    let digits: String = value.chars().filter(char::is_ascii_digit).collect();
    if digits.is_empty() {
        String::new()
    } else {
        format!("+{digits}")
    }
}

fn map_transport<E: std::fmt::Display>(error: E) -> UserAccountError {
    UserAccountError::Transport(error.to_string())
}

pub(super) fn map_send_code_error(error: ferogram::InvocationError) -> UserAccountError {
    // Rasmiy hujjat (core.telegram.org/method/auth.resendCode):
    // SEND_CODE_UNAVAILABLE = bu raqam uchun hamma usullar ishlatib bo'lindi,
    // keyingi resend boshqa usulni ishga tushirishi mumkin.
    use ferogram::InvocationErrorExt;
    match error.kind() {
        ferogram::ErrorKind::Rpc { name, .. } if name == "SEND_CODE_UNAVAILABLE" => {
            UserAccountError::SendCodeUnavailable
        }
        _ => map_transport(error),
    }
}

fn map_store(error: TelegramStoreError) -> UserAccountError {
    match error {
        TelegramStoreError::UserNotFound => UserAccountError::NotAuthorized,
        other => UserAccountError::Store(other.to_string()),
    }
}

pub use super::models::{TelegramAccountRole, TelegramUserAccount};
