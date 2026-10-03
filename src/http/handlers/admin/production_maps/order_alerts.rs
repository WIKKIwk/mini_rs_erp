use super::*;
use crate::core::chat::{ChatError, ChatPrincipalInput};
use std::collections::BTreeSet;

#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum OrderAlertKind {
    RawMaterial,
    Qolip,
}

impl OrderAlertKind {
    fn role(self) -> PrincipalRole {
        match self {
            Self::RawMaterial => PrincipalRole::MaterialTaminotchi,
            Self::Qolip => PrincipalRole::Qolipchi,
        }
    }

    fn code(self) -> &'static str {
        match self {
            Self::RawMaterial => "raw_material",
            Self::Qolip => "qolip",
        }
    }

    fn message(self, order: &str, apparatus: &str, worker: &str) -> String {
        let item = match self {
            Self::RawMaterial => "homashyo",
            Self::Qolip => "qolip",
        };
        format!(
            "№{order} orderga {item} biriktirib bering.\nApparat: {apparatus}\nIshchi: {worker}"
        )
    }

    fn telegram_message(self, order: &str, title: &str, apparatus: &str, worker: &str) -> String {
        let item = match self {
            Self::RawMaterial => "homashyo",
            Self::Qolip => "qolip",
        };
        let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
        let order = order.trim().trim_start_matches('№');
        let order = if title.is_empty() {
            format!("№{order}")
        } else {
            format!("№{order} {title}")
        };
        let worker = worker.trim();
        let worker = if worker.to_lowercase().ends_with(" aka") {
            worker.to_string()
        } else {
            format!("{worker} aka")
        };
        format!(
            "{order} buyurtmasiga {item} biriktirib bering, {}'dagi {worker} kutyapti",
            apparatus.trim(),
        )
    }
}

#[derive(serde::Deserialize)]
struct OrderAlertCommand {
    order_id: String,
    apparatus: String,
    kind: OrderAlertKind,
    request_id: String,
}

pub async fn production_map_order_alert(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal =
        authorize_any_capability(&state, &headers, &[Capability::ApparatusQueueManage]).await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    if principal.role != PrincipalRole::Aparatchi {
        return Err(forbidden());
    }
    let command: OrderAlertCommand = parse_json(&body)?;
    let request_id = command.request_id.trim();
    if request_id.is_empty()
        || request_id.len() > 80
        || !request_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        || command.order_id.trim().is_empty()
    {
        return Err(bad_request("order_alert_invalid_request"));
    }
    let apparatus = queue_actions::resolve_queue_apparatus(&state, &command.apparatus).await?;
    let assigned = state.admin.principal_assigned_apparatus(&principal).await;
    if !raw_material_details::assigned_apparatus_contains(apparatus.id.as_str(), &assigned) {
        return Err(forbidden());
    }
    let map = state
        .production_maps
        .raw_map(&command.order_id)
        .await
        .map_err(production_map_error)?
        .ok_or_else(|| not_found("map_not_found"))?;
    if !map.nodes.iter().any(|node| {
        node.kind == ProductionMapNodeKind::Apparatus
            && node.canonical_apparatus_id().as_ref() == Some(&apparatus.id)
    }) {
        return Err(forbidden());
    }
    let recipients = alert_recipients(&state, command.kind).await?;
    let order = if map.order_number.trim().is_empty() {
        &map.id
    } else {
        &map.order_number
    };
    let message = command
        .kind
        .message(order, &apparatus.display_name, &principal.display_name);
    // Each recipient's existing chat deduplicates a transport retry with the same command ID.
    let client_message_id = format!(
        "order-alert:{}:{}:{request_id}",
        map.id,
        command.kind.code()
    );
    let recipient_count = recipients.len();
    for recipient in recipients {
        let conversation = state
            .chat
            .create_or_get_dm(
                ChatPrincipalInput {
                    role: principal.role,
                    ref_: principal.ref_.clone(),
                    display_name: principal.display_name.clone(),
                    avatar_url: principal.avatar_url.clone(),
                },
                recipient,
            )
            .await
            .map_err(alert_delivery_error)?;
        // Chat persists the message and its push outbox together, including delivery retries.
        state
            .chat
            .send_message(
                &principal,
                &conversation.conversation_id,
                &client_message_id,
                &message,
            )
            .await
            .map_err(alert_delivery_error)?;
    }
    let kind = match command.kind {
        OrderAlertKind::RawMaterial => crate::telegram::alerts::AlertKind::RawMaterial,
        OrderAlertKind::Qolip => crate::telegram::alerts::AlertKind::Qolip,
    };
    let telegram_id = serde_json::to_string(&(
        &principal.ref_,
        apparatus.id.as_str(),
        &map.id,
        kind.code(),
        request_id,
    ))
    .map_err(|_| server_error("order_alert_send_failed"))?;
    let telegram_queued = state
        .telegram
        .enqueue_order_alert(
            telegram_id,
            kind,
            command.kind.telegram_message(
                order,
                &map.title,
                &apparatus.display_name,
                &principal.display_name,
            ),
        )
        .await
        .map_err(|error| {
            tracing::warn!(%error, "Telegram order alert was not queued");
            server_error("order_alert_send_failed")
        })?;
    if recipient_count == 0 && !telegram_queued {
        return Err(bad_request("order_alert_no_recipients"));
    }
    Ok(json_response(serde_json::json!({
        "ok": true, "recipient_count": recipient_count, "telegram_queued": telegram_queued,
    })))
}

async fn alert_recipients(
    state: &AppState,
    kind: OrderAlertKind,
) -> Result<Vec<ChatPrincipalInput>, AdminError> {
    let mut recipients = Vec::new();
    match kind {
        OrderAlertKind::RawMaterial => {
            let mut offset = 0;
            loop {
                let page = state
                    .admin
                    .user_list_page("", 100, offset, Some("material_taminotchi"))
                    .await
                    .map_err(|_| server_error("order_alert_recipients_failed"))?;
                offset += page.items.len();
                for user in page.items {
                    if user.principal_role == kind.role()
                        && !user.blocked
                        && user.status != "removed"
                    {
                        recipients.push(ChatPrincipalInput {
                            role: user.principal_role,
                            ref_: user.entity_ref,
                            display_name: user.name,
                            avatar_url: user.avatar_url,
                        });
                    }
                }
                if !page.has_more {
                    break;
                }
            }
        }
        OrderAlertKind::Qolip => {
            let users = state
                .system_users
                .users(&kind.role(), "", 500)
                .await
                .map_err(|_| server_error("order_alert_recipients_failed"))?;
            for user in users {
                let detail = match state.admin.system_user_detail(user).await {
                    Ok(detail) => detail,
                    Err(crate::core::admin::ports::AdminPortError::NotFound) => continue,
                    Err(_) => return Err(server_error("order_alert_recipients_failed")),
                };
                if !detail.blocked {
                    recipients.push(ChatPrincipalInput {
                        role: detail.role,
                        ref_: detail.id,
                        display_name: detail.name,
                        avatar_url: detail.avatar_url,
                    });
                }
            }
        }
    }
    let mut seen = BTreeSet::new();
    recipients.retain(|user| !user.ref_.trim().is_empty() && seen.insert(user.ref_.clone()));
    Ok(recipients)
}

fn alert_delivery_error(error: ChatError) -> AdminError {
    tracing::warn!(%error, "order alert delivery failed");
    server_error("order_alert_send_failed")
}

#[cfg(test)]
mod message_tests {
    use super::OrderAlertKind;

    #[test]
    fn telegram_alert_is_a_plain_sentence_with_order_title_apparatus_and_worker() {
        for (kind, item) in [
            (OrderAlertKind::RawMaterial, "homashyo"),
            (OrderAlertKind::Qolip, "qolip"),
        ] {
            assert_eq!(
                kind.telegram_message(
                    "0023",
                    "guruch alanga arzon 1kg",
                    "8 ta rangli bosma aparat",
                    "Nuriddin",
                ),
                format!(
                    "№0023 guruch alanga arzon 1kg buyurtmasiga {item} biriktirib bering, 8 ta rangli bosma aparat'dagi Nuriddin aka kutyapti"
                ),
            );
        }
    }

    #[test]
    fn telegram_alert_trims_title_and_avoids_duplicate_number_prefix_or_honorific() {
        assert_eq!(
            OrderAlertKind::RawMaterial.telegram_message(
                " №0023 ",
                " guruch\n alanga ",
                " Bosma ",
                " Nuriddin aka "
            ),
            "№0023 guruch alanga buyurtmasiga homashyo biriktirib bering, Bosma'dagi Nuriddin aka kutyapti",
        );
        assert_eq!(
            OrderAlertKind::Qolip.telegram_message("0023", "", "Bosma", "Nuriddin"),
            "№0023 buyurtmasiga qolip biriktirib bering, Bosma'dagi Nuriddin aka kutyapti",
        );
    }
}
