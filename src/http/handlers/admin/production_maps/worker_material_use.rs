use super::*;
use crate::core::apparatus_standard::ApparatusId;

#[derive(serde::Deserialize)]
pub struct WorkerMaterialQuery {
    order_id: String,
    apparatus: String,
}

#[derive(serde::Deserialize)]
struct WorkerMaterialCommand {
    #[serde(flatten)]
    scope: WorkerMaterialQuery,
    barcode: String,
    placement_version: i64,
}

pub async fn worker_material_use(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<std::collections::HashMap<String, String>>,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal =
        authorize_capability(&state, &headers, Capability::ApparatusQueueManage).await?;
    if !matches!(method, Method::GET | Method::POST) {
        return Err(method_not_allowed());
    }
    let command = if method == Method::POST {
        Some(parse_json::<WorkerMaterialCommand>(&body)?)
    } else {
        None
    };
    let scope = if let Some(command) = &command {
        &command.scope
    } else {
        &WorkerMaterialQuery {
            order_id: query.get("order_id").cloned().unwrap_or_default(),
            apparatus: query.get("apparatus").cloned().unwrap_or_default(),
        }
    };
    let order_id = scope.order_id.trim();
    let apparatus = scope.apparatus.trim();
    material_link_requests::worker_scope(&state, &principal, order_id, apparatus).await?;
    let id = ApparatusId::new(apparatus.to_string())
        .map_err(|_| bad_request("raw_material_invalid_input"))?;
    let configuration = state
        .apparatus
        .current_configuration(&id)
        .await
        .map_err(canonical_apparatus_error)?
        .ok_or_else(|| not_found("apparatus_not_found"))?;
    let enabled = configuration.has_coherent_source()
        && configuration.is_active()
        && configuration.runtime.worker_material_assignment_allowed;
    if !enabled {
        if command.is_some() {
            return Err(forbidden());
        }
        return Ok(json_response(
            serde_json::json!({"enabled": false, "candidates": []}),
        ));
    }
    let store = state
        .material_link_requests
        .as_ref()
        .ok_or_else(|| server_error("material_link_unavailable"))?
        .production_store();
    let _guard = state.production_maps.queue_action_guard().await;
    if let Some(command) = &command {
        if command.barcode.trim().is_empty() || command.placement_version < 1 {
            return Err(bad_request("raw_material_invalid_input"));
        }
        let map = state
            .production_maps
            .raw_map(order_id)
            .await
            .map_err(production_map_error)?
            .ok_or_else(|| not_found("map_not_found"))?;
        let actor = queue_action_actor(&principal);
        let expected_stock = store
            .worker_material_validation_snapshot(command.barcode.trim())
            .await
            .map_err(production_map_error)?;
        let existing = state
            .production_maps
            .raw_material_assignments_for_order(order_id)
            .await
            .map_err(production_map_error)?
            .into_iter()
            .find(|a| {
                a.barcode.eq_ignore_ascii_case(command.barcode.trim())
                    && a.apparatus_id == id
                    && a.assigned_by_role == actor.role
                    && a.assigned_by_ref == actor.ref_
            });
        let assignment = if let Some(existing) = existing {
            existing
        } else {
            let (input, _) = raw_material_details::fill_raw_material_assignment_input(
                &state,
                &principal,
                RawMaterialAssignmentInput {
                    order_id: order_id.into(),
                    apparatus: apparatus.into(),
                    barcode: command.barcode.clone(),
                    ..Default::default()
                },
            )
            .await?;
            state
                .production_maps
                .prepare_raw_material_assignment(input, &actor)
                .await
                .map_err(production_map_error)?
        };
        let result = store
            .use_worker_material(
                assignment,
                configuration.runtime.source_revision,
                &map,
                command.placement_version,
                &expected_stock,
                &actor,
            )
            .await
            .map_err(production_map_error)?;
        state.production_maps.notify_live();
        if let Ok(Some(stock)) = state
            .gscale
            .raw_material_stock_by_barcode(&result.assignment.barcode)
            .await
        {
            state
                .warehouse_events
                .notify_updated(&stock.warehouse, "raw_material_intake");
        }
        return Ok(json_response(
            serde_json::json!({"ok":true,"barcode":result.assignment.barcode,"in_use":result.in_use}),
        ));
    }
    let mut candidates = Vec::new();
    for candidate in store
        .worker_material_candidates(apparatus, order_id)
        .await
        .map_err(production_map_error)?
    {
        match raw_material_details::fill_raw_material_assignment_input(
            &state,
            &principal,
            RawMaterialAssignmentInput {
                order_id: order_id.into(),
                apparatus: apparatus.into(),
                barcode: candidate.barcode.clone(),
                ..Default::default()
            },
        )
        .await
        {
            Ok(_) => candidates.push(candidate),
            Err(error) if error.0.is_server_error() => return Err(error),
            Err(_) => {}
        }
    }
    Ok(json_response(
        serde_json::json!({"enabled":true,"candidates":candidates}),
    ))
}
