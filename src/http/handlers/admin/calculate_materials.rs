use super::*;
use crate::core::calculate_materials::{CalculateMaterialError, CalculateMaterialUpsert};

pub async fn calculate_materials(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    match method {
        Method::GET => {
            let materials = state
                .calculate_materials
                .list()
                .await
                .map_err(calculate_material_store_error)?;
            Ok(json_response(serde_json::json!({
                "ok": true,
                "materials": materials,
            })))
        }
        Method::PUT => {
            require_capability(&state, &principal, Capability::AdminAccess).await?;
            let input: CalculateMaterialUpsert = parse_json(&body)?;
            let material = state
                .calculate_materials
                .upsert(input)
                .await
                .map_err(calculate_material_store_error)?;
            Ok(json_response(serde_json::json!({
                "ok": true,
                "material": material,
            })))
        }
        _ => Err(method_not_allowed()),
    }
}

fn calculate_material_store_error(error: CalculateMaterialError) -> AdminError {
    match error {
        CalculateMaterialError::InvalidInput(message) => bad_request(message),
        CalculateMaterialError::StoreFailed => server_error("calculate materials store failed"),
    }
}
