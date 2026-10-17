use super::*;

use crate::core::production_map::queue_state;

#[derive(serde::Deserialize)]
struct BosmaAstatkaRequest {
    apparatus: String,
    order_id: String,
    total_waste: f64,
    finished_goods_meter: Option<f64>,
    finished_goods_kg: Option<f64>,
    #[serde(alias = "babina_kg")]
    bobina_kg: Option<f64>,
    #[serde(default)]
    returned_paint_items: Vec<crate::core::returned_paint::ReturnedPaintItem>,
    #[serde(default)]
    returned_paint_image_id: String,
    #[serde(default)]
    description: String,
}

pub async fn production_map_bosma_astatka(
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
            Capability::ApparatusQueueManage,
        ],
    )
    .await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: BosmaAstatkaRequest = parse_json(&body)?;
    let apparatus = input.apparatus.trim().to_string();
    let order_id = input.order_id.trim().to_string();
    if !state
        .admin
        .principal_has_capability(&principal, Capability::AdminAccess)
        .await
    {
        let assigned = state.admin.principal_assigned_apparatus(&principal).await;
        if !queue_state::apparatus_matches_assigned(&apparatus, &assigned) {
            return Err(production_map_error(
                ProductionMapError::ApparatusNotAssigned,
            ));
        }
    }
    let map = state
        .production_maps
        .raw_map(&order_id)
        .await
        .map_err(production_map_error)?
        .ok_or_else(|| production_map_error(ProductionMapError::MapNotFound))?;
    // Validate paint values and image ownership without creating a stock/queue action.
    let returned_paint = state
        .returned_paint
        .prepare_request(
            crate::core::returned_paint::ReturnedPaintRequestCreate {
                order_id: order_id.clone(),
                order_code: map.code,
                order_name: map.title,
                apparatus: apparatus.clone(),
                image_id: input.returned_paint_image_id,
                items: input.returned_paint_items,
            },
            &principal,
            "bosma-astatka".to_string(),
        )
        .await
        .map_err(|error| bad_request(error.to_string()))?;
    let report = state
        .production_maps
        .record_bosma_astatka(crate::core::production_map::BosmaAstatkaReport {
            report_id: String::new(),
            order_id,
            apparatus,
            from_at_unix: 0,
            to_at_unix: 0,
            total_waste: input.total_waste,
            finished_goods_meter: input.finished_goods_meter,
            finished_goods_kg: input.finished_goods_kg,
            bobina_kg: input.bobina_kg,
            returned_paint,
            description: input.description,
        })
        .await
        .map_err(production_map_error)?;
    Ok(json_response(
        serde_json::json!({"ok": true, "report": report}),
    ))
}

#[derive(Default, serde::Deserialize)]
struct LaminatsiyaAstatkaRequest {
    #[serde(default)]
    action: Option<queue_state::ApparatusQueueAction>,
    #[serde(default)]
    full_completion_report_required: bool,
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    lamination_print_leftover_rolls: Option<f64>,
    #[serde(default)]
    lamination_film_leftover_rolls: Option<f64>,
    #[serde(default)]
    total_waste: Option<f64>,
    #[serde(default)]
    finished_goods_meter: Option<f64>,
    #[serde(default)]
    finished_goods_kg: Option<f64>,
    #[serde(default, alias = "babina_kg")]
    bobina_kg: Option<f64>,
    #[serde(default)]
    description: String,
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    request_id: String,
    #[serde(default)]
    layer_remainders: Option<Vec<crate::core::production_map::LaminatsiyaAstatkaLayerInput>>,
    #[serde(flatten)]
    print: super::layer_remainders::LayerRemainderPrintOptions,
}

#[derive(serde::Deserialize)]
struct LaminatsiyaAstatkaQuery {
    apparatus: String,
    order_id: String,
    #[serde(default)]
    request_id: String,
}

#[derive(Default, serde::Deserialize)]
struct RezkaAstatkaRequest {
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    stage_node_id: String,
    #[serde(default)]
    total_waste: Option<f64>,
    #[serde(default)]
    rezka_bosma_waste: Option<f64>,
    #[serde(default)]
    rezka_lamination_waste: Option<f64>,
    #[serde(default)]
    rezka_edge_waste: Option<f64>,
    #[serde(default)]
    finished_goods_meter: Option<f64>,
    #[serde(default)]
    finished_goods_kg: Option<f64>,
    #[serde(default, alias = "babina_kg")]
    bobina_kg: Option<f64>,
    #[serde(default)]
    description: String,
}

pub async fn production_map_laminatsiya_astatka(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    uri: axum::http::Uri,
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
    if method != Method::GET && method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: LaminatsiyaAstatkaRequest = if method == Method::GET {
        let Query(query) = Query::<LaminatsiyaAstatkaQuery>::try_from_uri(&uri)
            .map_err(|_| bad_request("apparatus and order_id are required"))?;
        LaminatsiyaAstatkaRequest {
            apparatus: query.apparatus,
            order_id: query.order_id,
            request_id: query.request_id,
            ..Default::default()
        }
    } else {
        parse_json(&body)?
    };
    if input.apparatus.trim().is_empty() || input.order_id.trim().is_empty() {
        return Err(bad_request("apparatus and order_id are required"));
    }
    let is_admin = state
        .admin
        .principal_has_capability(&principal, Capability::AdminAccess)
        .await;
    let assigned_apparatus = if is_admin {
        vec![input.apparatus.trim().to_string()]
    } else {
        state.admin.principal_assigned_apparatus(&principal).await
    };
    if !is_admin {
        if !queue_state::apparatus_matches_assigned(&input.apparatus, &assigned_apparatus) {
            return Err(production_map_error(
                ProductionMapError::ApparatusNotAssigned,
            ));
        }
    }
    if method == Method::GET {
        let snapshot = state
            .production_maps
            .laminatsiya_astatka_snapshot(
                &input.apparatus,
                &input.order_id,
                &principal.ref_,
                &input.request_id,
            )
            .await
            .map_err(production_map_error)?;
        let mut response = serde_json::to_value(snapshot)
            .map_err(|_| production_map_error(ProductionMapError::StoreFailed))?;
        response["ok"] = serde_json::json!(true);
        if response["request_result"].is_object() {
            response["request_result"]["ok"] = serde_json::json!(true);
        }
        return Ok(json_response(response));
    }
    if let Some(layer_remainders) = input.layer_remainders {
        super::layer_remainders::validate_layer_print(&input.print)?;
        let required = |value: Option<f64>| {
            value.ok_or_else(|| {
                production_map_error(ProductionMapError::LaminatsiyaAstatkaMetricsRequired)
            })
        };
        let result = state
            .production_maps
            .record_laminatsiya_astatka_shift(
                crate::core::production_map::LaminatsiyaAstatkaShiftInput {
                    action: input.action.unwrap_or(queue_state::ApparatusQueueAction::Pause),
                    full_completion_report_required: input.full_completion_report_required,
                    apparatus: input.apparatus,
                    order_id: input.order_id,
                    session_id: input.session_id,
                    request_id: input.request_id,
                    lamination_print_leftover_rolls: required(
                        input.lamination_print_leftover_rolls,
                    )?,
                    lamination_film_leftover_rolls: required(input.lamination_film_leftover_rolls)?,
                    total_waste: required(input.total_waste)?,
                    finished_goods_meter: input.finished_goods_meter,
                    finished_goods_kg: input.finished_goods_kg,
                    bobina_kg: input.bobina_kg,
                    description: input.description,
                    layer_remainders,
                },
                queue_action_actor(&principal),
                &assigned_apparatus,
            )
            .await
            .map_err(production_map_error)?;
        return laminatsiya_astatka_shift_response(&state, result, &input.print).await;
    }
    if input.action.is_some_and(|action| action != queue_state::ApparatusQueueAction::Pause) {
        return Err(production_map_error(ProductionMapError::ProgressInputInvalid));
    }
    let report = state
        .production_maps
        .record_laminatsiya_astatka(
            &input.apparatus,
            &input.order_id,
            queue_action_actor(&principal),
            input.lamination_print_leftover_rolls,
            input.lamination_film_leftover_rolls,
            input.total_waste,
            input.finished_goods_meter,
            input.finished_goods_kg,
            input.bobina_kg,
            &input.description,
        )
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "report": report,
    })))
}

pub async fn production_map_rezka_astatka(
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
            Capability::ApparatusQueueManage,
        ],
    )
    .await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: RezkaAstatkaRequest = parse_json(&body)?;
    if input.apparatus.trim().is_empty() || input.order_id.trim().is_empty() {
        return Err(bad_request("apparatus and order_id are required"));
    }
    let is_admin = state
        .admin
        .principal_has_capability(&principal, Capability::AdminAccess)
        .await;
    if !is_admin {
        let assigned_apparatus = state.admin.principal_assigned_apparatus(&principal).await;
        if !queue_state::apparatus_matches_assigned(&input.apparatus, &assigned_apparatus) {
            return Err(production_map_error(
                ProductionMapError::ApparatusNotAssigned,
            ));
        }
    }
    let report = state
        .production_maps
        .record_rezka_astatka(
            &input.apparatus,
            &input.order_id,
            queue_action_actor(&principal),
            input.total_waste,
            input.rezka_bosma_waste,
            input.rezka_lamination_waste,
            input.rezka_edge_waste,
            input.finished_goods_meter,
            input.finished_goods_kg,
            input.bobina_kg,
            &input.description,
            &input.stage_node_id,
        )
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "report": report,
    })))
}

async fn laminatsiya_astatka_shift_response(
    state: &AppState,
    result: crate::core::production_map::LaminatsiyaAstatkaCommitResult,
    options: &super::layer_remainders::LayerRemainderPrintOptions,
) -> Result<Response, AdminError> {
    // Hardware is contacted after the atomic business commit. A failed label
    // retains its saved QR and does not turn a successful stock write into an
    // ambiguous failed submission.
    let mut prints = Vec::new();
    for batch in &result.progress_batches {
        let request = ProgressLabelPrintRequest {
            driver_url: options.driver_url.clone(),
            printer: options.printer.clone(),
            print_mode: options.print_mode.clone(),
            print_count: 1,
            qr_payload: batch.qr_payload.clone(),
            item_code: batch.label_item_code.clone(),
            item_name: batch.label_item_name.clone(),
            apparatus: batch.apparatus.clone(),
            apparatus_display_name: state
                .production_maps
                .resolve_canonical_apparatus_text(&batch.apparatus)
                .await
                .map(|canonical| canonical.runtime.display.display_name.clone())
                .unwrap_or_else(|_| batch.apparatus.clone()),
            customer_name: batch.payload_json["customer_name"]
                .as_str()
                .unwrap_or_default()
                .into(),
            executor_name: batch.executor_name.clone(),
            gross_qty: batch.payload_json["gross_qty"]
                .as_f64()
                .or(batch.finished_goods_kg)
                .unwrap_or(batch.produced_qty),
            tare_kg: batch.bobina_kg.unwrap_or(0.0),
            tare_enabled: batch.bobina_kg.is_some_and(|v| v > 0.0),
            progress_qty: batch.finished_goods_meter.unwrap_or(batch.produced_qty),
            unit: "kg".into(),
            progress_unit: "m".into(),
            label_kind: "progress".into(),
        };
        let printed = if options
            .print_transport
            .trim()
            .eq_ignore_ascii_case("offline")
        {
            state.gscale.prepare_progress_label(request)
        } else {
            state.gscale.print_progress_label(request).await
        };
        let mut print = match printed {
            Ok(value) => serde_json::to_value(value).unwrap_or(serde_json::Value::Null),
            Err(error) => super::queue_actions::progress_print_failure_json(error),
        };
        if print.is_object() {
            print["qr_payload"] = serde_json::json!(batch.qr_payload);
        }
        prints.push(print);
    }
    for remainder in &result.remainders {
        if let Some(stock) = &remainder.raw_material_stock {
            state
                .warehouse_events
                .notify_updated(&stock.warehouse, "layer_remainder_recorded");
        }
        if remainder.qr_payload.trim().is_empty() || remainder.remaining_net_kg <= 0.0 {
            continue;
        }
        let print =
            match super::layer_remainders::layer_remainder_print(state, remainder, options).await {
                Ok(print) => print,
                Err(_) => serde_json::json!({
                    "ok": false, "status": "error", "error": "layer_remainder_print_failed",
                    "qr_payload": remainder.qr_payload,
                }),
            };
        prints.push(print);
    }
    let mut response = serde_json::to_value(result)
        .map_err(|_| production_map_error(ProductionMapError::StoreFailed))?;
    response["ok"] = serde_json::json!(true);
    response["prints"] = serde_json::json!(prints);
    response["rev"] = serde_json::json!(state.production_maps.snapshot_revision());
    response["epoch"] = serde_json::json!(state.production_maps.snapshot_epoch());
    Ok(json_response(response))
}
