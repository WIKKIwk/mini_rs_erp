async fn execute_queue_action(
    state: &AppState,
    principal: &Principal,
    input: QueueActionCommand,
    apparatus: &QueueApparatusMetadata,
    assigned_apparatus: Vec<String>,
    state_material_barcodes: Vec<String>,
    returned_paint_report: Option<crate::core::returned_paint::ReturnedPaintRequest>,
) -> Result<Response, AdminError> {
    let qolip_preparations = if matches!(input.action, queue_state::ApparatusQueueAction::Start) {
        prepare_qolips_for_bosma_start(state, principal, &input, apparatus).await?
    } else {
        Vec::new()
    };
    let qolip_validation = TrustedQolipStartValidation::from_preparations(
        &apparatus.id,
        &input.order_id,
        &qolip_preparations,
    );
    let frame_specific_metrics = !input.progress.rezka_frames.is_empty();
    let recording_rezka_frame = input.progress.rezka_record_frame_index.is_some();
    let fallback_gross_qty = input.progress.gross_qty.or(input.progress.finished_goods_kg);
    let fallback_bobina_kg = input.progress.bobina_kg;
    let QueueActionCommand {
        apparatus: input_apparatus,
        order_id,
        action,
        materials,
        progress,
        completion: _,
        print,
    } = input;
    let mut prepared = state
        .production_maps
        .prepare_apparatus_queue_action_with_material_scan_and_progress(
            MaterialScanProgressAction {
                apparatus: &input_apparatus,
                order_id: &order_id,
                action,
                assigned_apparatus: &assigned_apparatus,
                actor: queue_action_actor(principal),
                material_barcode: &materials.combined_barcode,
                state_material_barcodes: &state_material_barcodes,
                progress,
                qolip_validation,
            },
        )
        .await
        .map_err(production_map_error)?;
    if !qolip_preparations.is_empty() {
        prepared.attach_qolip_codes(
            &qolip_preparations
                .iter()
                .map(|preparation| preparation.spec.qolip_code.clone())
                .collect::<Vec<_>>(),
        );
    }
    let qolip_checkouts = qolip_preparations
        .into_iter()
        .filter_map(|preparation| preparation.checkout)
        .collect::<Vec<_>>();
    let mut raw_material_stock_transitions = Vec::new();
    if matches!(action, queue_state::ApparatusQueueAction::Start) {
        let material_stock_barcodes = materials
            .combined_barcode
            .split(',')
            .map(|barcode| barcode.trim().to_string())
            .filter(|barcode| !barcode.is_empty())
            .collect::<Vec<_>>();
        if !prepared.material_scan_skipped() && !material_stock_barcodes.is_empty() {
            raw_material_stock_transitions.push(RawMaterialStockTransition::new(
                RawMaterialStockTransitionKind::InUse,
                material_stock_barcodes,
                &order_id,
            ));
        }
    }
    let completed_material_barcodes =
        if matches!(action, queue_state::ApparatusQueueAction::Complete) {
            raw_material_barcodes_for_order_apparatus(state, &order_id, &input_apparatus)
                .await?
        } else {
            Vec::new()
        };
    if !completed_material_barcodes.is_empty() {
        raw_material_stock_transitions.push(RawMaterialStockTransition::new(
            RawMaterialStockTransitionKind::Complete,
            completed_material_barcodes,
            &order_id,
        ));
    }
    // A card first commits its record, then prints that exact QR through the
    // reprint endpoint. A printer failure must never repeat the business write.
    let print_requests = if action.records_progress_output() && !recording_rezka_frame {
        prepared
            .progress_output_batches()
            .iter()
            .map(|batch| ProgressLabelPrintRequest {
                driver_url: print.driver_url.clone(),
                qr_payload: batch.qr_payload.clone(),
                item_code: batch.label_item_code.clone(),
                item_name: batch.label_item_name.clone(),
                apparatus: batch.apparatus.clone(),
                apparatus_display_name: apparatus.display_name.clone(),
                customer_name: print.customer_name.trim().to_string(),
                executor_name: batch.executor_name.clone(),
                printer: print.printer.clone(),
                print_mode: print.print_mode.clone(),
                gross_qty: if frame_specific_metrics {
                    batch
                        .payload_json
                        .get("gross_qty")
                        .and_then(serde_json::Value::as_f64)
                        .or(batch.finished_goods_kg)
                        .unwrap_or(batch.produced_qty)
                } else {
                    fallback_gross_qty.unwrap_or(batch.produced_qty)
                },
                tare_enabled: if frame_specific_metrics {
                    batch.bobina_kg.is_some_and(|value| value > 0.0)
                } else {
                    fallback_bobina_kg.is_some_and(|value| value > 0.0)
                },
                tare_kg: if frame_specific_metrics {
                    batch.bobina_kg.unwrap_or(0.0)
                } else {
                    fallback_bobina_kg.unwrap_or(0.0)
                },
                progress_qty: if frame_specific_metrics {
                    batch.finished_goods_meter.unwrap_or(batch.produced_qty)
                } else {
                    batch.produced_qty
                },
                unit: "kg".to_string(),
                progress_unit: if batch.uom.trim().is_empty() {
                    "m".to_string()
                } else {
                    batch.uom.clone()
                },
                label_kind: "progress".to_string(),
                print_count: if frame_specific_metrics {
                    1
                } else {
                    print.print_count
                },
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let result = state
        .production_maps
        .commit_prepared_queue_action_with_raw_material_stock(
            prepared,
            raw_material_stock_transitions.clone(),
            qolip_checkouts,
            returned_paint_report,
        )
        .await
        .map_err(production_map_error)?;
    let mut result = result;
    let mut warehouse_stock_update_warehouses =
        std::mem::take(&mut result.raw_material_stock_warehouses);
    if !raw_material_stock_transitions.is_empty() && !result.raw_material_stock_committed {
        for transition in &raw_material_stock_transitions {
            let updates = match transition.kind {
                RawMaterialStockTransitionKind::InUse => {
                    state
                        .gscale
                        .mark_raw_material_stock_in_use(&transition.barcodes, &transition.order_id)
                        .await
                }
                RawMaterialStockTransitionKind::Consumed => {
                    state
                        .gscale
                        .mark_raw_material_stock_consumed(
                            &transition.barcodes,
                            &transition.order_id,
                        )
                        .await
                }
                RawMaterialStockTransitionKind::Complete => {
                    let warehouses = settle_completion_raw_materials_fallback(
                        state,
                        &transition.order_id,
                        &transition.barcodes,
                    )
                    .await?;
                    warehouse_stock_update_warehouses.extend(warehouses);
                    continue;
                }
            }
            .map_err(raw_material_stock_status_error)?;
            warehouse_stock_update_warehouses.extend(
                updates
                    .into_iter()
                    .map(|stock| stock.warehouse)
                    .filter(|warehouse| !warehouse.trim().is_empty()),
            );
        }
    }
    for warehouse in warehouse_stock_update_warehouses {
        state
            .warehouse_events
            .notify_updated(&warehouse, "raw_material_stock");
    }
    let prints = dispatch_progress_label_prints(
        state.gscale.clone(),
        print_requests,
        &print.print_transport,
        &input_apparatus,
        &order_id,
        action,
    );
    let print = prints.first().cloned().unwrap_or(serde_json::Value::Null);
    let order_control = result.order_control;
    let mut response = serde_json::json!({
        "ok": true,
        "states": result.states,
        "order_status": result.order_status,
        "session": result.session,
        "progress_event": result.progress_event,
        "progress_batch": result.progress_batch,
        "progress_batches": result.progress_batches,
        "print": print,
        "prints": prints,
    });
    if let Some(order_control) = order_control {
        response["order_control"] = serde_json::json!(order_control);
    }
    Ok(json_response(response))
}
