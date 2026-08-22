
fn training_queue_action_controls(
    apparatus: &str,
    canonical: &RuntimeApparatusConfiguration,
    sequence: &[String],
    states: &BTreeMap<String, String>,
    maps: &[ProductionMapSaved],
    input_progress_batches: &BTreeMap<String, Vec<OrderProgressBatch>>,
) -> BTreeMap<String, ApparatusQueueOrderActionControl> {
    let parsed_states = sequence
        .iter()
        .map(|order_id| {
            (
                order_id.clone(),
                states
                    .get(order_id)
                    .and_then(|state| queue_state::ApparatusQueueOrderState::parse(state))
                    .unwrap_or(queue_state::ApparatusQueueOrderState::Pending),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let active_order_id = parsed_states
        .iter()
        .find_map(|(order_id, state)| state.is_active().then_some(order_id.as_str()));
    let actionable_order_id = queue_state::first_actionable_order_id(sequence, &parsed_states);
    sequence
        .iter()
        .map(|order_id| {
            let state = parsed_states
                .get(order_id)
                .copied()
                .unwrap_or(queue_state::ApparatusQueueOrderState::Pending);
            let active_order_is_this = active_order_id.is_none_or(|active| active == order_id);
            let queue_actionable = state.is_active()
                || (state == queue_state::ApparatusQueueOrderState::Pending
                    && active_order_is_this
                    && actionable_order_id.as_deref() == Some(order_id));
            let previous_stage = maps
                .iter()
                .find(|saved| saved.map.id.trim() == order_id.trim())
                .and_then(|saved| training_input_stage_for_map(&saved.map, apparatus))
                .unwrap_or_default();
            let input_batches = input_progress_batches
                .get(order_id.trim())
                .map(Vec::as_slice)
                .unwrap_or_default();
            let current_input_batch_id = input_batches
                .iter()
                .find(|batch| {
                    batch.wip_status == OrderProgressBatchWipStatus::InUse
                        && training_input_batch_matches(batch, order_id, &previous_stage, apparatus)
                        && canonical_apparatus_matches(&batch.used_by_apparatus, apparatus)
                })
                .map(|batch| batch.batch_id.as_str())
                .unwrap_or_default();
            let has_unprocessed_previous_wips = !previous_stage.is_empty()
                && training_has_unprocessed_previous_wips(
                    input_batches,
                    order_id,
                    &previous_stage,
                    apparatus,
                    current_input_batch_id,
                );
            let previous_stage_ready = previous_stage.is_empty()
                || input_batches.iter().any(|batch| {
                    training_input_batch_matches(batch, order_id, &previous_stage, apparatus)
                });
            let previous_wip_mode = if previous_stage.is_empty() {
                ApparatusQueuePreviousWipMode::NotRequired
            } else if previous_stage_ready {
                ApparatusQueuePreviousWipMode::ScanRequired
            } else {
                ApparatusQueuePreviousWipMode::Waiting
            };
            let pending_actionable =
                queue_actionable && previous_wip_mode != ApparatusQueuePreviousWipMode::Waiting;
            let allowed_actions = if !queue_actionable {
                Vec::new()
            } else {
                match state {
                    queue_state::ApparatusQueueOrderState::Pending => {
                        if pending_actionable {
                            vec![queue_state::ApparatusQueueAction::Start]
                        } else {
                            Vec::new()
                        }
                    }
                    queue_state::ApparatusQueueOrderState::InProgress => {
                        let mut actions = vec![
                            queue_state::ApparatusQueueAction::Pause,
                            queue_state::ApparatusQueueAction::DetachRoll,
                            queue_state::ApparatusQueueAction::Complete,
                        ];
                        if maps
                            .iter()
                            .find(|saved| saved.map.id.trim() == order_id)
                            .is_some_and(|saved| is_rezka_apparatus(&saved.map, apparatus))
                        {
                            actions.push(queue_state::ApparatusQueueAction::RollComplete);
                        }
                        actions
                    }
                    queue_state::ApparatusQueueOrderState::Paused => {
                        vec![queue_state::ApparatusQueueAction::Resume]
                    }
                    queue_state::ApparatusQueueOrderState::Frozen => Vec::new(),
                    queue_state::ApparatusQueueOrderState::Completed => Vec::new(),
                }
            };
            let interaction = match state {
                queue_state::ApparatusQueueOrderState::Pending if !queue_actionable => {
                    ApparatusQueueWorkerInteraction {
                        mode: ApparatusQueueInteractionMode::FreshStartBlocked,
                        assigned_materials_display_only: true,
                        blocking_reason_code: "waiting_sequence".to_string(),
                        ..ApparatusQueueWorkerInteraction::default()
                    }
                }
                queue_state::ApparatusQueueOrderState::Pending
                    if previous_wip_mode == ApparatusQueuePreviousWipMode::Waiting =>
                {
                    ApparatusQueueWorkerInteraction {
                        mode: ApparatusQueueInteractionMode::WaitingPreviousStage,
                        assigned_materials_display_only: true,
                        previous_wip_mode,
                        blocking_reason_code: "waiting_previous_stage".to_string(),
                        ..ApparatusQueueWorkerInteraction::default()
                    }
                }
                queue_state::ApparatusQueueOrderState::Pending => ApparatusQueueWorkerInteraction {
                    mode: ApparatusQueueInteractionMode::FreshStart,
                    assigned_materials_display_only: false,
                    previous_wip_mode,
                    qolip_mode: if pechat::is_pechat_apparatus(canonical) {
                        ApparatusQueueQolipMode::ScanRequired
                    } else {
                        ApparatusQueueQolipMode::NotRequired
                    },
                    ..ApparatusQueueWorkerInteraction::default()
                },
                queue_state::ApparatusQueueOrderState::InProgress => {
                    ApparatusQueueWorkerInteraction {
                        mode: ApparatusQueueInteractionMode::InProgress,
                        material_intake_allowed: true,
                        ..ApparatusQueueWorkerInteraction::default()
                    }
                }
                queue_state::ApparatusQueueOrderState::Paused => ApparatusQueueWorkerInteraction {
                    mode: ApparatusQueueInteractionMode::Paused,
                    material_intake_allowed: true,
                    ..ApparatusQueueWorkerInteraction::default()
                },
                queue_state::ApparatusQueueOrderState::Frozen => ApparatusQueueWorkerInteraction {
                    mode: ApparatusQueueInteractionMode::Frozen,
                    assigned_materials_display_only: true,
                    blocking_reason_code: "order_frozen".to_string(),
                    ..ApparatusQueueWorkerInteraction::default()
                },
                queue_state::ApparatusQueueOrderState::Completed => {
                    ApparatusQueueWorkerInteraction {
                        mode: ApparatusQueueInteractionMode::Completed,
                        assigned_materials_display_only: true,
                        ..ApparatusQueueWorkerInteraction::default()
                    }
                }
            };
            (
                order_id.clone(),
                ApparatusQueueOrderActionControl {
                    state,
                    allowed_actions,
                    interaction,
                    previous_stage,
                    previous_stage_ready,
                    complete_requires_full_report: maps
                        .iter()
                        .find(|saved| saved.map.id.trim() == order_id)
                        .is_some_and(|saved| {
                            training_complete_requires_full_report(
                                &saved.map,
                                apparatus,
                                has_unprocessed_previous_wips,
                            )
                        }),
                    freeze_request: None,
                },
            )
        })
        .collect()
}

fn training_complete_requires_full_report(
    map: &ProductionMapDefinition,
    apparatus: &str,
    has_unprocessed_previous_wips: bool,
) -> bool {
    !(is_laminatsiya_apparatus(map, apparatus) || is_rezka_apparatus(map, apparatus))
        || !has_unprocessed_previous_wips
}

fn training_order_status(
    state: queue_state::ApparatusQueueOrderState,
) -> ProductionOrderStatusDetail {
    let status = state.as_str().to_string();
    ProductionOrderStatusDetail {
        order_status: status.clone(),
        work_status: status.clone(),
        flow_status: status,
        ..ProductionOrderStatusDetail::default()
    }
}

pub async fn training_production_maps(
    State(state): State<AppState>,
    Query(query): Query<TrainingMapsQuery>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    let store = training_store(&state)?;
    match method {
        Method::GET => {
            if !query.id.trim().is_empty() {
                let saved = store
                    .map(&query.id)
                    .await
                    .map_err(training_workspace_error)?
                    .ok_or_else(|| not_found("training_map_not_found"))?;
                return Ok(json_response(saved));
            }
            let maps = store.maps().await.map_err(training_workspace_error)?;
            Ok(json_response(maps))
        }
        Method::DELETE => {
            let order_id = query.id.trim();
            if order_id.is_empty() {
                return Err(bad_request("training order id kerak"));
            }
            store
                .delete_order(order_id)
                .await
                .map_err(training_workspace_error)?;
            Ok(json_response(serde_json::json!({
                "ok": true,
                "id": order_id,
            })))
        }
        Method::PUT => {
            let map: ProductionMapDefinition = parse_json(&body)?;
            let saved = store
                .save_map(map)
                .await
                .map_err(training_workspace_error)?;
            Ok(json_response(saved))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn training_input_batches(
    State(state): State<AppState>,
    Query(query): Query<TrainingInputBatchesQuery>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    let store = training_store(&state)?;
    match method {
        Method::GET => {
            let maps = if query.order_id.trim().is_empty() {
                store.maps().await.map_err(training_workspace_error)?
            } else {
                store
                    .map(query.order_id.trim())
                    .await
                    .map_err(training_workspace_error)?
                    .into_iter()
                    .collect()
            };
            let mut batches = Vec::new();
            for saved in maps {
                let apparatus = if query.apparatus.trim().is_empty() {
                    training_input_target_apparatus(&saved.map).unwrap_or_default()
                } else {
                    query.apparatus.trim().to_string()
                };
                if apparatus.is_empty() {
                    continue;
                }
                if training_input_stage_for_map(&saved.map, &apparatus).is_none() {
                    continue;
                }
                batches.extend(
                    training_generated_input_progress_batches_for_map(
                        store, &saved.map, &apparatus,
                    )
                    .await
                    .map_err(training_workspace_error)?
                    .into_iter()
                    .filter(|batch| {
                        query.qr_payload.trim().is_empty()
                            || batch
                                .qr_payload
                                .eq_ignore_ascii_case(query.qr_payload.trim())
                    }),
                );
            }
            Ok(json_response(serde_json::json!({"batches": batches})))
        }
        Method::POST => {
            let input: TrainingInputBatchRequest = parse_json(&body)?;
            let order_id = input.order_id.trim();
            if order_id.is_empty() || !order_id.starts_with("training-") {
                return Err(bad_request("training order id kerak"));
            }
            let saved = store
                .map(order_id)
                .await
                .map_err(training_workspace_error)?
                .ok_or_else(|| not_found("training_map_not_found"))?;
            let apparatus = if input.apparatus.trim().is_empty() {
                training_input_target_apparatus(&saved.map).unwrap_or_default()
            } else {
                input.apparatus.trim().to_string()
            };
            let Some(previous_stage) = training_input_stage_for_map(&saved.map, &apparatus) else {
                return Err(bad_request("training_input_batch_not_applicable"));
            };
            let count = input.count.unwrap_or(1);
            if count == 0 || count > 100 {
                return Err(bad_request("training_input_batch_count_invalid"));
            }
            let queue_started = store
                .queue_states()
                .await
                .map_err(training_workspace_error)?
                .iter()
                .any(|(configured_apparatus, states)| {
                    canonical_apparatus_matches(configured_apparatus, &apparatus)
                        && states
                            .get(order_id)
                            .and_then(|state| queue_state::ApparatusQueueOrderState::parse(state))
                            .is_some_and(|state| {
                                state != queue_state::ApparatusQueueOrderState::Pending
                            })
                });
            let input_set_started = store
                .training_input_batch_set_started(order_id, &apparatus)
                .await
                .map_err(training_workspace_error)?;
            if queue_started || input_set_started {
                return Err(bad_request("training_input_batch_set_closed"));
            }
            let identities = store
                .generate_training_input_batches(order_id, &apparatus, &previous_stage, count)
                .await
                .map_err(training_workspace_error)?;
            let batches = identities
                .iter()
                .filter_map(|identity| {
                    training_input_progress_batch(&saved.map, &apparatus, identity)
                })
                .collect::<Vec<_>>();
            if batches.len() != identities.len() {
                return Err(bad_request("training_input_batch_not_applicable"));
            }
            store
                .put_training_progress_batches(&batches)
                .await
                .map_err(training_workspace_error)?;
            state.production_maps.notify_live();
            Ok(json_response(serde_json::json!({
                "ok": true,
                "batch": batches.first(),
                "batches": batches,
            })))
        }
        Method::DELETE => {
            let order_id = query.order_id.trim();
            if order_id.is_empty() || !order_id.starts_with("training-") {
                return Err(bad_request("training order id kerak"));
            }
            let deleted = store
                .delete_training_input_batch(order_id, &query.apparatus, &query.qr_payload)
                .await
                .map_err(training_workspace_error)?;
            if deleted.is_empty() {
                return Err(not_found("training_input_batch_not_found"));
            }
            state.production_maps.notify_live();
            Ok(json_response(serde_json::json!({
                "ok": true,
                "order_id": order_id,
            })))
        }
        _ => Err(method_not_allowed()),
    }
}
