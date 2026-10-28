use super::*;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LaminationSettingsRequest {
    layer_remainders_required: bool,
}

pub async fn production_map_lamination_settings(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let settings = match method {
        Method::GET => {
            authorize_any_capability(
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
            state.production_maps.lamination_settings().await
        }
        Method::PUT => {
            let principal = authorize_capability(&state, &headers, Capability::AdminAccess).await?;
            if principal.role != PrincipalRole::Admin {
                return Err(forbidden());
            }
            let input: LaminationSettingsRequest = parse_json(&body)?;
            state
                .production_maps
                .update_lamination_settings(
                    input.layer_remainders_required,
                    &queue_action_actor(&principal),
                )
                .await
        }
        _ => return Err(method_not_allowed()),
    }
    .map_err(production_map_error)?;
    Ok(json_response(
        serde_json::json!({"ok": true, "settings": settings}),
    ))
}
