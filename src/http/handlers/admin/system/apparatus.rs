use super::*;
use crate::core::apparatus_standard::{
    AAS_METAMODEL_VERSION, AASX_PART_5_VERSION, ApparatusId, CANONICAL_APPARATUS_SCHEMA_VERSION,
    CanonicalApparatusDraft, CanonicalApparatusPatch, IDTA_RELEASE,
};

pub async fn apparatus_options(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::CatalogItemRead,
            Capability::ApparatusQueueRead,
            Capability::RawMaterialAssign,
            Capability::QolipManage,
        ],
    )
    .await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    require_capability(&state, &principal, Capability::ProductionMapManage).await?;
    Ok(json_response(serde_json::json!({
        "contract": "canonical_apparatus_revision",
        "schema_version": CANONICAL_APPARATUS_SCHEMA_VERSION,
        "aas_profile": {
            "idta_release": IDTA_RELEASE,
            "aas_metamodel_version": AAS_METAMODEL_VERSION,
            "aasx_part_5_version": AASX_PART_5_VERSION,
        },
        "vocabulary": {
            "equipment_capabilities": [
                "print", "laminate", "cut", "package", "glue", "tooling",
                "virtual_task", "training"
            ],
            "execution_operations": ["print", "laminate", "cut", "package", "glue"],
            "process_technologies": [
                "rotogravure", "flexographic", "adhesive_lamination",
                "extrusion_lamination", "slitting", "bag_making", "cold_glue"
            ],
            "queue_disciplines": ["strict_sequence", "free_pick"],
            "material_policy_modes": ["not_required", "all_required", "requirement_sets"],
            "tooling_policy_modes": ["not_required", "qolip_scan_required"],
            "virtual_task_policies": ["disabled", "input_bridge"],
            "lifecycle_states": ["active", "retired"]
        }
    })))
}

pub async fn apparatus(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<ItemQuery>,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_apparatus(&state, &headers).await?;
    match method {
        Method::GET => {
            let limit = optional_search_limit(query.limit.as_deref(), 50, 500);
            let search = query.q.as_deref().unwrap_or("").trim().to_lowercase();
            let mut rows = state
                .apparatus
                .list_runtime_projections()
                .await
                .map_err(canonical_apparatus_error)?;
            if !search.is_empty() {
                rows.retain(|row| row.display.display_name.to_lowercase().contains(&search));
            }
            rows.truncate(limit);
            Ok(json_response(rows))
        }
        Method::POST => {
            require_capability(&state, &principal, Capability::ProductionMapManage).await?;
            let draft: CanonicalApparatusDraft = parse_json(&body)?;
            let committed = state
                .apparatus
                .create(draft, canonical_command_metadata(&principal, &headers)?)
                .await
                .map_err(canonical_apparatus_error)?;
            state.production_maps.notify_live();
            Ok(json_response(committed))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn apparatus_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_apparatus(&state, &headers).await?;
    let apparatus_id = parse_apparatus_id(id)?;
    match method {
        Method::GET => state
            .apparatus
            .current_projection(&apparatus_id)
            .await
            .map_err(canonical_apparatus_error)?
            .map(|projection| json_response(projection.as_ref().clone()))
            .ok_or_else(|| not_found("apparatus_not_found")),
        Method::PUT => {
            require_capability(&state, &principal, Capability::ProductionMapManage).await?;
            let request: CanonicalUpdateRequest = parse_json(&body)?;
            let committed = state
                .apparatus
                .update(
                    apparatus_id,
                    request.expected_revision,
                    request.draft,
                    canonical_command_metadata(&principal, &headers)?,
                )
                .await
                .map_err(canonical_apparatus_error)?;
            state.production_maps.notify_live();
            Ok(json_response(committed))
        }
        Method::PATCH => {
            require_capability(&state, &principal, Capability::ProductionMapManage).await?;
            let request: CanonicalPatchRequest = parse_json(&body)?;
            let committed = state
                .apparatus
                .patch(
                    apparatus_id,
                    request.expected_revision,
                    request.patch,
                    canonical_command_metadata(&principal, &headers)?,
                )
                .await
                .map_err(canonical_apparatus_error)?;
            state.production_maps.notify_live();
            Ok(json_response(committed))
        }
        Method::DELETE => {
            require_capability(&state, &principal, Capability::ProductionMapManage).await?;
            let request: CanonicalRetireRequest = parse_json(&body)?;
            let committed = state
                .apparatus
                .retire(
                    apparatus_id,
                    request.expected_revision,
                    request.retirement_reason,
                    canonical_command_metadata(&principal, &headers)?,
                )
                .await
                .map_err(canonical_apparatus_error)?;
            state.production_maps.notify_live();
            Ok(json_response(committed))
        }
        _ => Err(method_not_allowed()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CanonicalUpdateRequest {
    expected_revision: u64,
    draft: CanonicalApparatusDraft,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CanonicalPatchRequest {
    expected_revision: u64,
    patch: CanonicalApparatusPatch,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CanonicalRetireRequest {
    expected_revision: u64,
    retirement_reason: String,
}

pub(super) async fn authorize_apparatus(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, AdminError> {
    authorize_any_capability(
        state,
        headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::CatalogItemRead,
            Capability::ApparatusQueueRead,
            Capability::RawMaterialAssign,
            Capability::QolipManage,
        ],
    )
    .await
}

pub(super) fn parse_apparatus_id(id: String) -> Result<ApparatusId, AdminError> {
    ApparatusId::new(id).map_err(|_| bad_request("apparatus_id_invalid"))
}
