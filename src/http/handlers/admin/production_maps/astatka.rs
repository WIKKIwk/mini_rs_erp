use super::*;

use crate::core::production_map::queue_state;

#[derive(Default, serde::Deserialize)]
struct LaminatsiyaAstatkaRequest {
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    lamination_print_leftover_rolls: Option<f64>,
    #[serde(default)]
    lamination_film_leftover_rolls: Option<f64>,
    #[serde(default)]
    total_waste: Option<f64>,
    #[serde(default)]
    description: String,
}

pub async fn production_map_laminatsiya_astatka(
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
            Capability::ApparatusQueueManage,
        ],
    )
    .await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: LaminatsiyaAstatkaRequest = parse_json(&body)?;
    if input.apparatus.trim().is_empty() || input.order_id.trim().is_empty() {
        return Err(bad_request("apparatus and order_id are required"));
    }
    let is_admin = state
        .admin
        .principal_has_capability(&principal, Capability::AdminAccess)
        .await;
    if !is_admin {
        let assigned_apparatus = state.admin.principal_assigned_apparatus(&principal).await;
        if !queue_state::apparatus_matches_assigned(&input.apparatus, &assigned_apparatus) {
            return Err(production_map_error(ProductionMapError::ApparatusNotAssigned));
        }
    }
    let report = state
        .production_maps
        .record_laminatsiya_astatka(
            &input.apparatus,
            &input.order_id,
            queue_action_actor(&principal),
            input.lamination_print_leftover_rolls,
            input.lamination_film_leftover_rolls,
            input.total_waste,
            &input.description,
        )
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "report": report,
    })))
}
