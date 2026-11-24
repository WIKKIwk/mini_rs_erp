async fn save_production_map_with_order(
    state: &AppState,
    principal: &Principal,
    mut input: ProductionMapSaveWithOrderRequest,
) -> Result<Response, AdminError> {
    if input.map.combo.is_some() && input.template.is_none() {
        return Err(bad_request("combo_template_required"));
    }
    if let Some(template) = input.template.take() {
        super::combos::validate_combo_template(state, &template).await?;
        if input.map.combo.is_some() && input.map.combo != template.combo {
            return Err(conflict("combo_reference_mismatch"));
        }
        let template = if template.products.is_empty() {
            template
        } else {
            crate::core::calculate_orders::hydrate_template_dimensions(template)
        };
        validate_multi_product_customer(state, &template).await?;
        input.map.products = template.products.clone();
        input.map.combo = template.combo.clone();
        if !template.products.is_empty() {
            input.map.product_code = template.item_code.clone();
            input.map.title = template.product.clone();
        }
        input.template = Some(template);
    }
    super::combos::validate_combo_map_catalog_identity(state, &input.map).await?;
    if !input.pending_order_id.is_empty() {
        return complete_pending_order(state, principal, input).await;
    }
    if !matches!(
        input.template_action.as_str(),
        "" | "use" | "update" | "fork"
    ) {
        return Err(bad_request("template_action_invalid"));
    }
    if input.template_action == "use"
        && !is_sheet_order_map(&input.map)
        && !input.map.id.starts_with("zakaz-draft-")
    {
        return Err(bad_request("template_action_invalid"));
    }
    let owner_key = principal_owner_key(principal);
    let creation = order_creation_identity(&input);
    if let Some(response) = replay_order_creation(state, &owner_key, creation.as_ref()).await? {
        return Ok(json_response(response));
    }
    let fork_template = input.template_action == "fork";
    validate_quick_template_identity(state, &owner_key, &input).await?;
    // The calculate draft owns the photo. Synchronize it before deriving a
    // template fork so the order and its independent source map agree.
    if let Some(template) = input.template.as_ref() {
        input.map.image_id = template.image_id.trim().to_string();
    }
    let mut fork_map = prepare_template_fork(state, &mut input).await?;
    if let Some(template) = &input.template {
        validate_template(template).map_err(calculate_order_error)?;
        validate_order_type_apparatus(state, &input.map, template).await?;
        input.map.print_val_size_mm = template.print_val_size_mm;
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
    let order_number_was_generated = assign_order_number_if_missing(state, &mut input.map)
        .await
        .map_err(production_map_error)?;
    if order_number_was_generated {
        let order_number = input.map.order_number.trim().to_string();
        if let Some(template) = input.template.as_mut() {
            template.order_number = order_number;
        }
    }
    let opens_quick_template_as_order = !fork_template
        && input
            .template
            .as_ref()
            .is_some_and(|template| is_quick_template_order_clone(&input.map, template));
    let map_id = input.map.id.trim().to_string();
    let previous = state
        .production_maps
        .raw_map(&map_id)
        .await
        .map_err(production_map_error)?;
    if previous.as_ref().is_some_and(|map| map.combo.is_some()) && input.map.combo.is_none() {
        return Err(conflict("combo_reference_required"));
    }
    if previous.is_none()
        && (is_sheet_order_map(&input.map) || fork_template)
        && let Some(template) = input.template.as_mut()
    {
        if is_quick_template_order_clone(&input.map, template) {
            refresh_reused_order_product(state, template, &mut input.map, fork_map.as_mut())
                .await?;
        }
        let cut_apparatus_ids = canonical_cut_apparatus_ids(state).await?;
        apply_order_rezka_kadr_count(&mut input.map, template, &cut_apparatus_ids)?;
        if let Some(map) = fork_map.as_mut() {
            apply_order_rezka_kadr_count(map, template, &cut_apparatus_ids)?;
        }
    }
    let template_map = fork_map.or_else(|| {
        input
            .template
            .as_ref()
            .and_then(|template| template_map_copy_for_save(&input.map, template))
    });
    let template_map_id = template_map.as_ref().map(|map| map.id.trim().to_string());
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
    if previous.is_none() && is_sheet_order_map(&input.map) && state.production_orders.enabled() {
        let template = input
            .template
            .as_ref()
            .ok_or_else(|| bad_request("Order ochish uchun Calculate hisob-kitobi kerak"))?;
        if input.map.standard_print_side.is_none() {
            input.map.standard_print_side = Some(state.production_orders.print_side_preference(template)
                .await.map_err(|_| server_error("print side preference lookup failed"))?.unwrap_or(true));
        }
        let _guard = state.production_maps.queue_action_guard().await;
        let saved = state
            .production_maps
            .prepare_map_for_save(input.map.clone())
            .await
            .map_err(production_map_error)?;
        let template_map = match template_map {
            Some(map) => Some(
                state
                    .production_maps
                    .prepare_map_for_save(map)
                    .await
                    .map_err(production_map_error)?
                    .map,
            ),
            None => None,
        };
        let snapshot = order_template_snapshot_for_map(&saved.map, template);
        let quick_template = if opens_quick_template_as_order {
            None
        } else {
            let mut quick = template.clone();
            if fork_template {
                quick.kg = 0.0;
                quick.order_number.clear();
            }
            quick.source_map_id = template_map
                .as_ref()
                .map(|m| m.id.clone())
                .unwrap_or_else(|| template_source_map_id_for_save(&saved.map, template));
            Some(quick)
        };
        let created = state
            .production_orders
            .create_order_atomic(&crate::core::mini_orders::NewProductionOrder {
                creation: creation.clone(),
                map: saved.map.clone(),
                template_map,
                template: snapshot.clone(),
                quick_template,
                owner_key: owner_key.clone(),
                created_by: queue_action_actor(principal),
            })
            .await;
        let saved_template = match created {
            Ok(template) => template,
            Err(error) => {
                if let Some(response) =
                    replay_order_creation(state, &owner_key, creation.as_ref()).await?
                {
                    return Ok(json_response(response));
                }
                return Err(production_map_error(error));
            }
        };
        state.production_maps.notify_live();
        spawn_order_sheet_append(state.order_sheets.clone(), saved.map.clone(), snapshot);
        return Ok(json_response(serde_json::json!({
            "ok": true, "saved": saved, "template": saved_template,
        })));
    }
    if !is_sheet_order_map(&input.map)
        && state.production_orders.enabled()
        && let Some(mut template) = input.template.clone()
    {
        let _guard = state.production_maps.queue_action_guard().await;
        let saved = state
            .production_maps
            .prepare_map_for_save(input.map.clone())
            .await
            .map_err(production_map_error)?;
        template.source_map_id = template_source_map_id_for_save(&saved.map, &template);
        let saved_template = state
            .production_orders
            .save_template_map_atomic(&saved.map, &template, &owner_key)
            .await
            .map_err(production_map_error)?;
        state.production_maps.notify_live();
        return Ok(json_response(
            serde_json::json!({"ok":true,"saved":saved,"template":saved_template}),
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
        spawn_order_sheet_append(state.order_sheets.clone(), saved_map.map.clone(), template);
    }
    Ok(json_response(serde_json::json!({
        "ok": true,
        "saved": saved_map,
        "template": saved_template,
    })))
}

include!("../production_maps_save_helpers.rs");
include!("../production_maps_template_intent.rs");
include!("../production_maps_pending_order.rs");
include!("../production_maps_order_edit.rs");

#[derive(serde::Deserialize)]
struct ApparatusSequencePutRequest {
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    order_ids: Vec<String>,
    #[serde(default)]
    moved_order_id: Option<String>,
}
