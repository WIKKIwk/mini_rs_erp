use super::*;

#[derive(serde::Deserialize)]
struct CompletionRequestDecisionRequest {
    #[serde(default)]
    event_id: String,
    #[serde(default)]
    decision: String,
}

/// Pushes production-map queue snapshots over SSE so operators see changes
/// instantly without polling.
pub async fn production_map_live(
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
    let include_completion_requests = matches!(principal.role, PrincipalRole::Admin);
    Ok(production_map_live_sse(
        state,
        queue_action_actor(&principal).ref_,
        include_completion_requests,
    )
    .into_response())
}

fn production_map_live_sse(
    state: AppState,
    actor_ref: String,
    include_completion_requests: bool,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let service = state.production_maps.clone();
    let mut rx = service.subscribe_live();
    let event_stream = stream! {
        let mut last_payload = String::new();
        loop {
            let snapshot = service.live_snapshot().await;
            let completed_orders = service
                .completed_queue_orders_for_actor(&actor_ref, 200)
                .await;
            let completion_requests = if include_completion_requests {
                service.completion_requests(200).await
            } else {
                Ok(Vec::new())
            };
            let completion_request_decisions = service
                .completion_request_decisions_for_actor(&actor_ref, 200)
                .await;
            match (snapshot, completed_orders, completion_requests, completion_request_decisions) {
                (Ok(snapshot), Ok(completed_orders), Ok(completion_requests), Ok(completion_request_decisions)) => {
                    let payload = serde_json::json!({
                        "ok": true,
                        "maps": snapshot.maps,
                        "sequences": snapshot.sequences,
                        "queue_states": snapshot.queue_states,
                        "queue_policies": snapshot.queue_policies,
                        "completed_orders": completed_orders,
                        "completion_requests": completion_requests,
                        "completion_request_decisions": completion_request_decisions,
                    });
                    if let Ok(json) = serde_json::to_string(&payload) {
                        if json != last_payload {
                            last_payload = json.clone();
                            yield Ok(Event::default().event("snapshot").data(json));
                        }
                    }
                }
                (Err(error), _, _, _)
                | (_, Err(error), _, _)
                | (_, _, Err(error), _)
                | (_, _, _, Err(error)) => {
                    yield Ok(Event::default().event("error").data(error.to_string()));
                }
            }

            match rx.recv().await {
                Ok(()) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    };
    Sse::new(event_stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
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
    }
    Ok(json_response(serde_json::json!({
        "ok": true,
        "states": result.states,
        "decision": result.decision,
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
