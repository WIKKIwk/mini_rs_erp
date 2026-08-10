use super::queue_actions::{apparatus_requires_qolip_scan, qolip_queue_error, reject_qolip_in_use};
use super::*;

#[derive(serde::Deserialize)]
struct QolipStartValidationRequest {
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    qolip_code: String,
}

pub async fn production_map_qolip_validate(
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
    let input: QolipStartValidationRequest = parse_json(&body)?;
    let apparatus = input.apparatus.trim();
    let order_id = input.order_id.trim();
    if apparatus.is_empty() || order_id.is_empty() {
        return Err(bad_request("apparatus and order_id are required"));
    }
    if !apparatus_requires_qolip_scan(apparatus) {
        return Err(bad_request("qolip_scan_not_required"));
    }
    let is_admin = state
        .admin
        .principal_has_capability(&principal, Capability::AdminAccess)
        .await;
    let assigned_apparatus = state.admin.principal_assigned_apparatus(&principal).await;
    if !is_admin && !queue_state::apparatus_matches_assigned(apparatus, &assigned_apparatus) {
        return Err(bad_request("apparatus_not_assigned"));
    }
    if order_id.starts_with("training-") {
        let Some(training_map) = super::super::training::training_map_for_principal(
            &state,
            &principal,
            order_id,
            apparatus,
        )
        .await
        .map_err(super::super::training::training_workspace_error)?
        else {
            return Err(production_map_error(ProductionMapError::MapNotFound));
        };
        let required_qolips = state
            .qolip
            .required_qolips_for_order(
                &training_map.map.product_code,
                &training_map.map.title,
            )
            .await
            .map_err(qolip_queue_error)?;
        let required_qolip_codes = required_qolips
            .iter()
            .map(|spec| spec.qolip_code.trim().to_string())
            .collect::<Vec<_>>();
        let required_qolip_count = required_qolips.len();
        let required_qolips_payload = required_qolips
            .iter()
            .map(required_qolip_payload)
            .collect::<Vec<_>>();
        if input.qolip_code.trim().is_empty() {
            return Ok(json_response(serde_json::json!({
                "ok": true,
                "qolip": {
                    "qolip_code": "",
                    "required_qolip_codes": required_qolip_codes,
                    "required_qolip_count": required_qolip_count,
                    "required_qolips": required_qolips_payload,
                }
            })));
        }
        let spec = state
            .qolip
            .product_spec_by_qolip_code(&input.qolip_code)
            .await
            .map_err(qolip_queue_error)?
            .ok_or_else(|| {
                qolip_queue_error(crate::core::qolip::QolipError::QolipCodeNotFound)
            })?;
        if !required_qolips.iter().any(|required| {
            required
                .qolip_code
                .trim()
                .eq_ignore_ascii_case(&spec.qolip_code)
        }) {
            return Err(qolip_queue_error(
                crate::core::qolip::QolipError::QolipCodeMismatch,
            ));
        }
        return Ok(json_response(serde_json::json!({
            "ok": true,
            "qolip": {
                "qolip_code": spec.qolip_code,
                "color": spec.color,
                "required_qolip_codes": required_qolip_codes,
                "required_qolip_count": required_qolip_count,
                "required_qolips": required_qolips_payload,
            }
        })));
    }
    let Some(map) = state
        .production_maps
        .raw_map(order_id)
        .await
        .map_err(production_map_error)?
    else {
        return Err(production_map_error(ProductionMapError::MapNotFound));
    };
    let required_qolips = state
        .qolip
        .required_qolips_for_order(&map.product_code, &map.title)
        .await
        .map_err(qolip_queue_error)?;
    let required_qolip_codes = required_qolips
        .iter()
        .map(|spec| spec.qolip_code.trim().to_string())
        .collect::<Vec<_>>();
    let required_qolip_count = required_qolips.len();
    let required_qolips_payload = required_qolips
        .iter()
        .map(required_qolip_payload)
        .collect::<Vec<_>>();
    if input.qolip_code.trim().is_empty() {
        return Ok(json_response(serde_json::json!({
            "ok": true,
            "qolip": {
                "qolip_code": "",
                "required_qolip_codes": required_qolip_codes,
                "required_qolip_count": required_qolip_count,
                "required_qolips": required_qolips_payload,
            }
        })));
    }
    reject_qolip_in_use(&state, apparatus, order_id, &input.qolip_code).await?;
    let preparation = state
        .qolip
        .prepare_qolip_code_for_order_start(
            &input.qolip_code,
            &map.product_code,
            &map.title,
            &principal.ref_,
            &principal.display_name,
            &principal,
        )
        .await
        .map_err(qolip_queue_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "qolip": {
            "qolip_code": preparation.spec.qolip_code,
            "color": preparation.spec.color,
            "required_qolip_codes": required_qolip_codes,
            "required_qolip_count": required_qolip_count,
            "required_qolips": required_qolips_payload,
        }
    })))
}

fn required_qolip_payload(spec: &crate::core::qolip::QolipProductSpec) -> serde_json::Value {
    serde_json::json!({
        "qolip_code": spec.qolip_code.as_str(),
        "color": spec.color.as_str(),
    })
}
