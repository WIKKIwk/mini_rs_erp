async fn assign_order_number_if_missing(
    state: &AppState,
    map: &mut ProductionMapDefinition,
) -> Result<bool, ProductionMapError> {
    let map_id = map.id.trim();
    if !map.order_number.trim().is_empty()
        || !map_id.to_ascii_lowercase().starts_with("zakaz-draft-")
    {
        return Ok(false);
    }
    let order_number = state.production_maps.next_order_number().await?;
    map.id = format!("zakaz-{order_number}");
    map.code = order_number.clone();
    map.order_number = order_number;
    Ok(true)
}

fn template_map_copy_for_save(
    map: &ProductionMapDefinition,
    template: &CalculateOrderTemplate,
) -> Option<ProductionMapDefinition> {
    if !template.source_map_id.trim().is_empty() || !is_sheet_order_map(map) {
        return None;
    }
    let map_id = map.id.trim();
    if map_id.is_empty() {
        return None;
    }
    let mut template_map = map.clone();
    template_map.id = format!("template-{map_id}");
    template_map.code.clear();
    template_map.order_number.clear();
    template_map.order_kg = None;
    template_map.base_length = None;
    Some(template_map)
}

fn order_template_snapshot_for_map(
    map: &ProductionMapDefinition,
    template: &CalculateOrderTemplate,
) -> CalculateOrderTemplate {
    let mut snapshot = template.clone();
    let order_number = map.order_number.trim();
    let code = map.code.trim();
    if !order_number.is_empty() {
        snapshot.order_number = order_number.to_string();
    }
    if !code.is_empty() {
        snapshot.code = code.to_string();
    } else if !order_number.is_empty() {
        snapshot.code = order_number.to_string();
    }
    snapshot.source_map_id = map.id.trim().to_string();
    snapshot
}

fn is_quick_template_order_clone(
    map: &ProductionMapDefinition,
    template: &CalculateOrderTemplate,
) -> bool {
    let source_map_id = template.source_map_id.trim();
    !source_map_id.is_empty() && source_map_id != map.id.trim() && is_sheet_order_map(map)
}

fn template_source_map_id_for_save(
    map: &ProductionMapDefinition,
    template: &CalculateOrderTemplate,
) -> String {
    let source_map_id = template.source_map_id.trim();
    if source_map_id.is_empty() && !is_sheet_order_map(map) {
        map.id.trim().to_string()
    } else {
        source_map_id.to_string()
    }
}

pub(super) fn apply_order_rezka_kadr_count(
    map: &mut ProductionMapDefinition,
    template: &CalculateOrderTemplate,
    cut_apparatus_ids: &std::collections::BTreeSet<ApparatusId>,
) {
    let frame_count = template.frame_count.round();
    if !template.frame_count.is_finite() || frame_count <= 0.0 {
        return;
    }
    let frame_count = frame_count as i64;
    for node in &mut map.nodes {
        if node.kind != ProductionMapNodeKind::Apparatus
            || !node
                .canonical_apparatus_id()
                .is_some_and(|apparatus_id| cut_apparatus_ids.contains(&apparatus_id))
        {
            continue;
        }
        node.rezka_kadr_count = Some(frame_count);
    }
}

pub(super) async fn canonical_cut_apparatus_ids(
    state: &AppState,
) -> Result<std::collections::BTreeSet<ApparatusId>, AdminError> {
    let configurations = state
        .apparatus
        .list_runtime_configurations()
        .await
        .map_err(canonical_apparatus_error)?;
    let mut ids = std::collections::BTreeSet::new();
    for configuration in configurations {
        if !configuration.has_coherent_source() || !configuration.is_active() {
            return Err(server_error("canonical apparatus projection is incoherent"));
        }
        if configuration.runtime.execution_profile.operation
            == crate::core::apparatus_standard::ExecutionOperation::Cut
        {
            ids.insert(configuration.runtime.apparatus_id);
        }
    }
    Ok(ids)
}

pub(super) fn apply_authoritative_calculation(
    map: &mut ProductionMapDefinition,
    template: &CalculateOrderTemplate,
    material_catalog: &[crate::core::calculate_materials::CalculateMaterial],
) -> Result<(), AdminError> {
    let response = calculate_with_material_catalog(
        CalculateRequest {
            order_number: if template.order_number.trim().is_empty() {
                None
            } else {
                Some(template.order_number.trim().to_string())
            },
            customer: if template.customer.trim().is_empty() {
                None
            } else {
                Some(template.customer.trim().to_string())
            },
            product: Some(template.product.trim().to_string()),
            status: if template.status.trim().is_empty() {
                None
            } else {
                Some(template.status.trim().to_string())
            },
            material_display: if template.material_display.trim().is_empty() {
                None
            } else {
                Some(template.material_display.trim().to_string())
            },
            color: if template.color.trim().is_empty() {
                None
            } else {
                Some(template.color.trim().to_string())
            },
            kg: Some(template.kg),
            frame_product_size_mm: Some(template.frame_product_size_mm),
            frame_count: Some(template.frame_count),
            edge_allowance_mm: Some(template.edge_allowance_mm),
            waste_percent: Some(template.waste_percent),
            roll_count: template.roll_count,
            layers: template.effective_layers(),
            note: if template.note.trim().is_empty() {
                None
            } else {
                Some(template.note.trim().to_string())
            },
            ..CalculateRequest::default()
        },
        material_catalog,
    )
    .map_err(|error| bad_request(&error))?;

    let planned_length = response
        .results
        .first()
        .map(|result| result.rounded_length)
        .ok_or_else(|| bad_request("calculate result is empty"))?;
    map.width_mm = Some(response.width_mm);
    map.print_val_size_mm = template.print_val_size_mm;
    map.order_kg = Some(response.kg);
    // The legacy map field carries the production target, including waste.
    map.base_length = Some(planned_length);
    map.roll_count = response.roll_count;
    Ok(())
}

fn spawn_order_sheet_append(
    order_sheets: std::sync::Arc<dyn crate::google_sheets::OrderSheetSink>,
    map: ProductionMapDefinition,
    template: CalculateOrderTemplate,
) {
    // Mobile orders sync to Sheets only. Telegram group delivery belongs to
    // the bot intake flow, not to saving an order through the Mobile API.
    tokio::spawn(async move {
        if let Err(error) = order_sheets.append_order(&map, &template).await {
            tracing::warn!(?error, map_id = %map.id, "google sheets order append failed");
        }
    });
}
