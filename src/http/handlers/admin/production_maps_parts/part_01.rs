
pub async fn production_map_audit(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let report = state
        .production_maps
        .audit_production_workflow()
        .await
        .map_err(production_map_error)?;
    Ok(json_response(report))
}

/// Capacity calendars and schedule reservations are shared planning state. Read
/// access is available to queue viewers; mutation remains an admin/planner
/// capability and the server stamps the authenticated actor on every write.
pub async fn production_map_capacity(
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
            Capability::ApparatusQueueRead,
        ],
    )
    .await?;
    match method {
        Method::GET => {
            let capacity = state
                .apparatus
                .list_runtime_configurations()
                .await
                .map_err(canonical_apparatus_error)?
                .into_iter()
                .map(|configuration| configuration.capacity)
                .collect::<Vec<_>>();
            Ok(json_response(serde_json::json!({
                "ok": true,
                "capacity": capacity,
            })))
        }
        Method::PUT => {
            authorize_any_capability(
                &state,
                &headers,
                &[Capability::AdminAccess, Capability::ProductionMapManage],
            )
            .await?;
            let input: CanonicalCapacityPatchRequest = parse_json(&body)?;
            let committed = state
                .apparatus
                .patch(
                    input.apparatus_id,
                    input.expected_revision,
                    CanonicalApparatusPatch {
                        capacity: Some(input.capacity),
                        ..CanonicalApparatusPatch::default()
                    },
                    canonical_command_metadata(&principal, &headers)?,
                )
                .await
                .map_err(canonical_apparatus_error)?;
            state.production_maps.notify_live();
            Ok(json_response(serde_json::json!({
                "ok": true,
                "revision": committed,
            })))
        }
        _ => {
            let _ = principal;
            Err(method_not_allowed())
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CanonicalCapacityPatchRequest {
    apparatus_id: ApparatusId,
    expected_revision: u64,
    capacity: ApparatusCapacity,
}

pub async fn production_map_capacity_downtime(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    if method != Method::POST && method != Method::PUT {
        return Err(method_not_allowed());
    }
    let mut input: ApparatusDowntime = parse_json(&body)?;
    input.actor = queue_action_actor(&principal);
    let downtime = state
        .production_maps
        .put_apparatus_downtime(input)
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "downtime": downtime,
    })))
}

pub async fn production_map_schedule(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let mut input: ApparatusScheduleRequest = parse_json(&body)?;
    input.actor = queue_action_actor(&principal);
    let result = state
        .production_maps
        .schedule_apparatus_order(input)
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "reservation": result.reservation,
        "conflicts": result.conflicts,
    })))
}

pub async fn production_map_schedule_cancel(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let mut input: ApparatusScheduleCancelRequest = parse_json(&body)?;
    input.actor = queue_action_actor(&principal);
    let reservation = state
        .production_maps
        .cancel_apparatus_schedule_reservation(input)
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "reservation": reservation,
    })))
}

pub async fn production_maps(
    State(state): State<AppState>,
    Query(query): Query<ProductionMapsQuery>,
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
            Capability::ApparatusQueueRead,
            Capability::RawMaterialAssign,
            Capability::QolipManage,
        ],
    )
    .await?;
    if !matches!(method, Method::GET | Method::PUT) {
        return Err(method_not_allowed());
    }
    match method {
        Method::GET => {
            if !query.id.trim().is_empty() {
                if query.id.trim().starts_with("training-") {
                    let overlay = super::training::worker_training_overlay(&state, &principal)
                        .await
                        .map_err(super::training::training_workspace_error)?;
                    let saved = overlay
                        .maps
                        .into_iter()
                        .find(|saved| saved.map.id.trim() == query.id.trim())
                        .ok_or_else(|| not_found("map_not_found"))?;
                    return Ok(json_response(saved));
                }
                let saved = state
                    .production_maps
                    .map(&query.id)
                    .await
                    .map_err(production_map_error)?
                    .ok_or_else(|| not_found("map_not_found"))?;
                return Ok(json_response(saved));
            }
            let mut maps = state
                .production_maps
                .maps()
                .await
                .map_err(|_| server_error("production maps fetch failed"))?;
            super::training::merge_worker_training_maps(&state, &principal, &mut maps)
                .await
                .map_err(super::training::training_workspace_error)?;
            Ok(json_response(maps))
        }
        Method::PUT => {
            authorize_any_capability(
                &state,
                &headers,
                &[Capability::AdminAccess, Capability::ProductionMapManage],
            )
            .await?;
            let mut input: ProductionMapDefinition = parse_json(&body)?;
            assign_order_number_if_missing(&state, &mut input)
                .await
                .map_err(production_map_error)?;
            match state.production_maps.upsert_map(input).await {
                Ok(saved) => Ok(json_response(saved)),
                Err(error) => Err(production_map_error(error)),
            }
        }
        _ => Err(method_not_allowed()),
    }
}

#[derive(Default, serde::Deserialize)]
pub struct ProductionMapsQuery {
    #[serde(default)]
    id: String,
}

#[derive(serde::Deserialize)]
struct ProductionMapSaveWithOrderRequest {
    map: ProductionMapDefinition,
    #[serde(default)]
    template: Option<CalculateOrderTemplate>,
}

/// Saves a production map and (optionally) its calculate order template in one
/// server-side operation, so the client never has to coordinate two writes.
pub async fn production_map_save_with_order(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    if method != Method::PUT {
        return Err(method_not_allowed());
    }
    let mut input: ProductionMapSaveWithOrderRequest = parse_json(&body)?;
    if let Some(template) = &input.template {
        validate_template(template).map_err(calculate_order_error)?;
        input.map.customer_name = template.customer.trim().to_string();
        if template.kg > 0.0 {
            let material_catalog = state
                .calculate_materials
                .list()
                .await
                .map_err(|_| production_map_error(ProductionMapError::StoreFailed))?;
            apply_authoritative_calculation(&mut input.map, template, &material_catalog)?;
        }
    }
    let order_number_was_generated = assign_order_number_if_missing(&state, &mut input.map)
        .await
        .map_err(production_map_error)?;
    if order_number_was_generated {
        let order_number = input.map.order_number.trim().to_string();
        if let Some(template) = input.template.as_mut() {
            template.order_number = order_number;
        }
    }
    let opens_quick_template_as_order = input
        .template
        .as_ref()
        .is_some_and(|template| is_quick_template_order_clone(&input.map, template));
    let owner_key = principal_owner_key(&principal);
    let map_id = input.map.id.trim().to_string();
    let template_map = input
        .template
        .as_ref()
        .and_then(|template| template_map_copy_for_save(&input.map, template));
    let template_map_id = template_map.as_ref().map(|map| map.id.trim().to_string());
    let previous = state
        .production_maps
        .raw_map(&map_id)
        .await
        .map_err(production_map_error)?;
    if previous.is_none()
        && is_sheet_order_map(&input.map)
        && let Some(template) = input.template.as_ref()
    {
        let cut_apparatus_ids = canonical_cut_apparatus_ids(&state).await?;
        apply_order_rezka_kadr_count(&mut input.map, template, &cut_apparatus_ids);
    }
    let previous_template_map = match &template_map_id {
        Some(template_map_id) => state
            .production_maps
            .raw_map(template_map_id)
            .await
            .map_err(production_map_error)?,
        None => None,
    };
    if opens_quick_template_as_order && previous.is_some() {
        return Err(production_map_error(
            ProductionMapError::DuplicateOrderNumber,
        ));
    }
    let saved_map = if let Some(template_map) = template_map {
        state
            .production_maps
            .upsert_maps_batch(vec![input.map, template_map])
            .await
            .map_err(production_map_error)?
            .into_iter()
            .next()
            .ok_or_else(|| production_map_error(ProductionMapError::StoreFailed))?
    } else {
        state
            .production_maps
            .upsert_map(input.map)
            .await
            .map_err(production_map_error)?
    };
    let mut integration_template = None;
    let saved_template = match input.template {
        Some(mut template) => {
            if opens_quick_template_as_order {
                integration_template =
                    Some(order_template_snapshot_for_map(&saved_map.map, &template));
                None
            } else {
                template.source_map_id = match template_map_id.as_deref() {
                    Some(template_map_id) => template_map_id.to_string(),
                    None => template_source_map_id_for_save(&saved_map.map, &template),
                };
                match state
                    .calculate_orders
                    .upsert(&owner_key, template)
                    .await
                    .map_err(calculate_order_error)
                {
                    Ok(saved_template) => {
                        integration_template = Some(order_template_snapshot_for_map(
                            &saved_map.map,
                            &saved_template,
                        ));
                        Some(saved_template)
                    }
                    Err(error) => {
                        if let Some(template_map_id) = template_map_id.as_deref()
                            && let Err(rollback_error) = state
                                .production_maps
                                .restore_map(previous_template_map.as_ref(), template_map_id)
                                .await
                        {
                            tracing::error!(
                                ?rollback_error,
                                "with-order template map rollback failed"
                            );
                        }
                        if let Err(rollback_error) = state
                            .production_maps
                            .restore_map(previous.as_ref(), &map_id)
                            .await
                        {
                            tracing::error!(?rollback_error, "with-order map rollback failed");
                        }
                        return Err(error);
                    }
                }
            }
        }
        None => None,
    };
    if previous.is_none()
        && is_sheet_order_map(&saved_map.map)
        && let Some(template) = integration_template
            .as_ref()
            .cloned()
            .or_else(|| saved_template.clone())
    {
        spawn_order_integrations(
            state.clone(),
            saved_map.map.clone(),
            template,
            owner_key,
            principal.display_name.clone(),
            principal.phone.clone(),
        );
    }
    Ok(json_response(serde_json::json!({
        "ok": true,
        "saved": saved_map,
        "template": saved_template,
    })))
}

include!("../production_maps_save_helpers.rs");

#[derive(serde::Deserialize)]
struct ApparatusSequencePutRequest {
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    order_ids: Vec<String>,
}
