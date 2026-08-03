fn returned_paint_queue_error(error: ReturnedPaintError) -> AdminError {
    match error {
        ReturnedPaintError::NegativeFinalValue => {
            bad_request("returned_paint_astatka_exceeds_rasxot")
        }
        other => bad_request(other.to_string()),
    }
}

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

fn rezka_queue_input_metrics_are_complete(
    input: &ApparatusQueueActionRequest,
    produced_qty: Option<f64>,
) -> bool {
    let is_positive = |value: Option<f64>| {
        value.is_some_and(|value| value.is_finite() && value > 0.0)
    };
    let has_output_meter = is_positive(produced_qty.or(input.finished_goods_meter));
    let has_output_kg = is_positive(input.gross_qty.or(input.finished_goods_kg));
    let has_waste = [
        input.total_waste,
        input.rezka_bosma_waste,
        input.rezka_lamination_waste,
        input.rezka_edge_waste,
    ]
    .into_iter()
    .any(is_positive);
    has_output_meter
        && has_output_kg
        && (input.action != queue_state::ApparatusQueueAction::Complete || has_waste)
}

async fn prepare_qolips_for_bosma_start(
    state: &AppState,
    principal: &Principal,
    input: &ApparatusQueueActionRequest,
) -> Result<Vec<crate::core::qolip::QolipOrderStartPreparation>, AdminError> {
    if !apparatus_requires_qolip_scan(&input.apparatus) {
        return Ok(Vec::new());
    }
    let Some(map) = state
        .production_maps
        .raw_map(&input.order_id)
        .await
        .map_err(production_map_error)?
    else {
        return Err(production_map_error(ProductionMapError::MapNotFound));
    };
    let qolip_codes = qolip_codes_for_start(input);
    if qolip_codes.is_empty() {
        return Err(bad_request("qolip_scan_required"));
    }
    let required_qolips = state
        .qolip
        .required_qolips_for_order(&map.product_code, &map.title)
        .await
        .map_err(qolip_queue_error)?;
    let mut preparations = Vec::with_capacity(qolip_codes.len());
    for qolip_code in &qolip_codes {
        reject_qolip_in_use(state, &input.apparatus, &input.order_id, &qolip_code).await?;
        let preparation = state
            .qolip
            .prepare_qolip_code_for_order_start(
                &qolip_code,
                &map.product_code,
                &map.title,
                &principal.ref_,
                &principal.display_name,
                principal,
            )
            .await
            .map_err(qolip_queue_error)?;
        preparations.push(preparation);
    }
    let scanned = qolip_codes
        .iter()
        .map(|code| code.trim().to_lowercase())
        .collect::<std::collections::BTreeSet<_>>();
    let required = required_qolips
        .iter()
        .map(|spec| spec.qolip_code.trim().to_lowercase())
        .collect::<std::collections::BTreeSet<_>>();
    if scanned != required {
        return Err(bad_request("qolip_scan_incomplete"));
    }
    Ok(preparations)
}

fn qolip_codes_for_start(input: &ApparatusQueueActionRequest) -> Vec<String> {
    let mut result = Vec::new();
    for code in input
        .qolip_codes
        .iter()
        .map(String::as_str)
        .chain(std::iter::once(input.qolip_code.as_str()))
    {
        let code = code.trim();
        if code.is_empty()
            || result
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(code))
        {
            continue;
        }
        result.push(code.to_string());
    }
    result
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
    pechat::is_pechat_apparatus(apparatus)
}

pub(super) fn qolip_queue_error(error: crate::core::qolip::QolipError) -> AdminError {
    match error {
        crate::core::qolip::QolipError::MissingQolipCode => bad_request("qolip_scan_required"),
        crate::core::qolip::QolipError::QolipCodeNotFound => bad_request("qolip_code_not_found"),
        crate::core::qolip::QolipError::QolipCodeMismatch => bad_request("qolip_code_mismatch"),
        crate::core::qolip::QolipError::CheckoutRequired => bad_request("qolip_checkout_required"),
        crate::core::qolip::QolipError::CheckoutAssignedToAnotherWorker => {
            bad_request("qolip_checkout_assigned_to_another_worker")
        }
        crate::core::qolip::QolipError::QolipInUse => bad_request("qolip_already_in_use"),
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
