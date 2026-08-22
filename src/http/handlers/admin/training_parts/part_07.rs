
pub async fn training_production_map_save_with_order(
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

    let mut input: TrainingMapSaveWithOrderRequest = parse_json(&body)?;
    validate_template(&input.template).map_err(training_calculate_error)?;
    input.map.customer_name = input.template.customer.trim().to_string();
    if input.template.kg > 0.0 {
        let material_catalog = state
            .calculate_materials
            .list()
            .await
            .map_err(|_| server_error("calculate materials store failed"))?;
        super::production_maps::apply_authoritative_calculation(
            &mut input.map,
            &input.template,
            &material_catalog,
        )?;
    }
    snapshot_new_training_order_rezka_kadr_count(&state, &mut input.map, &input.template).await?;

    let owner = owner_key("admin", &principal.ref_);
    let saved = training_store(&state)?
        .save_map_with_order(input.map, input.template, &owner)
        .await
        .map_err(training_workspace_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "saved": saved.saved,
        "template": saved.template,
    })))
}

pub async fn training_apparatus_modes(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    let store = training_store(&state)?;
    match method {
        Method::GET => {
            let modes = store
                .apparatus_modes()
                .await
                .map_err(training_workspace_error)?;
            Ok(json_response(serde_json::json!({"modes": modes})))
        }
        Method::PUT => {
            let input: TrainingApparatusModeInput = parse_json(&body)?;
            store
                .set_apparatus_mode(&input.apparatus, input.enabled)
                .await
                .map_err(training_workspace_error)?;
            state.production_maps.notify_live();
            Ok(json_response(serde_json::json!({
                "apparatus": input.apparatus.trim(),
                "enabled": input.enabled,
            })))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn training_restart(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: TrainingRestartInput = parse_json(&body)?;
    let apparatus = input.apparatus.trim().to_string();
    let reset_count = training_store(&state)?
        .reset_queue_states(&apparatus)
        .await
        .map_err(training_workspace_error)?;
    state.production_maps.notify_live();
    Ok(json_response(serde_json::json!({
        "ok": true,
        "apparatus": apparatus,
        "reset_count": reset_count,
    })))
}

pub async fn training_completed_orders(
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
            Capability::ApparatusQueueRead,
        ],
    )
    .await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let events = training_store(&state)?
        .completed_queue_events_for_actor(&principal.ref_, 200)
        .await
        .map_err(training_workspace_error)?;
    let completed_orders = events
        .into_iter()
        .map(|event| {
            let status = if event.action == "complete" && event.to_state == "completed" {
                "completed"
            } else {
                "in_progress"
            };
            serde_json::json!({
                "apparatus": event.apparatus,
                "order_id": event.order_id,
                "completed_at_unix": event.created_at_unix,
                "status": status,
                "actor_ref": event.actor_ref,
                "actor_display_name": event.actor_display_name,
            })
        })
        .collect::<Vec<_>>();
    Ok(json_response(serde_json::json!({
        "completed_orders": completed_orders,
    })))
}

pub async fn training_order_statuses(
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

    let store = training_store(&state)?;
    let maps = store.maps().await.map_err(training_workspace_error)?;
    let state_records = store
        .queue_state_records()
        .await
        .map_err(training_workspace_error)?;
    let latest_events = store
        .latest_queue_events()
        .await
        .map_err(training_workspace_error)?;
    let mut statuses = BTreeMap::new();
    for saved in maps {
        let order_id = saved.map.id.trim().to_string();
        if order_id.is_empty() {
            continue;
        }
        let map_apparatus = saved
            .map
            .nodes
            .iter()
            .find(|node| node.kind == ProductionMapNodeKind::Apparatus)
            .map(|node| node.apparatus_id.trim().to_string())
            .unwrap_or_default();
        let state_record = state_records
            .iter()
            .filter(|record| {
                record.order_id == order_id
                    && (map_apparatus.is_empty()
                        || canonical_apparatus_matches(&record.apparatus, &map_apparatus))
            })
            .max_by_key(|record| record.updated_at_unix);
        let event = latest_events
            .iter()
            .filter(|event| {
                event.order_id == order_id
                    && (map_apparatus.is_empty()
                        || canonical_apparatus_matches(&event.apparatus, &map_apparatus))
            })
            .max_by_key(|event| event.created_at_unix);
        let state = state_record
            .map(|record| record.state.as_str())
            .or_else(|| event.map(|event| event.to_state.as_str()))
            .unwrap_or("pending")
            .to_string();
        let apparatus = state_record
            .map(|record| record.apparatus.clone())
            .or_else(|| event.map(|event| event.apparatus.clone()))
            .unwrap_or(map_apparatus);
        let updated_at_unix = state_record
            .map(|record| record.updated_at_unix)
            .or_else(|| event.map(|event| event.created_at_unix))
            .unwrap_or_default();
        let completed_at_unix = if state == "completed" {
            event
                .filter(|event| event.action == "complete" && event.to_state == "completed")
                .map(|event| event.created_at_unix)
                .unwrap_or_default()
        } else {
            0
        };
        statuses.insert(
            order_id.clone(),
            serde_json::json!({
                "order_id": order_id,
                "apparatus": apparatus,
                "state": state.clone(),
                "status": state,
                "action": event.map(|event| event.action.clone()).unwrap_or_default(),
                "actor_ref": event.map(|event| event.actor_ref.clone()).unwrap_or_default(),
                "actor_display_name": event
                    .map(|event| event.actor_display_name.clone())
                    .unwrap_or_default(),
                "updated_at_unix": updated_at_unix,
                "completed_at_unix": completed_at_unix,
            }),
        );
    }
    Ok(json_response(serde_json::json!({"statuses": statuses})))
}

pub async fn training_raw_material_assignments(
    State(state): State<AppState>,
    Query(query): Query<TrainingRawMaterialAssignmentsQuery>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::RawMaterialAssign,
        ],
    )
    .await?;
    let store = training_store(&state)?;
    match method {
        Method::GET => {
            let assignments = store
                .raw_material_assignments(&query.order_id, &query.apparatus)
                .await
                .map_err(training_workspace_error)?;
            Ok(json_response(assignments))
        }
        Method::POST => {
            let payload: serde_json::Value = parse_json(&body)?;
            let order_id = payload
                .get("order_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .trim();
            if order_id.is_empty() {
                return Err(bad_request("order_id kerak"));
            }
            if store
                .map(order_id)
                .await
                .map_err(training_workspace_error)?
                .is_none()
            {
                return Err(not_found("training_order_not_found"));
            }
            let assignment = store
                .save_raw_material_assignment(payload)
                .await
                .map_err(training_workspace_error)?;
            Ok(json_response(assignment))
        }
        Method::DELETE => {
            let order_id = query.order_id.trim();
            let apparatus = query.apparatus.trim();
            let barcode = query.barcode.trim();
            if order_id.is_empty() || apparatus.is_empty() || barcode.is_empty() {
                return Err(bad_request("order_id, apparatus va barcode kerak"));
            }
            let deleted = store
                .delete_raw_material_assignment(order_id, apparatus, barcode)
                .await
                .map_err(training_workspace_error)?;
            if !deleted {
                return Err(not_found("training_material_assignment_not_found"));
            }
            Ok(json_response(serde_json::json!({
                "ok": true,
                "order_id": order_id,
                "apparatus": apparatus,
                "barcode": barcode,
            })))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn training_order_image_upload(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
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
    if method == Method::DELETE {
        let image_id = query_value(&uri, "id")
            .filter(|value| safe_image_id(value))
            .ok_or_else(|| bad_request("id kerak"))?;
        let owner = owner_key("admin", &principal.ref_);
        let deleted = training_store(&state)?
            .delete_image(&owner, &image_id)
            .await
            .map_err(training_workspace_error)?;
        if !deleted {
            return Err(not_found("rasm topilmadi"));
        }
        return Ok(json_response(serde_json::json!({"ok": true})));
    }
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    if body.is_empty() {
        return Err(bad_request("rasm kerak"));
    }
    const MAX_IMAGE_BYTES: usize = 6 * 1024 * 1024;
    if body.len() > MAX_IMAGE_BYTES {
        return Err(bad_request("rasm hajmi katta"));
    }
    let mime = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("image/jpeg")
        .split(';')
        .next()
        .unwrap_or("image/jpeg")
        .trim()
        .to_ascii_lowercase();
    let extension = image_extension(&mime).ok_or_else(|| bad_request("rasm formati noto'g'ri"))?;
    let image_id = format!("training-img{}", unix_micros());
    let image_name = headers
        .get("x-file-name")
        .and_then(|value| value.to_str().ok())
        .map(clean_file_name)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("rang.{extension}"));
    let owner = owner_key("admin", &principal.ref_);
    let image = training_store(&state)?
        .save_image(
            &owner,
            TrainingImage {
                image_id,
                image_name,
                image_mime: mime,
                image_size_bytes: body.len() as u64,
                body: body.to_vec(),
            },
        )
        .await
        .map_err(training_workspace_error)?;
    let image_url = format!(
        "/v1/mobile/admin/training/images/view?id={}",
        image.image_id
    );
    Ok(json_response(serde_json::json!({
        "ok": true,
        "image": {
            "image_id": image.image_id,
            "image_name": image.image_name,
            "image_mime": image.image_mime,
            "image_size_bytes": image.image_size_bytes,
            "image_url": image_url,
        }
    })))
}

pub async fn training_order_image_view(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
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
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let image_id = query_value(&uri, "id")
        .filter(|value| safe_image_id(value))
        .ok_or_else(|| bad_request("id kerak"))?;
    let owner = owner_key("admin", &principal.ref_);
    let image = training_store(&state)?
        .image(&owner, &image_id)
        .await
        .map_err(training_workspace_error)?
        .ok_or_else(|| not_found("rasm topilmadi"))?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, image.image_mime)
        .header(header::CACHE_CONTROL, "private, max-age=86400")
        .body(Body::from(image.body))
        .map_err(|_| server_error("training image response failed"))
}
