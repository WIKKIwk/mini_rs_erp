pub async fn production_map_progress_qr_reprint(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability_or_werka(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::ApparatusQueueRead,
            Capability::ApparatusQueueManage,
            Capability::WerkaAccess,
        ],
    )
    .await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: ProgressQrReprintRequest = parse_json(&body)?;
    let qr_payload = effective_progress_qr_payload(&input.qr_payload, &input.progress_qr);
    let batch = match super::super::training::training_progress_batch_for_qr(
        &state,
        &principal,
        &input.progress_batch_id,
        qr_payload,
    )
    .await
    .map_err(super::super::training::training_workspace_error)?
    {
        Some(batch) => batch,
        None => state
            .production_maps
            .progress_batch_for_qr(&input.progress_batch_id, qr_payload)
            .await
            .map_err(production_map_error)?,
    };
    if principal.role == PrincipalRole::RezkaMaster {
        let allowed = state.principal_assigned_apparatus(&principal).await;
        if !allowed.contains(&batch.apparatus) {
            return Err(forbidden());
        }
    } else if !principal_can_reprint_progress_batch(&principal, &batch) {
        return Err(forbidden());
    }
    let apparatus_display_name = state
        .production_maps
        .resolve_canonical_apparatus_text(&batch.apparatus)
        .await
        .map_err(production_map_error)?
        .runtime
        .display
        .display_name
        .clone();
    let order_map = state.production_maps.raw_map(&batch.order_id).await
        .map_err(production_map_error)?;
    let item_name = match order_map.as_ref() {
        _ if batch.wip_status == OrderProgressBatchWipStatus::Peremotka => batch.label_item_name.clone(),
        Some(order_map) if !order_map.products.is_empty() => batch.label_item_name.clone(),
        Some(order_map) => crate::core::production_map::progress_label_item_name(
            order_map,
            &batch.apparatus,
            batch.action,
        ),
        None => batch.label_item_name.clone(),
    };
    let mut request = progress_reprint_request(&input, &batch, &item_name, &apparatus_display_name);
    if input.automatic_rezka_labels {
        request.print_count = automatic_rezka_label_count(order_map.as_ref(), &batch);
    }
    let print_result = if input.print_transport.trim().eq_ignore_ascii_case("offline") {
        state.gscale.prepare_progress_label(request)
    } else {
        state.gscale.print_progress_label(request).await
    };
    let mut print = match print_result {
        Ok(response) => serde_json::to_value(response).unwrap_or(serde_json::Value::Null),
        Err(error) => {
            tracing::warn!(
                error = %error,
                qr_payload = %batch.qr_payload,
                batch_id = %batch.batch_id,
                order_id = %batch.order_id,
                "progress qr reprint failed"
            );
            progress_print_failure_json(error)
        }
    };
    if input.print_transport.trim().eq_ignore_ascii_case("offline")
        && print.get("ok").and_then(serde_json::Value::as_bool) == Some(true)
    {
        if let Some(code) = order_map.as_ref().and_then(production_map_print_order_code) {
            print["order_code"] = serde_json::json!(code);
        }
    }
    let ok = print
        .get("ok")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    Ok(json_response(serde_json::json!({
        "ok": ok,
        "batch": batch,
        "print": print,
    })))
}

fn progress_history_scope(
    principal: &Principal,
    query: &ProgressQrHistoryQuery,
) -> Result<(Vec<String>, String), AdminError> {
    if matches!(
        principal.role,
        PrincipalRole::Admin | PrincipalRole::Brigader | PrincipalRole::SalesManager
    ) {
        let refs = query
            .worker_ref
            .split(',')
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>();
        if refs.is_empty() {
            return Err(bad_request("worker_ref is required"));
        }
        return Ok((refs, String::new()));
    }
    let principal_ref = principal.ref_.trim().to_string();
    if principal_ref.is_empty() {
        return Err(forbidden());
    }
    Ok((vec![principal_ref], String::new()))
}

fn principal_can_reprint_progress_batch(
    principal: &Principal,
    batch: &crate::core::production_map::OrderProgressBatch,
) -> bool {
    let principal_ref = principal.ref_.trim();
    principal.role.is_admin_workspace_manager()
        || principal.role == PrincipalRole::Werka
        || principal.role == PrincipalRole::Aparatchi
        || (!principal_ref.is_empty() && batch.worker_ref.trim() == principal_ref)
        || (batch.order_id.trim().starts_with("training-")
            && batch
                .payload_json
                .get("training")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false))
}

fn progress_reprint_request(
    input: &ProgressQrReprintRequest,
    batch: &crate::core::production_map::OrderProgressBatch,
    item_name: &str,
    apparatus_display_name: &str,
) -> ProgressLabelPrintRequest {
    ProgressLabelPrintRequest {
        driver_url: input.driver_url.clone(),
        qr_payload: batch.qr_payload.clone(),
        item_code: batch.label_item_code.clone(),
        item_name: item_name.to_string(),
        apparatus: batch.apparatus.clone(),
        apparatus_display_name: apparatus_display_name.trim().to_string(),
        customer_name: batch
            .payload_json
            .get("customer_name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string(),
        executor_name: batch.executor_name.clone(),
        printer: input.printer.clone(),
        print_mode: input.print_mode.clone(),
        gross_qty: batch
            .payload_json
            .get("gross_qty")
            .and_then(serde_json::Value::as_f64)
            .or(batch.finished_goods_kg)
            .unwrap_or(batch.produced_qty),
        tare_enabled: batch.bobina_kg.is_some_and(|value| value > 0.0),
        tare_kg: batch.bobina_kg.unwrap_or(0.0),
        include_bobina_in_net: batch.payload_json
            .get("include_bobina_in_net")
            .and_then(serde_json::Value::as_bool) == Some(true),
        progress_qty: batch.finished_goods_meter.unwrap_or(batch.produced_qty),
        unit: "kg".to_string(),
        progress_unit: if batch.uom.trim().is_empty() {
            "m".to_string()
        } else {
            batch.uom.clone()
        },
        label_kind: "progress".to_string(),
        print_count: input.print_count,
    }
}
