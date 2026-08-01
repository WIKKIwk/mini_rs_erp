use std::collections::{BTreeMap, BTreeSet};

use axum::extract::Query;
use serde::Deserialize;

use crate::core::qolip::{QolipError, QolipOrderNote};

use super::queue_actions::qolip_queue_error;
use super::*;

#[derive(Debug, Default, Deserialize)]
pub struct QolipOrderNoteQuery {
    #[serde(default)]
    order_id: String,
}

#[derive(Debug, Default, Deserialize)]
struct QolipOrderNoteRequest {
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    qolip_codes: Vec<String>,
}

pub async fn production_map_qolip_order_notes(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<QolipOrderNoteQuery>,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::QolipManage],
    )
    .await?;

    match method {
        Method::GET => {
            let order_id = query.order_id.trim();
            if order_id.is_empty() {
                let notes = state
                    .qolip
                    .order_notes(&principal)
                    .await
                    .map_err(qolip_queue_error)?;
                return Ok(json_response(serde_json::json!({
                    "ok": true,
                    "notes": notes,
                })));
            }

            let (map, required_qolips) = load_order_qolips(&state, order_id).await?;
            let in_use_codes = state
                .qolip
                .order_note_qolip_codes_in_use(&principal, order_id)
                .await
                .map_err(qolip_queue_error)?
                .into_iter()
                .map(|code| code.trim().to_ascii_lowercase())
                .filter(|code| !code.is_empty())
                .collect::<BTreeSet<_>>();
            let note = state
                .qolip
                .order_note(&principal, order_id)
                .await
                .map_err(qolip_queue_error)?;
            Ok(json_response(serde_json::json!({
                "ok": true,
                "order_id": order_id,
                "item_code": map.product_code,
                "item_name": map.title,
                "required_qolips": required_qolips
                    .iter()
                    .map(|spec| required_qolip_payload(spec, &in_use_codes))
                    .collect::<Vec<_>>(),
                "note": note,
            })))
        }
        Method::POST => {
            let input: QolipOrderNoteRequest = parse_json(&body)?;
            let order_id = input.order_id.trim();
            if order_id.is_empty() {
                return Err(bad_request("order_id is required"));
            }
            let (map, required_qolips) = load_order_qolips(&state, order_id).await?;
            let status = input.status.trim().to_ascii_lowercase();
            match status.as_str() {
                "given" => {
                    let qolip_codes = canonical_qolip_codes(&input.qolip_codes, &required_qolips)?;
                    if qolip_codes.is_empty() {
                        return Err(bad_request("qolip_code_required"));
                    }
                    let note = state
                        .qolip
                        .save_order_note(
                            QolipOrderNote {
                                order_id: order_id.to_string(),
                                item_code: map.product_code.clone(),
                                item_name: map.title.clone(),
                                qolip_codes,
                                status,
                                updated_at: String::new(),
                            },
                            &principal,
                        )
                        .await
                        .map_err(order_note_save_error)?;
                    Ok(json_response(serde_json::json!({
                        "ok": true,
                        "note": note,
                    })))
                }
                "returned" => {
                    let Some(existing) = state
                        .qolip
                        .order_note(&principal, order_id)
                        .await
                        .map_err(qolip_queue_error)?
                    else {
                        return Err(bad_request("qolip_order_note_not_found"));
                    };
                    let note = state
                        .qolip
                        .save_order_note(QolipOrderNote { status, ..existing }, &principal)
                        .await
                        .map_err(qolip_queue_error)?;
                    Ok(json_response(serde_json::json!({
                        "ok": true,
                        "note": note,
                    })))
                }
                _ => Err(bad_request("qolip_order_note_status_invalid")),
            }
        }
        _ => Err(method_not_allowed()),
    }
}

async fn load_order_qolips(
    state: &AppState,
    order_id: &str,
) -> Result<
    (
        crate::core::production_map::ProductionMapDefinition,
        Vec<crate::core::qolip::QolipProductSpec>,
    ),
    AdminError,
> {
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
    Ok((map, required_qolips))
}

fn canonical_qolip_codes(
    requested: &[String],
    required: &[crate::core::qolip::QolipProductSpec],
) -> Result<Vec<String>, AdminError> {
    let required_by_key = required
        .iter()
        .map(|spec| {
            (
                spec.qolip_code.trim().to_ascii_lowercase(),
                spec.qolip_code.trim().to_string(),
            )
        })
        .filter(|(key, code)| !key.is_empty() && !code.is_empty())
        .collect::<BTreeMap<_, _>>();
    let mut result = Vec::new();
    for code in requested {
        let key = code.trim().to_ascii_lowercase();
        if key.is_empty() {
            continue;
        }
        let Some(canonical) = required_by_key.get(&key) else {
            return Err(bad_request("qolip_code_mismatch"));
        };
        if !result.iter().any(|saved: &String| saved == canonical) {
            result.push(canonical.clone());
        }
    }
    result.sort_by_key(|code| code.to_ascii_lowercase());
    Ok(result)
}

fn required_qolip_payload(
    spec: &crate::core::qolip::QolipProductSpec,
    in_use_codes: &BTreeSet<String>,
) -> serde_json::Value {
    let qolip_code = spec.qolip_code.trim();
    serde_json::json!({
        "qolip_code": spec.qolip_code.as_str(),
        "color": spec.color.as_str(),
        "size": spec.size,
        "in_use": in_use_codes.contains(&qolip_code.to_ascii_lowercase()),
    })
}

fn order_note_save_error(error: QolipError) -> AdminError {
    match error {
        QolipError::QolipInUse => bad_request("qolip_order_note_in_use"),
        other => qolip_queue_error(other),
    }
}
