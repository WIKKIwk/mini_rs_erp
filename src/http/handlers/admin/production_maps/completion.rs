use super::*;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use tokio::time::{Duration, timeout};

const LIVE_SEND_TIMEOUT: Duration = Duration::from_secs(5);
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
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, AdminError> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal_for_live(&state, &headers, &query.token).await?;
    require_any_live_capability(&state, &principal).await?;
    let include_completion_requests = matches!(principal.role, PrincipalRole::Admin);
    Ok(ws
        .on_upgrade(move |socket| {
            production_map_live_socket(
                state,
                socket,
                queue_action_actor(&principal).ref_,
                include_completion_requests,
            )
        })
        .into_response())
}

#[derive(Default, serde::Deserialize)]
pub struct ProductionMapLiveQuery {
    #[serde(default)]
    token: String,
}

async fn authenticated_principal_for_live(
    state: &AppState,
    headers: &HeaderMap,
    query_token: &str,
) -> Result<Principal, AdminError> {
    let token = query_token.trim().to_string();
    let token = if token.is_empty() {
        bearer_token(headers).ok_or_else(unauthorized)?
    } else {
        token
    };
    state.sessions.get(&token).await.map_err(|_| unauthorized())
}

async fn require_any_live_capability(
    state: &AppState,
    principal: &Principal,
) -> Result<(), AdminError> {
    for capability in [
        Capability::AdminAccess,
        Capability::ProductionMapManage,
        Capability::ApparatusQueueRead,
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
    actor_ref: String,
    include_completion_requests: bool,
) {
    let service = state.production_maps.clone();
    let mut rx = service.subscribe_live();
    let mut heartbeat = tokio::time::interval(LIVE_HEARTBEAT_INTERVAL);
    let mut last_payload = String::new();

    if !send_production_map_live_snapshot(
        &service,
        &mut socket,
        &actor_ref,
        include_completion_requests,
        &mut last_payload,
    )
    .await
    {
        return;
    }

    loop {
        tokio::select! {
            received = rx.recv() => {
                match received {
                    Ok(()) => {
                        if !send_production_map_live_snapshot(
                            &service,
                            &mut socket,
                            &actor_ref,
                            include_completion_requests,
                            &mut last_payload,
                        ).await {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        if !send_production_map_live_snapshot(
                            &service,
                            &mut socket,
                            &actor_ref,
                            include_completion_requests,
                            &mut last_payload,
                        ).await {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            _ = heartbeat.tick() => {
                if !send_production_map_live_message(&mut socket, Message::Ping(Vec::new().into())).await {
                    break;
                }
            }
        }
    }
}

async fn send_production_map_live_snapshot(
    service: &crate::core::production_map::ProductionMapService,
    socket: &mut WebSocket,
    actor_ref: &str,
    include_completion_requests: bool,
    last_payload: &mut String,
) -> bool {
    let snapshot = service.live_snapshot().await;
    let completed_orders = service
        .completed_queue_orders_for_actor(actor_ref, 200)
        .await;
    let completion_requests = if include_completion_requests {
        service.completion_requests(200).await
    } else {
        Ok(Vec::new())
    };
    let completion_request_decisions = service
        .completion_request_decisions_for_actor(actor_ref, 200)
        .await;
    match (
        snapshot,
        completed_orders,
        completion_requests,
        completion_request_decisions,
    ) {
        (
            Ok(snapshot),
            Ok(completed_orders),
            Ok(completion_requests),
            Ok(completion_request_decisions),
        ) => {
            let payload = serde_json::json!({
                "ok": true,
                "maps": snapshot.maps,
                "sequences": snapshot.sequences,
                "visible_order_ids": snapshot.visible_order_ids,
                "queue_states": snapshot.queue_states,
                "queue_policies": snapshot.queue_policies,
                "completed_orders": completed_orders,
                "completion_requests": completion_requests,
                "completion_request_decisions": completion_request_decisions,
            });
            match serde_json::to_string(&payload) {
                Ok(json) => {
                    if json == *last_payload {
                        return true;
                    }
                    *last_payload = json.clone();
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
            match serde_json::to_string(&payload) {
                Ok(json) => {
                    send_production_map_live_message(socket, Message::Text(json.into())).await
                }
                Err(_) => true,
            }
        }
    }
}

async fn send_production_map_live_message(socket: &mut WebSocket, message: Message) -> bool {
    match timeout(LIVE_SEND_TIMEOUT, socket.send(message)).await {
        Ok(Ok(())) => true,
        Ok(Err(error)) => {
            tracing::warn!(%error, "production map live message send failed");
            false
        }
        Err(_) => {
            tracing::warn!("production map live message send timed out");
            false
        }
    }
}

pub async fn production_map_completed_orders(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::ApparatusQueueRead,
        ],
    )
    .await?;
    let completed_orders = state
        .production_maps
        .completed_queue_orders_for_actor(&queue_action_actor(&principal).ref_, 200)
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "completed_orders": completed_orders,
    })))
}

pub async fn production_map_completion_requests(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    let completion_requests = state
        .production_maps
        .completion_requests(200)
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "completion_requests": completion_requests,
    })))
}

pub async fn production_map_completion_request_decision(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    let input: CompletionRequestDecisionRequest = parse_json(&body)?;
    let decision = CompletionRequestDecision::parse(&input.decision)
        .ok_or_else(|| bad_request("decision is required"))?;
    let result = state
        .production_maps
        .decide_completion_request(&input.event_id, decision, queue_action_actor(&principal))
        .await
        .map_err(production_map_error)?;
    if result.decision.decision == CompletionRequestDecision::Approved.as_str() {
        if result.raw_material_stock_warehouses.is_empty() {
            let material_barcodes = raw_material_barcodes_for_order_apparatus(
                &state,
                &result.decision.order_id,
                &result.decision.apparatus,
            )
            .await?;
            if !material_barcodes.is_empty() {
                for stock in state
                    .gscale
                    .mark_raw_material_stock_consumed(&material_barcodes, &result.decision.order_id)
                    .await
                    .map_err(raw_material_stock_status_error)?
                {
                    state
                        .warehouse_events
                        .notify_updated(&stock.warehouse, "raw_material_stock");
                }
            }
        } else {
            for warehouse in &result.raw_material_stock_warehouses {
                state
                    .warehouse_events
                    .notify_updated(warehouse, "raw_material_stock");
            }
        }
    }
    let order_status = state
        .production_maps
        .order_status_detail(&result.decision.order_id)
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "states": result.states,
        "decision": result.decision,
        "order_status": order_status,
    })))
}

pub async fn production_map_completion_request_decisions(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::ApparatusQueueRead,
            Capability::ApparatusQueueManage,
        ],
    )
    .await?;
    let decisions = state
        .production_maps
        .completion_request_decisions_for_actor(&queue_action_actor(&principal).ref_, 200)
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "completion_request_decisions": decisions,
    })))
}

pub async fn production_map_closed_orders(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::ApparatusQueueRead,
        ],
    )
    .await?;
    let closed_orders = state
        .production_maps
        .fully_completed_orders(200)
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "closed_orders": closed_orders,
    })))
}
