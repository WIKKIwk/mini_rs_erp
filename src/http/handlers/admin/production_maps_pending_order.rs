async fn complete_pending_order(
    state: &AppState,
    principal: &Principal,
    mut input: ProductionMapSaveWithOrderRequest,
) -> Result<Response, AdminError> {
    use super::pending_orders::{pending_error, pending_store};
    use crate::core::pending_orders::PendingOrderCompletion;
    let store = pending_store(state)?;
    let pending = store
        .get(&input.pending_order_id)
        .await
        .map_err(pending_error)?;
    if let Some(done) = pending.completion {
        return Ok(json_response(
            serde_json::json!({"ok":true,"saved":done.saved,"template":done.template}),
        ));
    }
    let mut template = pending.merge_completion(
        input
            .template
            .take()
            .ok_or_else(|| bad_request("template kerak"))?,
    );
    if !template.image_id.trim().is_empty() && template.image_id != pending.template.image_id {
        let image = state
            .calculate_orders
            .get_image(&principal_owner_key(principal), &template.image_id)
            .await
            .map_err(calculate_order_error)?
            .ok_or_else(|| bad_request("order image not found"))?;
        template.image_name = image.image_name;
        template.image_mime = image.image_mime;
        template.image_size_bytes = image.image_size_bytes;
    }
    validate_template(&template).map_err(calculate_order_error)?;
    input.map.id = pending.id.clone();
    input.map.code = template.order_number.clone();
    input.map.order_number = template.order_number.clone();
    input.map.product_code = template.item_code.clone();
    input.map.title = template.product.clone();
    input.map.customer_name = template.customer.clone();
    input.map.image_id = template.image_id.clone();
    let materials = state
        .calculate_materials
        .list()
        .await
        .map_err(|_| server_error("material catalog failed"))?;
    apply_authoritative_calculation(&mut input.map, &template, &materials)?;
    template.width_mm = input.map.width_mm.unwrap_or_default();
    let cut_ids = canonical_cut_apparatus_ids(state).await?;
    apply_order_rezka_kadr_count(&mut input.map, &template, &cut_ids);
    let saved = state
        .production_maps
        .prepare_map_for_save(input.map)
        .await
        .map_err(production_map_error)?;
    let mut source = saved.map.clone();
    source.id = format!("template-{:032x}", rand::random::<u128>());
    source.code.clear();
    source.order_number.clear();
    source.order_kg = None;
    source.base_length = None;
    template.source_map_id = source.id.clone();
    template = crate::db::postgres_calculate_order::stamp_template(template, None);
    let source = state
        .production_maps
        .prepare_map_for_save(source)
        .await
        .map_err(production_map_error)?;
    let (done, created) = store
        .complete(
            &pending.id,
            &principal_owner_key(principal),
            PendingOrderCompletion { saved, template },
            source,
        )
        .await
        .map_err(pending_error)?;
    if created {
        state.production_maps.notify_live();
        // The bot already delivered the intake. Do not send a duplicate order notification.
        let sheets = state.order_sheets.clone();
        let completed = done.clone();
        tokio::spawn(async move {
            if let Err(error) = sheets
                .append_order(&completed.saved.map, &completed.template)
                .await
            {
                tracing::warn!(?error, "completed Telegram order sheet append failed");
            }
        });
    }
    Ok(json_response(
        serde_json::json!({"ok":true,"saved":done.saved,"template":done.template}),
    ))
}
