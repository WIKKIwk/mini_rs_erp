use super::*;
use crate::http::json_research;
use crate::http::ws::{Message, WebSocket, WebSocketUpgrade};
use tokio::time::{Duration, timeout};

// Large order lists serialize to multi-megabyte snapshots. Dropping a slow
// consumer too eagerly forces a reconnect storm where every client refetches
// the same full snapshot at once, so the timeout favors riding out spikes.
const LIVE_SEND_TIMEOUT: Duration = Duration::from_secs(15);
const LIVE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(25);

#[derive(serde::Deserialize)]
struct CompletionRequestDecisionRequest {
    #[serde(default)]
    event_id: String,
    #[serde(default)]
    decision: String,
}

/// Pushes production-map queue snapshots over WebSocket so operators see changes
/// instantly without polling.
pub async fn production_map_live(
    State(state): State<AppState>,
    Query(query): Query<ProductionMapLiveQuery>,
    method: Method,
    axum::Extension(session): axum::Extension<crate::http::router::LiveSession>,
    ws: WebSocketUpgrade,
) -> Result<Response, AdminError> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let paged_access = query.paged || query.protocol.as_deref() == Some("page_delta_v1");
    let ws = session.protect(ws, &state, move |state, actor| async move {
        if paged_access {
            require_paged_live_capability(&state, &actor).await.is_ok()
        } else {
            require_any_live_capability(&state, &actor).await.is_ok()
        }
    }).with_denial_notices(
        None, Some(r#"{"ok":false,"error":"forbidden"}"#),
    );
    let principal = session.principal;
    require_any_live_capability(&state, &principal).await?;
    if principal.role == PrincipalRole::RezkaMaster
        && (query.paged || !matches!(query.protocol.as_deref(), Some("state_delta_v1" | "state_delta_v2")))
    {
        return Err(forbidden());
    }
    let worker_scope = query.worker_scope || principal.role == PrincipalRole::RezkaMaster;
    let include_completion_requests = principal.role.is_admin_workspace_manager();
    let delta_enabled = query.protocol.as_deref() == Some("delta");
    let compact_state_delta = query.protocol.as_deref() == Some("state_delta_v2");
    let state_delta_enabled =
        compact_state_delta || query.protocol.as_deref() == Some("state_delta_v1");
    let page_delta_enabled = query.protocol.as_deref() == Some("page_delta_v1");
    let notifications_enabled = query.protocol.as_deref() == Some("notifications_v1");
    let page_windows = if page_delta_enabled {
        require_paged_live_capability(&state, &principal).await?;
        Some(super::page_live::requests(&query.pages)?)
    } else {
        None
    };
    if query.paged {
        if !delta_enabled && !page_delta_enabled {
            return Err(bad_request("paged_live_requires_delta"));
        }
        require_paged_live_capability(&state, &principal).await?;
    }
    let window = super::worker_snapshot::WorkerQueueWindow::new(
        query.queue_limit,
        &query.queue_search,
        worker_scope && state_delta_enabled,
    )?
    .with_positions(query.queue_positions)
    .with_projection(&query.projection, worker_scope && compact_state_delta)?;
    Ok(ws
        .on_upgrade(move |socket| {
            if let Some(windows) = page_windows {
                return Box::pin(super::page_live::socket(state, socket, principal, windows))
                    as std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;
            }
            if notifications_enabled {
                return Box::pin(super::page_live::notifications_socket(
                    state,
                    socket,
                    principal,
                    include_completion_requests,
                ));
            }
            if state_delta_enabled {
                return super::live_delta::production_map_state_live_socket(
                    state,
                    socket,
                    principal,
                    include_completion_requests,
                    query.epoch,
                    query.rev,
                    worker_scope,
                    query.scope,
                    window,
                    compact_state_delta,
                );
            }
            // Box both protocol futures to retain the legacy wire contract.
            Box::pin(async move {
                production_map_live_socket(
                    state,
                    socket,
                    principal,
                    include_completion_requests,
                    delta_enabled,
                    query.paged,
                )
                .await
            })
        })
        .into_response())
}

#[derive(Default, serde::Deserialize)]
pub struct ProductionMapLiveQuery {
    #[serde(default)]
    protocol: Option<String>,
    #[serde(default)]
    epoch: String,
    #[serde(default)]
    rev: Option<u64>,
    #[serde(default)]
    worker_scope: bool,
    #[serde(default)]
    scope: String,
    #[serde(default)]
    queue_limit: Option<usize>,
    #[serde(default)]
    queue_search: String,
    #[serde(default)]
    queue_positions: bool,
    #[serde(default)]
    paged: bool,
    #[serde(default)]
    pages: String,
    #[serde(default)]
    projection: String,
}

pub(super) async fn require_any_live_capability(
    state: &AppState,
    principal: &Principal,
) -> Result<(), AdminError> {
    for capability in [
        Capability::AdminAccess,
        Capability::ProductionMapManage,
        Capability::ProductionMapRead,
        Capability::ApparatusQueueRead,
        Capability::RawMaterialAssign,
        Capability::QolipManage,
        Capability::PreparationAccess,
    ] {
        if state
            .admin
            .principal_has_capability(principal, capability)
            .await
        {
            return Ok(());
        }
    }
    Err(forbidden())
}

pub(super) async fn require_paged_live_capability(
    state: &AppState,
    principal: &Principal,
) -> Result<(), AdminError> {
    for capability in [
        Capability::AdminAccess,
        Capability::ProductionMapManage,
        Capability::ProductionMapRead,
    ] {
        if state
            .admin
            .principal_has_capability(principal, capability)
            .await
        {
            return Ok(());
        }
    }
    Err(forbidden())
}

async fn production_map_live_socket(
    state: AppState,
    mut socket: WebSocket,
    principal: Principal,
    include_completion_requests: bool,
    delta_enabled: bool,
    paged: bool,
) {
    let service = state.production_maps.clone();
    let mut rx = service.subscribe_live();
    let mut heartbeat = tokio::time::interval(LIVE_HEARTBEAT_INTERVAL);
    // Fingerprint of the last payload sent on this connection. Stored as a
    // hash so a multi-megabyte snapshot is not duplicated in memory per
    // connected operator.
    let mut last_payload_fingerprint: u64 = 0;
    let mut last_payload_initialized = false;

    if !send_production_map_live_snapshot(
        &state,
        &mut socket,
        &principal,
        include_completion_requests,
        &mut last_payload_fingerprint,
        &mut last_payload_initialized,
        paged,
    )
    .await
    {
        return;
    }

    loop {
        tokio::select! {
            // Reading drives tungstenite's automatic Pong/Close handling.
            // Without it, a healthy client heartbeat appears to time out.
            inbound = socket.recv() => {
                match inbound {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }
            received = rx.recv() => {
                match received {
                    Ok(crate::core::production_map::ProductionMapLiveEvent::Invalidate
                        | crate::core::production_map::ProductionMapLiveEvent::PrintPreflight { .. }) => {
                        if !send_production_map_live_snapshot(
                            &state,
                            &mut socket,
                            &principal,
                            include_completion_requests,
                            &mut last_payload_fingerprint,
                            &mut last_payload_initialized,
                            paged,
                        ).await {
                            break;
                        }
                    }
                    Ok(crate::core::production_map::ProductionMapLiveEvent::Delta(delta)) => {
                        if delta_enabled {
                            let mut payload = serde_json::json!({
                                "ok": true,
                                "type": "delta",
                                "epoch": &delta.epoch,
                                "apparatus": &delta.apparatus,
                                "base_revision": delta.base_revision,
                                "revision": delta.revision,
                                "ops": &delta.ops,
                                "version": &delta.version,
                                "base_version": delta.ops.first().and_then(|op| op.get("base_version")),
                            });
                            if paged {
                                payload["snapshot_rev"] = service.snapshot_revision().into();
                            }
                            if let Ok(json) = json_research::to_string(&payload) {
                                if !send_production_map_live_message(&mut socket, Message::Text(json.into())).await {
                                    break;
                                }
                            }
                        } else {
                            if !send_production_map_live_snapshot(
                                &state,
                                &mut socket,
                                &principal,
                                include_completion_requests,
                                &mut last_payload_fingerprint,
                                &mut last_payload_initialized,
                                paged,
                            ).await {
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        if !send_production_map_live_snapshot(
                            &state,
                            &mut socket,
                            &principal,
                            include_completion_requests,
                            &mut last_payload_fingerprint,
                            &mut last_payload_initialized,
                            paged,
                        ).await {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            _ = heartbeat.tick() => {
                if paged && require_paged_live_capability(&state, &principal).await.is_err() {
                    let _ = send_production_map_live_message(&mut socket,
                        Message::Text(json_research::value_to_string(&serde_json::json!({"ok": false, "error": "forbidden"})).into())).await;
                    break;
                }
                // The transport owns native Ping/Pong deadlines. This timer
                // remains responsible only for live authorization checks.
            }
        }
    }
}

fn live_payload_fingerprint(json: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    json.hash(&mut hasher);
    hasher.finish()
}

async fn send_production_map_live_snapshot(
    state: &AppState,
    socket: &mut WebSocket,
    principal: &Principal,
    include_completion_requests: bool,
    last_payload_fingerprint: &mut u64,
    last_payload_initialized: &mut bool,
    paged: bool,
) -> bool {
    if paged {
        // Keep the existing delta operations. Bootstrap/invalidation carries
        // only a cursor; the client reconciles its already requested pages.
        let payload = serde_json::json!({
            "ok": true,
            "type": if *last_payload_initialized { "page_invalidated" } else { "page_ready" },
            "epoch": state.production_maps.snapshot_epoch(),
            "rev": state.production_maps.snapshot_revision(),
        });
        *last_payload_initialized = true;
        return send_production_map_live_message(
            socket,
            Message::Text(json_research::value_to_string(&payload).into()),
        )
        .await;
    }
    let service = &state.production_maps;
    let actor_ref = queue_action_actor(principal).ref_;
    let snapshot = match service.live_snapshot_shared_with_revision().await {
        Ok((snapshot, revision)) => {
            match super::super::training::merge_worker_training_snapshot_shared(
                state, principal, snapshot,
            )
            .await
            {
                Ok(snapshot) => Ok((snapshot, revision)),
                Err(error) => {
                    let payload = serde_json::json!({
                        "ok": false,
                        "error": error.to_string(),
                    });
                    return match json_research::to_string(&payload) {
                        Ok(json) => {
                            send_production_map_live_message(socket, Message::Text(json.into()))
                                .await
                        }
                        Err(_) => true,
                    };
                }
            }
        }
        Err(error) => Err(error),
    };
    let (completed_orders, completion_requests, completion_request_decisions) = tokio::join!(
        async {
            if principal.role.is_apparatus_operator() {
                let assigned = state.principal_assigned_apparatus(principal).await;
                service
                    .completed_queue_orders_for_apparatuses(&assigned, 200)
                    .await
            } else {
                service
                    .completed_queue_orders_for_actor(&actor_ref, 200)
                    .await
            }
        },
        async {
            if include_completion_requests {
                service.completion_requests(200).await
            } else {
                Ok(Vec::new())
            }
        },
        service.completion_request_decisions_for_actor(&actor_ref, 200),
    );
    match (
        snapshot,
        completed_orders,
        completion_requests,
        completion_request_decisions,
    ) {
        (
            Ok((snapshot, revision)),
            Ok(completed_orders),
            Ok(completion_requests),
            Ok(completion_request_decisions),
        ) => {
            let order_customers = production_map_order_customers(state, &snapshot.maps).await;
            let payload = serde_json::json!({
                "ok": true,
                "type": "snapshot",
                // Monotonic snapshot revision. Clients ignore unknown fields
                // today, but can use `rev` to detect a missed update and
                // resync instead of sitting on stale state.
                "rev": revision,
                "epoch": service.snapshot_epoch(),
                "worker_show_all_apparatus_tabs": state.show_all_worker_apparatus(&principal).await,
                "maps": &snapshot.maps,
                "sequences": &snapshot.sequences,
                "sequence_versions": &snapshot.sequence_versions,
                "sequence_revisions": &snapshot.sequence_revisions,
                "visible_order_ids": &snapshot.visible_order_ids,
                "queue_states": &snapshot.queue_states,
                "stage_states": &snapshot.stage_states,
                "queue_policies": &snapshot.queue_policies,
                "queue_action_controls": &snapshot.queue_action_controls,
                "order_statuses": &snapshot.order_statuses,
                "order_controls": &snapshot.order_controls,
                "stage_order_controls": &snapshot.stage_order_controls,
                "frozen_orders_by_apparatus": &snapshot.frozen_orders_by_apparatus,
                "order_customers": order_customers,
                "completed_orders": completed_orders,
                "completion_requests": completion_requests,
                "completion_request_decisions": completion_request_decisions,
            });
            match json_research::to_string(&payload) {
                Ok(json) => {
                    let fingerprint = live_payload_fingerprint(&json);
                    if *last_payload_initialized && fingerprint == *last_payload_fingerprint {
                        return true;
                    }
                    *last_payload_fingerprint = fingerprint;
                    *last_payload_initialized = true;
                    send_production_map_live_message(socket, Message::Text(json.into())).await
                }
                Err(error) => {
                    tracing::warn!(%error, "production map live snapshot serialization failed");
                    true
                }
            }
        }
        (Err(error), _, _, _)
        | (_, Err(error), _, _)
        | (_, _, Err(error), _)
        | (_, _, _, Err(error)) => {
            let payload = serde_json::json!({
                "ok": false,
                "error": error.to_string(),
            });
            match json_research::to_string(&payload) {
                Ok(json) => {
                    send_production_map_live_message(socket, Message::Text(json.into())).await
                }
                Err(_) => true,
            }
        }
    }
}

include!("completion_parts/send_production_map_live_message.rs");
