use super::*;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ManagementSettingsRequest {
    free_movement_enabled: bool,
}

pub async fn production_map_paddon_management_settings(
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
                    Capability::ApparatusQueueRead,
                    Capability::ApparatusQueueManage,
                ],
            )
            .await?;
            state.production_maps.paddon_management_settings().await
        }
        Method::PUT => {
            let principal =
                authorize_any_capability(&state, &headers, &[Capability::AdminAccess]).await?;
            if principal.role != PrincipalRole::Admin {
                return Err(forbidden());
            }
            let input: ManagementSettingsRequest = parse_json(&body)?;
            state
                .production_maps
                .update_paddon_management_settings(
                    input.free_movement_enabled,
                    &queue_action_actor(&principal),
                )
                .await
        }
        _ => return Err(method_not_allowed()),
    }
    .map_err(production_map_error)?;
    Ok(json_response(
        serde_json::json!({"ok":true, "settings":settings}),
    ))
}
