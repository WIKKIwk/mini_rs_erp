use super::*;
use crate::core::production_map::PaddonTransferInput;

pub async fn production_map_paddon_items_transfer(
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
        &[Capability::AdminAccess, Capability::ApparatusQueueManage],
    )
    .await?;
    if principal.role != PrincipalRole::Admin
        && (principal.role != PrincipalRole::Aparatchi
            || super::paddon_print_lock::assigned_cut_apparatuses(&state, &principal)
                .await?
                .is_empty())
    {
        return Err(forbidden());
    }
    let input: PaddonTransferInput = parse_json(&body)?;
    super::paddons::require_paddon_visible(&state, &principal, &input.source_code).await?;
    super::paddons::require_paddon_visible(&state, &principal, &input.target_code).await?;
    let result = state
        .production_maps
        .transfer_paddon_items(input, &queue_action_actor(&principal))
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "source_code": result.source_code,
        "target_code": result.target_code,
        "progress_batch_ids": result.progress_batch_ids,
    })))
}
