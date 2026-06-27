use super::*;

#[derive(Default, serde::Deserialize)]
pub struct WipBatchesQuery {
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    next_apparatus: String,
    #[serde(default)]
    current_location: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    limit: Option<String>,
}

pub async fn production_map_wip_batches(
    State(state): State<AppState>,
    Query(query): Query<WipBatchesQuery>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
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
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let can_view_all = state
        .admin
        .principal_has_capability(&principal, Capability::AdminAccess)
        .await
        || state
            .admin
            .principal_has_capability(&principal, Capability::ProductionMapManage)
            .await;
    if !can_view_all {
        let assigned_apparatus = state.admin.principal_assigned_apparatus(&principal).await;
        let scoped_to_current =
            queue_state::apparatus_matches_assigned(&query.apparatus, &assigned_apparatus);
        let scoped_to_next =
            queue_state::apparatus_matches_assigned(&query.next_apparatus, &assigned_apparatus);
        if !scoped_to_current && !scoped_to_next {
            return Err(forbidden());
        }
    }
    let include_processed = query.status.trim().eq_ignore_ascii_case("all");
    let status = if query.status.trim().is_empty() || include_processed {
        None
    } else {
        Some(
            OrderProgressBatchWipStatus::parse(&query.status)
                .ok_or_else(|| bad_request("invalid wip status"))?,
        )
    };
    let batches = state
        .production_maps
        .wip_progress_batches(
            &query.apparatus,
            &query.next_apparatus,
            &query.current_location,
            status,
            include_processed,
            &query.order_id,
            positive_int(query.limit.as_deref(), 100),
        )
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "batches": batches,
    })))
}

#[derive(Default, serde::Deserialize)]
struct FinishedGoodsReceiveRequest {
    #[serde(default)]
    progress_batch_id: String,
    #[serde(default)]
    progress_qr: String,
    #[serde(default)]
    qr_payload: String,
    #[serde(default)]
    warehouse: String,
}

pub async fn production_map_finished_goods_receive(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::GscalePrint,
        ],
    )
    .await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: FinishedGoodsReceiveRequest = parse_json(&body)?;
    let qr_payload = if input.qr_payload.trim().is_empty() {
        input.progress_qr.clone()
    } else {
        input.qr_payload.clone()
    };
    let receipt = state
        .production_maps
        .receive_finished_goods(
            &input.progress_batch_id,
            &qr_payload,
            &input.warehouse,
            queue_action_actor(&principal),
        )
        .await
        .map_err(production_map_error)?;
    state
        .warehouse_events
        .notify_updated(&receipt.stock.warehouse, "finished_goods_stock");
    Ok(json_response(serde_json::json!({
        "ok": true,
        "batch": receipt.batch,
        "stock": receipt.stock,
        "order_status": receipt.order_status,
    })))
}
