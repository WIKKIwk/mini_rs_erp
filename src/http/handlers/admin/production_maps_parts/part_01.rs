pub async fn production_map_audit(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::ProductionMapRead,
        ],
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
            Capability::ProductionMapRead,
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
            Capability::ProductionMapRead,
            Capability::ApparatusQueueRead,
            Capability::RawMaterialAssign,
            Capability::QolipManage,
            Capability::PreparationAccess,
        ],
    )
    .await?;
    if !matches!(method, Method::GET | Method::PUT) {
        return Err(method_not_allowed());
    }
    match method {
        Method::GET => {
            if principal.role == PrincipalRole::RezkaMaster {
                super::rezka_master::require_order_scope(&state, &principal, &query.id).await?;
            }
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
                &[
                    Capability::AdminAccess,
                    Capability::ProductionMapManage,
                    Capability::ProductionMapEdit,
                ],
            )
            .await?;
            let mut input: ProductionMapDefinition = parse_json(&body)?;
            let previous_combo = state
                .production_maps
                .raw_map(&input.id)
                .await
                .map_err(production_map_error)?;
            if principal.role == PrincipalRole::SalesManager && previous_combo.is_none() {
                return Err(forbidden());
            }
            super::combos::validate_combo_map_catalog_identity(&state, &input).await?;
            super::combos::validate_combo_map_update(&input, previous_combo.as_ref())?;
            assign_order_number_if_missing(&state, &mut input)
                .await
                .map_err(production_map_error)?;
            if state.production_orders.enabled()
                && is_sheet_order_map(&input)
                && state
                    .production_maps
                    .raw_map(&input.id)
                    .await
                    .map_err(production_map_error)?
                    .is_none()
            {
                return Err(bad_request(
                    "Yangi orderni Calculate hisob-kitobi bilan oching. Faqat production mapni saqlash order ochish uchun yetarli emas",
                ));
            }
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

#[derive(serde::Deserialize, serde::Serialize)]
struct ProductionMapSaveWithOrderRequest {
    #[serde(default)]
    pending_order_id: String,
    map: ProductionMapDefinition,
    #[serde(default)]
    template: Option<CalculateOrderTemplate>,
    /// `use` opens an independent order; `fork` also creates a reusable variant.
    #[serde(default)]
    template_action: String,
}

#[derive(serde::Deserialize)]
struct ProductionMapAutoOpenRequest {
    template: CalculateOrderTemplate,
    #[serde(default)]
    duplicate_rezka_labels: bool,
    #[serde(default)]
    include_bobina_in_net: bool,
    #[serde(default)]
    standard_print_side: Option<bool>,
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
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::ProductionOrderCreate,
        ],
    )
    .await?;
    if method != Method::PUT {
        return Err(method_not_allowed());
    }
    let input: ProductionMapSaveWithOrderRequest = parse_json(&body)?;
    save_production_map_with_order(&state, &principal, input).await
}

/// Generates and saves an order-specific production map using the same
/// deterministic routing used by Telegram automatic order intake.
pub async fn production_map_auto_open(
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
            Capability::ProductionOrderCreate,
        ],
    )
    .await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let ProductionMapAutoOpenRequest {
        mut template,
        duplicate_rezka_labels,
        include_bobina_in_net,
        standard_print_side,
    } = parse_json(&body)?;
    super::combos::validate_combo_template(&state, &template).await?;
    if !template.products.is_empty() {
        template = crate::core::calculate_orders::hydrate_template_dimensions(template);
    }
    // Automatic opening always creates a new order from the current Calculate
    // fields; never mutate or clone the reusable template identity supplied by
    // the client.
    template.id.clear();
    template.variant_id.clear();
    template.code.clear();
    template.order_number.clear();
    template.source_map_id.clear();
    validate_template(&template).map_err(calculate_order_error)?;
    template.order_number = state
        .production_maps
        .next_order_number()
        .await
        .map_err(production_map_error)?;
    let apparatus = state
        .apparatus
        .list_runtime_configurations()
        .await
        .map_err(canonical_apparatus_error)?;
    let materials = state
        .calculate_materials
        .list()
        .await
        .map_err(|_| server_error("material catalog failed"))?;
    let mut map = automatic::generate(&template, &apparatus, &materials)
        .map_err(|error| bad_request(&error))?;
    map.duplicate_rezka_labels = Some(duplicate_rezka_labels);
    map.include_bobina_in_net = Some(include_bobina_in_net);
    map.standard_print_side = standard_print_side;
    template.order_number = map.order_number.clone();
    template.code = map.code.clone();
    template.width_mm = map.width_mm.unwrap_or_default();
    template.image_url.clear();
    save_production_map_with_order(
        &state,
        &principal,
        ProductionMapSaveWithOrderRequest {
            pending_order_id: String::new(),
            map,
            template: Some(template),
            template_action: String::new(),
        },
    )
    .await
}

include!("save_production_map_with_order.rs");
