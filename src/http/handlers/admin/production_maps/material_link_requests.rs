use std::collections::BTreeMap;

use super::raw_material_details::assigned_apparatus_contains;
use super::*;
use crate::db::postgres_production_map::material_link_requests::{
    LinkCandidate, LinkError, LinkRequest, MaterialLinkStore,
};

#[derive(Default, serde::Deserialize)]
pub struct MaterialLinkQuery {
    #[serde(default)]
    pub request_id: String,
    #[serde(default)]
    pub order_id: String,
    #[serde(default)]
    pub apparatus: String,
}

#[derive(Default, serde::Deserialize)]
struct MaterialLinkCommand {
    #[serde(default)]
    request_id: String,
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    action: String,
    #[serde(default)]
    barcodes: Vec<String>,
}

fn link_error(error: LinkError) -> AdminError {
    match error {
        LinkError::Forbidden => forbidden(),
        LinkError::NotFound => not_found("material_link_not_found"),
        LinkError::Selection => bad_request("material_link_selection_required"),
        LinkError::Production(error) => production_map_error(error),
        error => {
            tracing::error!(%error, "material link request failed");
            server_error("material_link_store_failed")
        }
    }
}

fn store(state: &AppState) -> Result<&MaterialLinkStore, AdminError> {
    state
        .material_link_requests
        .as_ref()
        .ok_or_else(|| server_error("material_link_unavailable"))
}

async fn worker_scope(
    state: &AppState,
    principal: &Principal,
    order: &str,
    apparatus: &str,
) -> Result<(), AdminError> {
    if principal.role != PrincipalRole::Aparatchi {
        return Err(forbidden());
    }
    require_capability(state, principal, Capability::ApparatusQueueManage).await?;
    let assigned = state.admin.principal_assigned_apparatus(principal).await;
    if order.trim().is_empty()
        || apparatus.trim().is_empty()
        || !assigned_apparatus_contains(apparatus, &assigned)
    {
        return Err(forbidden());
    }
    if !state
        .production_maps
        .raw_material_assignment_orders()
        .await
        .map_err(production_map_error)?
        .iter()
        .any(|saved| saved.map.id == order.trim())
    {
        return Err(bad_request("raw_material_order_not_active"));
    }
    Ok(())
}

async fn eligible_candidates(
    state: &AppState,
    order: &str,
    apparatus: &str,
) -> Result<Vec<LinkCandidate>, AdminError> {
    let mut result = Vec::new();
    for candidate in store(state)?
        .candidates(apparatus)
        .await
        .map_err(link_error)?
    {
        let mover = Principal {
            role: PrincipalRole::MaterialTaminotchi,
            ref_: candidate.mover_ref.clone(),
            display_name: candidate.mover_display_name.clone(),
            legal_name: String::new(),
            phone: String::new(),
            avatar_url: String::new(),
        };
        match super::raw_material_details::fill_raw_material_assignment_input(
            state,
            &mover,
            RawMaterialAssignmentInput {
                order_id: order.into(),
                apparatus: apparatus.into(),
                barcode: candidate.barcode.clone(),
                ..Default::default()
            },
        )
        .await
        {
            Ok(_) => result.push(candidate),
            Err(error) if error.0.is_server_error() => return Err(error),
            Err(_) => {} // Wrong width, family, warehouse or apparatus: not an approvable candidate.
        }
    }
    Ok(result)
}

pub async fn material_link_requests(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<MaterialLinkQuery>,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::ApparatusQueueManage,
            Capability::RawMaterialAssign,
            Capability::AdminAccess,
        ],
    )
    .await?;
    let store = store(&state)?;
    match method {
        Method::GET if !query.request_id.is_empty() => {
            let request = store.get(&query.request_id).await.map_err(link_error)?;
            if !request.can_read(&principal) {
                return Err(forbidden());
            }
            Ok(json_response(
                serde_json::json!({"can_decide":request.can_decide(&principal),"request":request}),
            ))
        }
        Method::GET => {
            // Terminal requests remain readable even after an order closes, so the UI can leave pending.
            if principal.role != PrincipalRole::Aparatchi {
                return Err(forbidden());
            }
            let assigned = state.admin.principal_assigned_apparatus(&principal).await;
            if !assigned_apparatus_contains(&query.apparatus, &assigned) {
                return Err(forbidden());
            }
            let requests = store
                .list(&query.order_id, &query.apparatus, &principal.ref_)
                .await
                .map_err(link_error)?;
            let candidates =
                match worker_scope(&state, &principal, &query.order_id, &query.apparatus).await {
                    Ok(()) => {
                        eligible_candidates(&state, &query.order_id, &query.apparatus).await?
                    }
                    Err(error) if error.0.is_server_error() => return Err(error),
                    Err(_) => Vec::new(),
                };
            Ok(json_response(
                serde_json::json!({"requests":requests,"available_count":candidates.len()}),
            ))
        }
        Method::POST => {
            let command: MaterialLinkCommand = parse_json(&body)?;
            if command.action == "create" {
                worker_scope(&state, &principal, &command.order_id, &command.apparatus).await?;
                let map = state
                    .production_maps
                    .raw_map(&command.order_id)
                    .await
                    .map_err(production_map_error)?
                    .ok_or_else(|| bad_request("map_not_found"))?;
                let apparatus =
                    super::queue_actions::resolve_queue_apparatus(&state, &command.apparatus)
                        .await?;
                let mut groups: BTreeMap<String, Vec<LinkCandidate>> = BTreeMap::new();
                for candidate in
                    eligible_candidates(&state, &command.order_id, &command.apparatus).await?
                {
                    groups
                        .entry(candidate.mover_ref.clone())
                        .or_default()
                        .push(candidate);
                }
                if groups.is_empty() {
                    return Err(bad_request("material_link_no_candidates"));
                }
                let mut requests = Vec::new();
                for (mover_ref, candidates) in groups {
                    let now = time::OffsetDateTime::now_utc().unix_timestamp();
                    let request = LinkRequest {
                        request_id: format!("material-link-{:032x}", rand::random::<u128>()),
                        status: "pending".into(),
                        event_sequence: 1,
                        order_id: command.order_id.trim().into(),
                        order_number: map.order_number.clone(),
                        order_title: map.title.clone(),
                        apparatus_id: apparatus.id.to_string(),
                        apparatus_name: apparatus.display_name.clone(),
                        requester_ref: principal.ref_.clone(),
                        requester_display_name: principal.display_name.clone(),
                        mover_ref,
                        mover_display_name: candidates[0].mover_display_name.clone(),
                        candidates,
                        selected_barcodes: Vec::new(),
                        decided_by_name: String::new(),
                        decided_by_role: String::new(),
                        decided_at_unix: 0,
                        reason: String::new(),
                        requested_at_unix: now,
                        expires_at_unix: now + 1800,
                    };
                    requests.push(store.create(request).await.map_err(link_error)?);
                }
                return Ok(json_response(serde_json::json!({"requests":requests})));
            }
            let request = store.get(&command.request_id).await.map_err(link_error)?;
            if command.action == "cancel" {
                let request = store
                    .cancel(&command.request_id, &principal)
                    .await
                    .map_err(link_error)?;
                return Ok(json_response(serde_json::json!({"request":request})));
            }
            if !request.can_decide(&principal) {
                return Err(forbidden());
            }
            require_capability(&state, &principal, Capability::RawMaterialAssign).await?;
            if !matches!(command.action.as_str(), "approve" | "reject") {
                return Err(bad_request("material_link_action_invalid"));
            }
            if request.status != "pending" {
                return Ok(json_response(serde_json::json!({"request":request})));
            }
            let _guard = state.production_maps.queue_action_guard().await;
            let mut assignments = Vec::new();
            if command.action == "approve" {
                request
                    .validate_selection(&command.barcodes)
                    .map_err(link_error)?;
                for barcode in &command.barcodes {
                    let prepared = async {
                        let (input, _) =
                            super::raw_material_details::fill_raw_material_assignment_input(
                                &state,
                                &principal,
                                RawMaterialAssignmentInput {
                                    order_id: request.order_id.clone(),
                                    apparatus: request.apparatus_id.clone(),
                                    barcode: barcode.clone(),
                                    ..Default::default()
                                },
                            )
                            .await?;
                        state
                            .production_maps
                            .prepare_raw_material_assignment(input, &queue_action_actor(&principal))
                            .await
                            .map_err(production_map_error)
                    }
                    .await;
                    match prepared {
                        Ok(assignment) => assignments.push(assignment),
                        Err(error) if error.0.is_server_error() => return Err(error),
                        Err(_) => {
                            let request = store.invalidate(&request.request_id,
                                "Tanlangan rulonning holati yoki ulash shartlari o‘zgardi. So‘rovni yangilang.").await.map_err(link_error)?;
                            return Ok(json_response(serde_json::json!({"request":request})));
                        }
                    }
                }
            }
            let result = store
                .decide(
                    &request.request_id,
                    &principal,
                    command.action == "approve",
                    assignments,
                )
                .await;
            let request = match result {
                Ok(request) => request,
                Err(LinkError::Production(
                    ProductionMapError::RawMaterialAlreadyAssigned
                    | ProductionMapError::RawMaterialStockUnavailable,
                )) => store.get(&request.request_id).await.map_err(link_error)?,
                Err(error) => return Err(link_error(error)),
            };
            if request.status == "approved" {
                state.production_maps.notify_live();
            }
            Ok(json_response(serde_json::json!({"request":request})))
        }
        _ => Err(method_not_allowed()),
    }
}
