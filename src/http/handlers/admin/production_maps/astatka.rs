use super::*;

use crate::core::production_map::queue_state;

#[derive(serde::Deserialize)]
struct BosmaAstatkaRequest {
    apparatus: String,
    order_id: String,
    total_waste: f64,
    finished_goods_meter: Option<f64>,
    finished_goods_kg: Option<f64>,
    #[serde(alias = "babina_kg")]
    bobina_kg: Option<f64>,
    #[serde(default)]
    returned_paint_items: Vec<crate::core::returned_paint::ReturnedPaintItem>,
    #[serde(default)]
    returned_paint_image_id: String,
    #[serde(default)]
    description: String,
}

pub async fn production_map_bosma_astatka(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(&state, &headers, &[
        Capability::AdminAccess, Capability::ProductionMapManage, Capability::ApparatusQueueManage,
    ]).await?;
    if method != Method::POST { return Err(method_not_allowed()); }
    let input: BosmaAstatkaRequest = parse_json(&body)?;
    let apparatus = input.apparatus.trim().to_string();
    let order_id = input.order_id.trim().to_string();
    if !state.admin.principal_has_capability(&principal, Capability::AdminAccess).await {
        let assigned = state.admin.principal_assigned_apparatus(&principal).await;
        if !queue_state::apparatus_matches_assigned(&apparatus, &assigned) {
            return Err(production_map_error(ProductionMapError::ApparatusNotAssigned));
        }
    }
    let map = state.production_maps.raw_map(&order_id).await.map_err(production_map_error)?
        .ok_or_else(|| production_map_error(ProductionMapError::MapNotFound))?;
    // Validate paint values and image ownership without creating a stock/queue action.
    let returned_paint = state.returned_paint.prepare_request(
        crate::core::returned_paint::ReturnedPaintRequestCreate {
            order_id: order_id.clone(), order_code: map.code, order_name: map.title,
            apparatus: apparatus.clone(), image_id: input.returned_paint_image_id,
            items: input.returned_paint_items,
        }, &principal, "bosma-astatka".to_string(),
    ).await.map_err(|error| bad_request(error.to_string()))?;
    let report = state.production_maps.record_bosma_astatka(
        crate::core::production_map::BosmaAstatkaReport {
            report_id: String::new(), order_id, apparatus, from_at_unix: 0, to_at_unix: 0,
            total_waste: input.total_waste, finished_goods_meter: input.finished_goods_meter,
            finished_goods_kg: input.finished_goods_kg, bobina_kg: input.bobina_kg,
            returned_paint, description: input.description,
        },
    ).await.map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({"ok": true, "report": report})))
}

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
    finished_goods_meter: Option<f64>,
    #[serde(default)]
    finished_goods_kg: Option<f64>,
    #[serde(default, alias = "babina_kg")]
    bobina_kg: Option<f64>,
    #[serde(default)]
    description: String,
}

#[derive(Default, serde::Deserialize)]
struct RezkaAstatkaRequest {
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    total_waste: Option<f64>,
    #[serde(default)]
    rezka_bosma_waste: Option<f64>,
    #[serde(default)]
    rezka_lamination_waste: Option<f64>,
    #[serde(default)]
    rezka_edge_waste: Option<f64>,
    #[serde(default)]
    finished_goods_meter: Option<f64>,
    #[serde(default)]
    finished_goods_kg: Option<f64>,
    #[serde(default, alias = "babina_kg")]
    bobina_kg: Option<f64>,
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
            return Err(production_map_error(
                ProductionMapError::ApparatusNotAssigned,
            ));
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
            input.finished_goods_meter,
            input.finished_goods_kg,
            input.bobina_kg,
            &input.description,
        )
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "report": report,
    })))
}

pub async fn production_map_rezka_astatka(
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
    let input: RezkaAstatkaRequest = parse_json(&body)?;
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
            return Err(production_map_error(
                ProductionMapError::ApparatusNotAssigned,
            ));
        }
    }
    let report = state
        .production_maps
        .record_rezka_astatka(
            &input.apparatus,
            &input.order_id,
            queue_action_actor(&principal),
            input.total_waste,
            input.rezka_bosma_waste,
            input.rezka_lamination_waste,
            input.rezka_edge_waste,
            input.finished_goods_meter,
            input.finished_goods_kg,
            input.bobina_kg,
            &input.description,
        )
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "report": report,
    })))
}
