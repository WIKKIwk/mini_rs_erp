//! A sheet-local bootstrap. Its revision covers production state only, not
//! an atomic inventory/Qolip snapshot; Start must still validate every resource.
use super::*;
use crate::core::authz::capability_code;
use crate::core::production_map::{
    ApparatusQueuePreviousWipMode, ApparatusQueueQolipMode, ApparatusQueueStartMaterialsMode,
};
use axum::body::to_bytes;
use axum::extract::rejection::QueryRejection;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const SECTION_BODY_LIMIT: usize = 4 * 1024 * 1024;
const SNAPSHOT_ATTEMPTS: usize = 2;
const BOOTSTRAP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
pub(super) const BOOTSTRAP_CAPABILITIES: &[Capability] = &[
    Capability::AdminAccess,
    Capability::ProductionMapManage,
    Capability::ApparatusQueueRead,
    Capability::RawMaterialAssign,
    Capability::QolipManage,
    Capability::PreparationAccess,
];

#[derive(serde::Deserialize)]
pub struct OrderScanBootstrapQuery {
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    material_barcodes: String,
    #[serde(default = "include_sections_default")]
    include_sections: bool,
    #[serde(default)]
    include_context: bool,
    #[serde(default)]
    qr_payload: String,
}

fn include_sections_default() -> bool {
    true
}

pub async fn production_map_order_scan_bootstrap(
    State(state): State<AppState>,
    query: Result<Query<OrderScanBootstrapQuery>, QueryRejection>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    let result = match query {
        // The canonical snapshot reader retries internally during rebuild
        // churn. Bound the whole read, including those retries and sections.
        Ok(Query(query)) => tokio::time::timeout(
            BOOTSTRAP_TIMEOUT,
            bootstrap(&state, query, method, &headers),
        )
        .await
        .unwrap_or_else(|_| {
            Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(AdminErrorResponse::new("order_scan_bootstrap_timeout")),
            ))
        }),
        Err(_) => Err(bad_request("invalid_order_scan_bootstrap_query")),
    };
    let mut response = match result {
        Ok(response) => response,
        Err(error) => error.into_response(),
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn bootstrap(
    state: &AppState,
    query: OrderScanBootstrapQuery,
    method: Method,
    headers: &HeaderMap,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(state, headers, BOOTSTRAP_CAPABILITIES).await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let apparatus = query.apparatus.trim();
    let order_id = query.order_id.trim();
    if !queue_state::is_canonical_apparatus_id(apparatus) || order_id.is_empty() {
        return Err(bad_request("canonical apparatus and order_id are required"));
    }
    // Training uses a separate per-principal merge/authorization flow. Do not
    // accidentally expose its maps through the ordinary production snapshot.
    if order_id.starts_with("training-") {
        return Err(bad_request("training_order_scan_bootstrap_unsupported"));
    }
    let scope = authorization_scope(state, &principal, apparatus).await?;
    for _ in 0..SNAPSHOT_ATTEMPTS {
        let (snapshot, mut revision) = state
            .production_maps
            .worker_snapshot_shared_with_revision(
                &[apparatus.to_string()], &[order_id.to_string()],
            )
            .await
            .map_err(production_map_error)?;
        let control = snapshot
            .queue_action_controls
            .get(apparatus)
            .and_then(|orders| orders.get(order_id))
            .ok_or_else(|| {
                (
                    StatusCode::NOT_FOUND,
                    Json(AdminErrorResponse::new("order_not_available")),
                )
            })?;
        let needs_materials = control.interaction.start_materials_mode
            == ApparatusQueueStartMaterialsMode::ScanRequired;
        let needs_qolips = control.interaction.qolip_mode == ApparatusQueueQolipMode::ScanRequired;
        let include_prepared_sections = query.include_context && query.include_sections;
        let needs_input_progress = control.interaction.previous_wip_mode
            == ApparatusQueuePreviousWipMode::ScanRequired
            && !control.previous_stage.trim().is_empty();
        // Reuse the existing handlers, including their distinct capability and
        // resource guards. No child request can contain a scanned Qolip code.
        let (materials, qolips, validated_progress, material_assignments, input_progress) = tokio::join!(
            async {
                if !query.include_sections || !needs_materials {
                    return not_required();
                }
                section(
                    raw_material_start_requirements(
                        State(state.clone()),
                        Query(
                            raw_materials::RawMaterialStartRequirementsQuery::for_bootstrap(
                                order_id,
                                apparatus,
                                &query.material_barcodes,
                            ),
                        ),
                        Method::GET,
                        headers.clone(),
                    )
                    .await,
                )
                .await
            },
            async {
                if !query.include_sections || !needs_qolips {
                    return not_required();
                }
                let body = serde_json::json!({
                    "apparatus": apparatus, "order_id": order_id, "qolip_code": "",
                });
                section(
                    production_map_qolip_validate(
                        State(state.clone()),
                        Method::POST,
                        headers.clone(),
                        Bytes::from(body.to_string()),
                    )
                    .await,
                )
                .await
            },
            validate_progress(state, headers, apparatus, order_id, &query.qr_payload),
            async {
                if !include_prepared_sections || needs_materials {
                    return not_required();
                }
                section(
                    raw_material_assignments(
                        State(state.clone()),
                        Query(raw_materials::RawMaterialAssignmentsQuery::for_bootstrap(
                            order_id, apparatus,
                        )),
                        Method::GET,
                        headers.clone(),
                        Bytes::new(),
                    ).await,
                ).await
            },
            async {
                if !include_prepared_sections || !needs_input_progress {
                    return not_required();
                }
                section(
                    production_map_wip_batches(
                        State(state.clone()),
                        Query(wip::WipBatchesQuery::for_scan_bootstrap(order_id)),
                        Method::GET,
                        headers.clone(),
                    ).await,
                ).await
            },
        );
        if validated_progress.is_err() {
            // Domain errors may contain the order title. Check the captured
            // authorization again before returning data from an awaited QR read.
            let current = authorize_any_capability(state, headers, BOOTSTRAP_CAPABILITIES).await?;
            if authorization_scope(state, &current, apparatus).await? != scope {
                return Err(conflict("order_scan_bootstrap_changed"));
            }
        }
        let validated_progress = validated_progress?;
        if state.production_maps.snapshot_revision() != revision {
            // A prepare result includes occupancy/maps and a particular scanned
            // roll. Rebuild all of it rather than relabeling old context with a
            // newer revision, even when only the target control stayed equal.
            if query.include_context || validated_progress.is_some() {
                continue;
            }
            let (latest, latest_revision) = state
                .production_maps
                .worker_snapshot_shared_with_revision(
                    &[apparatus.to_string()],
                    &[order_id.to_string()],
                )
                .await
                .map_err(production_map_error)?;
            // A global revision can change for another apparatus/order. Only
            // reuse the sections when every target control field is unchanged.
            let latest_control = latest
                .queue_action_controls
                .get(apparatus)
                .and_then(|orders| orders.get(order_id));
            let previous = serde_json::to_value((Some(control),
                snapshot.stage_states.get(order_id), snapshot.order_controls.get(order_id)))
                .map_err(|_| conflict("order_scan_bootstrap_changed"))?;
            let current = serde_json::to_value((latest_control,
                latest.stage_states.get(order_id), latest.order_controls.get(order_id)))
                .map_err(|_| conflict("order_scan_bootstrap_changed"))?;
            if previous != current {
                continue;
            }
            revision = latest_revision;
        }
        // Reauthenticate, rather than trusting the captured principal after
        // awaited readers. Never silently re-scope an in-flight request.
        let current = authorize_any_capability(state, headers, BOOTSTRAP_CAPABILITIES).await?;
        let current_scope = authorization_scope(state, &current, apparatus).await?;
        if current_scope != scope {
            return Err(conflict("order_scan_bootstrap_changed"));
        }
        if state.production_maps.snapshot_revision() != revision {
            continue;
        }
        let mut payload = serde_json::json!({
            "ok": true,
            "control_state": {
                "apparatus": apparatus,
                "order_id": order_id,
                "rev": revision,
                "epoch": state.production_maps.snapshot_epoch(),
                // This is endpoint-specific authorization context, never a
                // replacement for the whole worker snapshot/delta cursor.
                "scope": scope,
                "control": control,
                "queue_state": control.state.as_str(),
                "stage_states": snapshot.stage_states.get(order_id).cloned().unwrap_or_default(),
                "order_control": snapshot.order_controls.get(order_id)
                    .map(|record| record.state.as_str()).unwrap_or("active"),
            },
            "sections": { "materials": materials, "qolips": qolips },
        });
        if query.include_context {
            payload["queue_context"] =
                queue_context(state, &snapshot, revision, &scope, apparatus, order_id);
        }
        if include_prepared_sections {
            let scoped_section = |mut section: serde_json::Value| {
                section["apparatus"] = serde_json::json!(apparatus);
                section["order_id"] = serde_json::json!(order_id);
                section["stage_node_id"] = serde_json::json!(control.stage_node_id);
                section
            };
            payload["sections"]["material_assignments"] = scoped_section(material_assignments);
            let mut input_progress = scoped_section(input_progress);
            // This is the complete response to the bounded reader, not a
            // statement that the order has no WIP outside this 500-row window.
            input_progress["limit"] = serde_json::json!(500);
            input_progress["status_filter"] = serde_json::json!("waiting");
            payload["sections"]["input_progress"] = input_progress;
        }
        if let Some(validated_progress) = validated_progress {
            payload["validated_progress"] = validated_progress;
        }
        return Ok(json_response(payload));
    }
    Err(conflict("order_scan_bootstrap_changed"))
}

/// Reuse the ordinary scoped, active-order QR reader and every one of its
/// capability/ownership/route guards. This endpoint never starts a job.
async fn validate_progress(
    state: &AppState,
    headers: &HeaderMap,
    apparatus: &str,
    order_id: &str,
    qr_payload: &str,
) -> Result<Option<serde_json::Value>, AdminError> {
    if qr_payload.trim().is_empty() {
        return Ok(None);
    }
    let body = serde_json::json!({
        "apparatus": apparatus,
        "order_id": order_id,
        "qr_payload": qr_payload.trim(),
        "require_active_order": true,
    });
    let response = production_map_progress_qr_lookup(
        State(state.clone()),
        Method::POST,
        headers.clone(),
        Bytes::from(body.to_string()),
    )
    .await?;
    let bytes = to_bytes(response.into_body(), SECTION_BODY_LIMIT)
        .await
        .map_err(|_| conflict("order_scan_progress_body_limit"))?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| conflict("order_scan_progress_invalid_json"))
}

/// A local scoped sequence view, never a replacement for the worker's bounded
/// queue cursor. Keep the full station context for busy/priority decisions, but
/// send maps and action contracts only for this order and current active jobs.
fn queue_context(
    state: &AppState,
    snapshot: &crate::core::production_map::ProductionMapLiveSnapshot,
    revision: u64,
    scope: &str,
    apparatus: &str,
    order_id: &str,
) -> serde_json::Value {
    let mut selected = BTreeSet::from([order_id.to_string()]);
    if let Some(states) = snapshot.queue_states.get(apparatus) {
        selected.extend(states.iter().filter_map(|(id, state)| {
            queue_state::ApparatusQueueOrderState::parse(state)
                .is_some_and(|state| state.is_active())
                .then_some(id.clone())
        }));
    }
    if let Some(controls) = snapshot.queue_action_controls.get(apparatus) {
        selected.extend(
            controls
                .iter()
                .filter_map(|(id, control)| control.state.is_active().then_some(id.clone())),
        );
    }
    serde_json::json!({
        "ok": true,
        "rev": revision,
        "epoch": state.production_maps.snapshot_epoch(),
        "scope": scope,
        "worker_show_all_apparatus_tabs": state.worker_show_all_apparatus_tabs,
        "maps": snapshot.maps.iter().filter(|saved| selected.contains(&saved.map.id)).collect::<Vec<_>>(),
        "sequences": { apparatus: snapshot.sequences.get(apparatus).cloned().unwrap_or_default() },
        "sequence_versions": { apparatus: snapshot.sequence_versions.get(apparatus) },
        "sequence_revisions": { apparatus: snapshot.sequence_revisions.get(apparatus) },
        "visible_order_ids": { apparatus: snapshot.visible_order_ids.get(apparatus).cloned().unwrap_or_default() },
        "queue_states": { apparatus: snapshot.queue_states.get(apparatus).cloned().unwrap_or_default() },
        "queue_policies": snapshot.queue_policies.iter().filter(|policy| policy.apparatus_id.as_str() == apparatus).collect::<Vec<_>>(),
        "queue_action_controls": { apparatus: snapshot.queue_action_controls.get(apparatus).map(|controls| controls.iter().filter(|(id, _)| selected.contains(*id)).collect::<std::collections::BTreeMap<_, _>>()).unwrap_or_default() },
        "stage_states": snapshot.stage_states.iter().filter(|(id, _)| selected.contains(*id)).collect::<std::collections::BTreeMap<_, _>>(),
        "order_controls": &snapshot.order_controls,
                "stage_order_controls": &snapshot.stage_order_controls,
        "order_statuses": snapshot.order_statuses.iter().filter(|(id, _)| selected.contains(*id)).collect::<std::collections::BTreeMap<_, _>>(),
        "frozen_orders_by_apparatus": { apparatus: snapshot.frozen_orders_by_apparatus.get(apparatus).cloned().unwrap_or_default() },
        "order_customers": {},
    })
}

pub(super) async fn authorization_scope(
    state: &AppState,
    principal: &Principal,
    apparatus: &str,
) -> Result<String, AdminError> {
    let mut capabilities = state.admin.principal_capability_codes(principal).await;
    capabilities.sort();
    capabilities.dedup();
    let has = |capability| {
        capability_code(capability).is_some_and(|code| capabilities.iter().any(|item| item == code))
    };
    if !BOOTSTRAP_CAPABILITIES
        .iter()
        .any(|capability| has(*capability))
    {
        return Err(forbidden());
    }
    let mut assigned = state.admin.principal_assigned_apparatus(principal).await;
    assigned.sort();
    assigned.dedup();
    if !has(Capability::AdminAccess)
        && !queue_state::apparatus_matches_assigned(apparatus, &assigned)
    {
        return Err(forbidden());
    }
    let mut digest = Sha256::new();
    digest.update(b"order-scan-bootstrap-v1\0");
    digest.update(format!("{:?}\0{}\0", principal.role, principal.ref_));
    digest.update(apparatus.as_bytes());
    digest.update([0]);
    for values in [&capabilities, &assigned] {
        for value in values {
            digest.update(value.as_bytes());
            digest.update([0]);
        }
        digest.update([0]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn not_required() -> serde_json::Value {
    serde_json::json!({ "status": "not_required" })
}

async fn section(result: Result<Response, AdminError>) -> serde_json::Value {
    let response = match result {
        Ok(response) => response,
        Err((status, Json(error))) => {
            return serde_json::json!({
                "status": "error", "status_code": status.as_u16(), "error": error,
            });
        }
    };
    let status = response.status();
    let body = match to_bytes(response.into_body(), SECTION_BODY_LIMIT).await {
        Ok(body) => body,
        Err(_) => return section_read_error("order_scan_section_body_limit"),
    };
    let data: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(data) => data,
        Err(_) => return section_read_error("order_scan_section_invalid_json"),
    };
    if status.is_success() {
        serde_json::json!({ "status": "ready", "data": data })
    } else {
        serde_json::json!({ "status": "error", "status_code": status.as_u16(), "error": data })
    }
}

fn section_read_error(error: &str) -> serde_json::Value {
    serde_json::json!({ "status": "error", "status_code": 500, "error": { "error": error } })
}
