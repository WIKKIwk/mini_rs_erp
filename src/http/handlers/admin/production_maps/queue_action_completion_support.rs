fn zero_completion_metric_codes(
    input: &ApparatusQueueActionRequest,
    return_ink_kg: Option<f64>,
) -> Vec<String> {
    if !matches!(input.action, queue_state::ApparatusQueueAction::Complete) {
        return Vec::new();
    }
    [
        ("produced_qty", input.produced_qty.or(input.qty)),
        ("gross_qty", input.gross_qty),
        ("return_ink_kg", return_ink_kg),
        (
            "lamination_print_leftover_rolls",
            input.lamination_print_leftover_rolls,
        ),
        (
            "lamination_film_leftover_rolls",
            input.lamination_film_leftover_rolls,
        ),
        ("rezka_bosma_waste", input.rezka_bosma_waste),
        ("rezka_lamination_waste", input.rezka_lamination_waste),
        ("rezka_edge_waste", input.rezka_edge_waste),
        ("total_waste", input.total_waste),
        ("finished_goods_kg", input.finished_goods_kg),
        ("finished_goods_meter", input.finished_goods_meter),
    ]
    .into_iter()
    .filter_map(|(code, value)| {
        value
            .is_some_and(|value| value == 0.0)
            .then_some(code.to_string())
    })
    .collect()
}

async fn prepare_qolip_for_bosma_start(
    state: &AppState,
    principal: &Principal,
    input: &ApparatusQueueActionRequest,
) -> Result<Option<crate::core::qolip::QolipOrderStartPreparation>, AdminError> {
    if !apparatus_requires_qolip_scan(&input.apparatus) {
        return Ok(None);
    }
    let Some(map) = state
        .production_maps
        .raw_map(&input.order_id)
        .await
        .map_err(production_map_error)?
    else {
        return Err(production_map_error(ProductionMapError::MapNotFound));
    };
    let qolip_code = input.qolip_code.trim();
    if qolip_code.is_empty() {
        return Err(bad_request("qolip_scan_required"));
    }
    let preparation = state
        .qolip
        .prepare_qolip_code_for_order_start(
            qolip_code,
            &map.product_code,
            &map.title,
            &principal.ref_,
            &principal.display_name,
            principal,
        )
        .await
        .map_err(qolip_queue_error)?;
    reject_qolip_in_use(
        state,
        &input.apparatus,
        &input.order_id,
        &preparation.spec.qolip_code,
    )
    .await?;
    Ok(Some(preparation))
}

pub(super) async fn reject_qolip_in_use(
    state: &AppState,
    apparatus: &str,
    order_id: &str,
    qolip_code: &str,
) -> Result<(), AdminError> {
    let active = state
        .production_maps
        .active_order_run_session_for_qolip(qolip_code)
        .await
        .map_err(production_map_error)?;
    if active.is_some_and(|session| {
        session.order_id.trim() != order_id.trim()
            || !queue_state::apparatus_titles_match(&session.apparatus, apparatus)
    }) {
        return Err(production_map_error(ProductionMapError::QolipAlreadyInUse));
    }
    Ok(())
}

pub(super) fn apparatus_requires_qolip_scan(apparatus: &str) -> bool {
    pechat::pechat_color_count(apparatus).is_some()
}

pub(super) fn qolip_queue_error(error: crate::core::qolip::QolipError) -> AdminError {
    match error {
        crate::core::qolip::QolipError::MissingQolipCode => bad_request("qolip_scan_required"),
        crate::core::qolip::QolipError::QolipCodeNotFound => bad_request("qolip_code_not_found"),
        crate::core::qolip::QolipError::QolipCodeMismatch => bad_request("qolip_code_mismatch"),
        crate::core::qolip::QolipError::LocationNotFound => bad_request("qolip_location_not_found"),
        crate::core::qolip::QolipError::InsufficientStock => bad_request("insufficient_stock"),
        crate::core::qolip::QolipError::LocationIdentityMismatch => {
            bad_request("location_identity_mismatch")
        }
        crate::core::qolip::QolipError::StoreFailed => server_error("qolip store failed"),
        other => bad_request(other.to_string()),
    }
}

pub(super) fn progress_print_failure_json(
    error: crate::core::gscale::GscaleServiceError,
) -> serde_json::Value {
    let (code, detail) = match error {
        crate::core::gscale::GscaleServiceError::PrintFailed { detail, .. } => {
            ("print_failed", clean_progress_print_error(&detail))
        }
        crate::core::gscale::GscaleServiceError::NotConfigured(_) => (
            "scale_driver_not_configured",
            "scale_driver_not_configured".to_string(),
        ),
        other => (other.code(), other.to_string()),
    };
    serde_json::json!({
        "ok": false,
        "status": "failed",
        "code": code,
        "error": detail,
    })
}

pub(super) fn clean_progress_print_error(detail: &str) -> String {
    detail
        .trim()
        .strip_prefix("driver request failed: ")
        .unwrap_or_else(|| detail.trim())
        .to_string()
}

