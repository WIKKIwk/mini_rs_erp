#[derive(Deserialize)]
pub struct OpenedOrderEditQuery {
    id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenedOrderEditRequest {
    original: crate::core::order_edit::OrderEditSource,
    template: CalculateOrderTemplate,
}

fn opened_order_edit_error(error: crate::core::order_edit::OrderEditError) -> AdminError {
    use crate::core::order_edit::OrderEditError;
    match error {
        OrderEditError::NotFound => not_found(error.to_string()),
        OrderEditError::Locked(_) | OrderEditError::Conflict => conflict(error.to_string()),
        OrderEditError::Invalid(_) => bad_request(error.to_string()),
        OrderEditError::Store => server_error(error.to_string()),
    }
}

pub async fn production_map_order_edit(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<OpenedOrderEditQuery>,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    if method != Method::GET && method != Method::PUT {
        return Err(method_not_allowed());
    }
    let _guard = state.production_maps.queue_action_guard().await;
    let current = state
        .production_orders
        .order_edit_source(query.id.trim())
        .await
        .map_err(opened_order_edit_error)?;
    let materials = state
        .calculate_materials
        .list()
        .await
        .map_err(|_| server_error("Calculate materiallari yuklanmadi"))?;
    let mut baseline = current.map.clone();
    apply_authoritative_calculation(&mut baseline, &current.template, &materials)?;
    let same_quantity = |left: Option<f64>, right: Option<f64>| match (left, right) {
        (Some(left), Some(right)) => (left - right).abs() < 0.001,
        (None, None) => true,
        _ => false,
    };
    if !same_quantity(baseline.width_mm, current.map.width_mm)
        || !same_quantity(baseline.base_length, current.map.base_length)
        || baseline.roll_count != current.map.roll_count
        || baseline.print_val_size_mm != current.map.print_val_size_mm
    {
        return Err(conflict(
            "Asl Calculate qiymatlari mavjud mapga mos emas: tahrirlash mumkin emas",
        ));
    }
    if method == Method::GET {
        return Ok(json_response(current));
    }
    let input: OpenedOrderEditRequest = parse_json(&body)?;
    if input.original.map != current.map
        || input.original.revision != current.revision
        || input.original.template != current.template
    {
        return Err(opened_order_edit_error(
            crate::core::order_edit::OrderEditError::Conflict,
        ));
    }
    let mut template = crate::core::calculate_orders::hydrate_template_layers(input.template);
    // Identity belongs to the existing order, not to the submitted form.
    template.id = current.template.id.clone();
    template.code = current.map.code.clone();
    template.order_number = current.map.order_number.clone();
    template.source_map_id = current.map.id.clone();
    validate_template(&template).map_err(calculate_order_error)?;
    let mut map = current.map.clone();
    apply_authoritative_calculation(&mut map, &template, &materials)?;
    template.width_mm = map.width_mm.unwrap_or(template.width_mm);
    map.customer_name = template.customer.trim().to_string();
    map.product_code = template.item_code.trim().to_string();
    map.title = template.product.trim().to_string();
    map.image_id = template.image_id.trim().to_string();
    // A custom split is immutable; incompatible frame groups fail validation.
    // Only recalculate cut counts that represented the original frame count.
    let apparatus = state
        .apparatus
        .list_runtime_configurations()
        .await
        .map_err(canonical_apparatus_error)?;
    let cut_ids = apparatus
        .iter()
        .filter(|a| {
            a.runtime.execution_profile.operation
                == crate::core::apparatus_standard::ExecutionOperation::Cut
        })
        .map(|a| a.runtime.apparatus_id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    for node in &mut map.nodes {
        if node
            .canonical_apparatus_id()
            .is_some_and(|id| cut_ids.contains(&id))
        {
            if current.template.frame_count != template.frame_count
                && node
                    .rezka_kadr_count
                    .is_some_and(|n| n != current.template.frame_count as i64)
            {
                return Err(conflict("Kadr soni mavjud Rezka bo‘linishiga mos emas"));
            }
            if node.rezka_kadr_count.is_none()
                || node.rezka_kadr_count == Some(current.template.frame_count as i64)
            {
                node.rezka_kadr_count = Some(template.frame_count as i64);
            }
        }
    }
    crate::core::order_edit::validate_route(&current.template, &template, &map, &apparatus)
        .map_err(opened_order_edit_error)?;
    let prepared = state
        .production_maps
        .prepare_map_for_save(map)
        .await
        .map_err(production_map_error)?;
    let saved = state
        .production_orders
        .save_order_edit(
            &current,
            &prepared.map,
            &template,
            &queue_action_actor(&principal),
        )
        .await
        .map_err(opened_order_edit_error)?;
    state.production_maps.notify_live();
    Ok(json_response(saved))
}
