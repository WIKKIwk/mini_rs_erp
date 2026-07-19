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

fn apply_authoritative_calculation(
    map: &mut ProductionMapDefinition,
    template: &CalculateOrderTemplate,
) -> Result<(), AdminError> {
    let response = calculate(CalculateRequest {
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
        first_layer: LayerInput::new(
            template.first_layer_material.trim(),
            template.first_layer_micron.trim(),
        ),
        second_layer: LayerInput::new(
            template.second_layer_material.trim(),
            template.second_layer_micron.trim(),
        ),
        third_layer: LayerInput::new(
            template.third_layer_material.trim(),
            template.third_layer_micron.trim(),
        ),
        note: if template.note.trim().is_empty() {
            None
        } else {
            Some(template.note.trim().to_string())
        },
        ..CalculateRequest::default()
    })
    .map_err(|error| bad_request(&error))?;

    let base_length = response
        .results
        .first()
        .map(|result| result.base_length)
        .ok_or_else(|| bad_request("calculate result is empty"))?;
    map.width_mm = Some(response.width_mm);
    map.order_kg = Some(response.kg);
    map.base_length = Some(base_length);
    map.roll_count = response.roll_count;
    Ok(())
}

fn spawn_order_integrations(
    state: AppState,
    map: ProductionMapDefinition,
    template: CalculateOrderTemplate,
) {
    tokio::spawn(async move {
        if let Err(error) = state.order_sheets.append_order(&map, &template).await {
            tracing::warn!(?error, map_id = %map.id, "google sheets order append failed");
        }
        if let Err(error) = state.production_orders.save_order(&map, &template).await {
            tracing::warn!(?error, map_id = %map.id, "mini order save failed");
        }
    });
}

