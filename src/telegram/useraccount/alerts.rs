use super::*;
use crate::telegram::alerts::models::AlertJob;
use ferogram::tl;

impl TelegramUserAccountService {
    pub(crate) async fn send_alert(&self, job: &AlertJob) -> Result<(), UserAccountError> {
        let (client, shutdown) = self.authorized_client(&job.sender_user_id).await?;
        let _disconnect = shutdown.drop_guard();
        async {
            let peer =
                selected_group_peer(&client, &job.group.chat_id, &job.group.chat_type).await?;
            let peer = client
                .resolve_to_input_peer(&peer)
                .await
                .map_err(map_transport)?;
            let request = alert_request(peer, job);
            client.invoke(&request).await.map_err(|error| {
                if let ferogram::ErrorKind::FloodWait(seconds) = error.kind() {
                    UserAccountError::FloodWait { seconds }
                } else {
                    map_transport(error)
                }
            })?;
            Ok(())
        }
        .await
    }
}

fn utf16_prefix(value: &str, max: usize) -> String {
    let mut count = 0;
    value
        .chars()
        .take_while(|c| {
            count += c.len_utf16();
            count <= max
        })
        .collect()
}

pub(crate) fn alert_request(
    peer: tl::enums::InputPeer,
    job: &AlertJob,
) -> tl::functions::messages::SendMessage {
    let mut message = String::new();
    let mut entities = Vec::new();
    for (index, member) in job.members.iter().enumerate() {
        if index > 0 {
            message.push_str(", ");
        }
        let label = if member.username.is_empty() {
            if member.display_name.is_empty() {
                member.user_id.to_string()
            } else {
                member.display_name.clone()
            }
        } else {
            format!("@{}", member.username)
        };
        let label = utf16_prefix(&label, 64);
        let offset = message.encode_utf16().count() as i32;
        let length = label.encode_utf16().count() as i32;
        message.push_str(&label);
        // A Telegram user ID mention also works when the member has no @username.
        entities.push(
            tl::types::InputMessageEntityMentionName {
                offset,
                length,
                user_id: tl::types::InputUser {
                    user_id: member.user_id,
                    access_hash: member.access_hash.unwrap_or_default(),
                }
                .into(),
            }
            .into(),
        );
    }
    if !message.is_empty() {
        message.push_str(", ");
    }
    message.push_str(&utf16_prefix(&job.message, 2000));
    tl::functions::messages::SendMessage {
        peer,
        message,
        random_id: job.random_id,
        entities: Some(entities),
        no_webpage: true,
        silent: false,
        background: false,
        clear_draft: false,
        noforwards: false,
        update_stickersets_order: false,
        invert_media: false,
        allow_paid_floodskip: false,
        reply_to: None,
        reply_markup: None,
        schedule_date: None,
        schedule_repeat_period: None,
        send_as: None,
        quick_reply_shortcut: None,
        effect: None,
        allow_paid_stars: None,
        suggested_post: None,
        rich_message: None,
    }
}
