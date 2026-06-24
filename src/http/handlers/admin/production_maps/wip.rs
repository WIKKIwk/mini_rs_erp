use super::*;

#[derive(Default, serde::Deserialize)]
pub struct WipBatchesQuery {
    #[serde(default)]
    apparatus: String,
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
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let status = if query.status.trim().is_empty() {
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
            status,
            &query.order_id,
            positive_int(query.limit.as_deref(), 100),
        )
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "batches": batches,
    })))
}
