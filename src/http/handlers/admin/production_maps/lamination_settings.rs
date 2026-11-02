use super::*;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LaminationSettingsRequest {
    layer_remainders_required: Option<bool>,
    print_wip_remainder_labels: Option<bool>,
    print_film_remainder_labels: Option<bool>,
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
            if !principal.role.is_admin_workspace_manager() {
                return Err(forbidden());
            }
            let input: LaminationSettingsRequest = parse_json(&body)?;
            let legacy = input.layer_remainders_required;
            if legacy.is_some()
                && (input.print_wip_remainder_labels.is_some()
                    || input.print_film_remainder_labels.is_some())
                || legacy.is_none()
                    && input.print_wip_remainder_labels.is_none()
                    && input.print_film_remainder_labels.is_none()
            {
                return Err(bad_request("lamination_label_settings_required"));
            }
            state
                .production_maps
                .update_lamination_label_settings(
                    crate::core::production_map::LaminationLabelSettingsPatch {
                        print_wip_remainder_labels: input.print_wip_remainder_labels.or(legacy),
                        print_film_remainder_labels: input.print_film_remainder_labels.or(legacy),
                    },
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
