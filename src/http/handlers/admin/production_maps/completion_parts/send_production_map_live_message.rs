async fn send_production_map_live_message(socket: &mut WebSocket, message: Message) -> bool {
    // Byte size is logged on failures so oversized snapshots are visible in
    // ops logs before operators start reporting missing orders.
    let byte_len = match &message {
        Message::Text(text) => text.len(),
        Message::Binary(bytes) => bytes.len(),
        Message::Ping(bytes) | Message::Pong(bytes) => bytes.len(),
        Message::Close(_) => 0,
    };
    match timeout(LIVE_SEND_TIMEOUT, socket.send(message)).await {
        Ok(Ok(())) => true,
        Ok(Err(error)) => {
            tracing::warn!(%error, byte_len, "production map live message send failed");
            false
        }
        Err(_) => {
            tracing::warn!(
                byte_len,
                send_timeout_secs = LIVE_SEND_TIMEOUT.as_secs(),
                "production map live message send timed out"
            );
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
            Capability::ProductionMapRead,
            Capability::ApparatusQueueRead,
        ],
    )
    .await?;
    let completed_orders = if principal.role.is_apparatus_operator() {
        let assigned = state.principal_assigned_apparatus(&principal).await;
        state
            .production_maps
            .completed_queue_orders_for_apparatuses(&assigned, 200)
            .await
    } else {
        state
            .production_maps
            .completed_queue_orders_for_actor(&queue_action_actor(&principal).ref_, 200)
            .await
    }
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
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::ProductionMapRead,
        ],
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
        if !result.raw_material_stock_committed {
            let _queue_action_guard = state.production_maps.queue_action_guard().await;
            let material_barcodes = raw_material_barcodes_for_order_apparatus(
                &state,
                &result.decision.order_id,
                &result.decision.apparatus,
            )
            .await?;
            if !material_barcodes.is_empty() {
                for warehouse in settle_completion_raw_materials_fallback(
                    &state,
                    &result.decision.order_id,
                    &material_barcodes,
                )
                .await?
                {
                    state
                        .warehouse_events
                        .notify_updated(&warehouse, "raw_material_stock");
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
            Capability::ProductionMapRead,
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
            Capability::ProductionMapRead,
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
