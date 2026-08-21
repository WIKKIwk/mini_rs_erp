
fn training_process_input_batch(
    batch: &OrderProgressBatch,
    apparatus: &str,
    order_id: &str,
) -> OrderProgressBatch {
    let mut processed = batch.clone();
    processed.wip_status = OrderProgressBatchWipStatus::Processed;
    processed.current_apparatus = apparatus.trim().to_string();
    processed.current_apparatus_key = queue_state::apparatus_search_key(apparatus);
    processed.current_location = format!("{} yakunlandi", apparatus.trim());
    processed.processed_by_session_id = format!(
        "training-input-use:{}:{}:{}",
        apparatus.trim(),
        order_id.trim(),
        processed.batch_id.trim()
    );
    processed.processed_by_apparatus = apparatus.trim().to_string();
    processed.refresh_status_detail();
    processed
}

fn training_has_unprocessed_previous_wips(
    batches: &[OrderProgressBatch],
    order_id: &str,
    previous_stage: &str,
    apparatus: &str,
    ignored_batch_id: &str,
) -> bool {
    batches.iter().any(|batch| {
        training_input_batch_is_available(batch, order_id, previous_stage, apparatus)
            && (ignored_batch_id.trim().is_empty()
                || !batch
                    .batch_id
                    .trim()
                    .eq_ignore_ascii_case(ignored_batch_id.trim()))
    })
}

async fn training_input_progress_batches_for_map(
    store: &PostgresTrainingWorkspaceStore,
    map: &ProductionMapDefinition,
    apparatus: &str,
) -> Result<Vec<OrderProgressBatch>, TrainingWorkspaceError> {
    let Some(previous_stage) = training_input_stage_for_map(map, apparatus) else {
        return Ok(Vec::new());
    };
    let mut batches = store.training_progress_batches_for_order(&map.id).await?;
    let identities = store.training_input_batches(&map.id, apparatus).await?;
    for identity in identities {
        if batches.iter().any(|batch| {
            batch
                .batch_id
                .trim()
                .eq_ignore_ascii_case(identity.batch_id.trim())
        }) {
            continue;
        }
        if let Some(batch) = training_input_progress_batch(map, apparatus, &identity) {
            batches.push(batch);
        }
    }
    Ok(batches
        .into_iter()
        .filter(|batch| training_input_batch_matches(batch, &map.id, &previous_stage, apparatus))
        .collect())
}

async fn training_generated_input_progress_batches_for_map(
    store: &PostgresTrainingWorkspaceStore,
    map: &ProductionMapDefinition,
    apparatus: &str,
) -> Result<Vec<OrderProgressBatch>, TrainingWorkspaceError> {
    let persisted_batches = store.training_progress_batches_for_order(&map.id).await?;
    let identities = store.training_input_batches(&map.id, apparatus).await?;
    Ok(identities
        .into_iter()
        .filter_map(|identity| {
            persisted_batches
                .iter()
                .find(|batch| {
                    batch
                        .batch_id
                        .trim()
                        .eq_ignore_ascii_case(identity.batch_id.trim())
                })
                .cloned()
                .or_else(|| training_input_progress_batch(map, apparatus, &identity))
        })
        .collect())
}

pub(super) async fn training_input_progress_batch_for_principal(
    state: &AppState,
    principal: &Principal,
    order_id: &str,
    previous_apparatus: &str,
    next_apparatus: &str,
) -> Result<Vec<OrderProgressBatch>, TrainingWorkspaceError> {
    let order_id = order_id.trim();
    if !order_id.starts_with("training-") {
        return Ok(Vec::new());
    }
    let requested_next = if next_apparatus.trim().is_empty() {
        String::new()
    } else {
        canonical_training_apparatus(next_apparatus)?
    };
    let saved = if matches!(&principal.role, PrincipalRole::Aparatchi) {
        worker_training_overlay(state, principal)
            .await?
            .maps
            .into_iter()
            .find(|saved| {
                saved.map.id.trim() == order_id
                    && (requested_next.is_empty()
                        || training_map_has_apparatus(saved, &requested_next))
            })
    } else {
        state
            .training_workspace
            .as_ref()
            .ok_or(TrainingWorkspaceError::StoreFailed)?
            .map(order_id)
            .await?
    };
    let Some(saved) = saved else {
        return Ok(Vec::new());
    };
    let target = if requested_next.is_empty() {
        training_input_target_apparatus(&saved.map).unwrap_or_default()
    } else {
        requested_next.clone()
    };
    let Some(_) = training_input_stage_for_map(&saved.map, &target) else {
        return Ok(Vec::new());
    };
    let Some(store) = state.training_workspace.as_ref() else {
        return Ok(Vec::new());
    };
    Ok(
        training_input_progress_batches_for_map(store, &saved.map, &target)
            .await?
            .into_iter()
            .filter(|batch| {
                previous_apparatus.trim().is_empty()
                    || canonical_apparatus_matches(&batch.apparatus, previous_apparatus)
            })
            .collect(),
    )
}

pub(super) async fn training_progress_batch_for_qr(
    state: &AppState,
    principal: &Principal,
    progress_batch_id: &str,
    qr_payload: &str,
) -> Result<Option<OrderProgressBatch>, TrainingWorkspaceError> {
    let qr_payload = qr_payload.trim();
    let Some(store) = state.training_workspace.as_ref() else {
        return Ok(None);
    };
    if let Some(batch) = store
        .training_progress_batch_for_key(progress_batch_id, qr_payload)
        .await?
    {
        let is_visible = if matches!(&principal.role, PrincipalRole::Aparatchi) {
            worker_training_overlay(state, principal)
                .await?
                .maps
                .iter()
                .any(|saved| saved.map.id.trim().eq_ignore_ascii_case(&batch.order_id))
        } else {
            store.map(&batch.order_id).await?.is_some()
        };
        return Ok(is_visible.then_some(batch));
    }
    let identity_for_qr = store.training_input_batch_for_qr(qr_payload).await?;
    let legacy_order_id = training_input_order_id_from_qr(qr_payload);
    let order_id = identity_for_qr
        .as_ref()
        .map(|identity| identity.order_id.clone())
        .or(legacy_order_id.clone());
    let Some(order_id) = order_id else {
        return Ok(None);
    };
    let saved = if matches!(&principal.role, PrincipalRole::Aparatchi) {
        worker_training_overlay(state, principal)
            .await?
            .maps
            .into_iter()
            .find(|saved| saved.map.id.trim().eq_ignore_ascii_case(&order_id))
    } else {
        state
            .training_workspace
            .as_ref()
            .ok_or(TrainingWorkspaceError::StoreFailed)?
            .map(&order_id)
            .await?
    };
    let Some(saved) = saved else {
        return Ok(None);
    };
    let apparatus = identity_for_qr
        .as_ref()
        .map(|identity| identity.apparatus.clone())
        .or_else(|| training_input_target_apparatus(&saved.map));
    let Some(apparatus) = apparatus else {
        return Ok(None);
    };
    let identity = match identity_for_qr {
        Some(identity) => identity,
        None => {
            let Some(previous_stage) = training_input_stage_for_map(&saved.map, &apparatus) else {
                return Ok(None);
            };
            let identities = store.training_input_batches(&order_id, &apparatus).await?;
            if identities.len() != 1 {
                return Ok(None);
            }
            let identity = identities
                .into_iter()
                .next()
                .ok_or(TrainingWorkspaceError::StoreFailed)?;
            if !canonical_apparatus_matches(&identity.apparatus, &apparatus)
                || previous_stage.trim().is_empty()
            {
                return Ok(None);
            }
            identity
        }
    };
    let Some(batch) = training_input_progress_batch(&saved.map, &apparatus, &identity) else {
        return Ok(None);
    };
    if legacy_order_id.is_some() || batch.qr_payload.eq_ignore_ascii_case(qr_payload) {
        Ok(Some(batch))
    } else {
        Ok(None)
    }
}

pub(super) async fn training_progress_batches_for_order(
    state: &AppState,
    order_id: &str,
) -> Result<Vec<OrderProgressBatch>, TrainingWorkspaceError> {
    state
        .training_workspace
        .as_ref()
        .ok_or(TrainingWorkspaceError::StoreFailed)?
        .training_progress_batches_for_order(order_id)
        .await
}

pub(super) async fn worker_training_overlay(
    state: &AppState,
    principal: &Principal,
) -> Result<WorkerTrainingOverlay, TrainingWorkspaceError> {
    if !matches!(&principal.role, PrincipalRole::Aparatchi) {
        return Ok(WorkerTrainingOverlay::default());
    }
    let Some(store) = state.training_workspace.as_ref() else {
        return Ok(WorkerTrainingOverlay::default());
    };
    let assigned_apparatus = state.admin.principal_assigned_apparatus(principal).await;
    let modes = store.apparatus_modes().await?;
    let active_apparatuses = assigned_apparatus
        .into_iter()
        .filter_map(|apparatus| canonical_training_apparatus(&apparatus).ok())
        .filter(|apparatus| {
            modes.iter().any(|(configured, enabled)| {
                *enabled && canonical_apparatus_matches(configured, apparatus)
            })
        })
        .collect::<Vec<_>>();
    if active_apparatuses.is_empty() {
        return Ok(WorkerTrainingOverlay::default());
    }

    let all_maps = store.maps().await?;
    let maps = all_maps
        .into_iter()
        .filter(|saved| {
            active_apparatuses
                .iter()
                .any(|apparatus| training_map_has_apparatus(saved, apparatus))
        })
        .map(|mut saved| {
            saved.map = training_worker_map(saved.map);
            saved
        })
        .collect::<Vec<_>>();
    let stored_states = store.queue_states().await?;
    let mut overlay = WorkerTrainingOverlay {
        active_apparatuses,
        maps,
        ..WorkerTrainingOverlay::default()
    };

    for saved in &overlay.maps {
        let mut batches_by_id = BTreeMap::new();
        for apparatus in &overlay.active_apparatuses {
            if !training_map_has_apparatus(saved, apparatus) {
                continue;
            }
            for batch in
                training_input_progress_batches_for_map(store, &saved.map, apparatus).await?
            {
                batches_by_id.insert(batch.batch_id.trim().to_string(), batch);
            }
        }
        if !batches_by_id.is_empty() {
            overlay.input_progress_batches.insert(
                saved.map.id.trim().to_string(),
                batches_by_id.into_values().collect(),
            );
        }
    }

    for apparatus in &overlay.active_apparatuses {
        let visible_order_ids = overlay
            .maps
            .iter()
            .filter(|saved| training_map_has_apparatus(saved, apparatus))
            .map(|saved| saved.map.id.trim().to_string())
            .filter(|order_id| !order_id.is_empty())
            .collect::<Vec<_>>();
        let sequence = queue_state::effective_apparatus_sequence(&[], &visible_order_ids);
        let visible_set = visible_order_ids.iter().cloned().collect::<BTreeSet<_>>();
        let mut states = BTreeMap::new();
        for (stored_apparatus, stored) in &stored_states {
            if !canonical_apparatus_matches(stored_apparatus, apparatus) {
                continue;
            }
            for (order_id, state) in stored {
                if visible_set.contains(order_id) {
                    states.insert(order_id.clone(), state.clone());
                }
            }
        }
        let controls = training_queue_action_controls(
            apparatus,
            &*state
                .production_maps
                .resolve_canonical_apparatus_text(apparatus)
                .await
                .map_err(|_| TrainingWorkspaceError::StoreFailed)?,
            &sequence,
            &states,
            &overlay.maps,
            &overlay.input_progress_batches,
        );
        let statuses = sequence
            .iter()
            .map(|order_id| {
                let state = controls
                    .get(order_id)
                    .map(|control| control.state)
                    .unwrap_or(queue_state::ApparatusQueueOrderState::Pending);
                (order_id.clone(), training_order_status(state))
            })
            .collect::<BTreeMap<_, _>>();
        overlay
            .sequences
            .insert(apparatus.clone(), sequence.clone());
        overlay
            .visible_order_ids
            .insert(apparatus.clone(), visible_order_ids);
        overlay.queue_states.insert(apparatus.clone(), states);
        overlay
            .queue_action_controls
            .insert(apparatus.clone(), controls);
        let apparatus_id = crate::core::apparatus_standard::ApparatusId::new(apparatus.clone())
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        overlay.queue_policies.push(ApparatusQueuePolicyRecord {
            apparatus_id,
            apparatus: apparatus.clone(),
            policy: ApparatusQueuePolicy::StrictSequence,
            locked: true,
            reason: "training_mode".to_string(),
        });
        overlay.order_statuses.extend(statuses);
    }
    overlay.order_customers = overlay
        .maps
        .iter()
        .filter_map(|saved| {
            let order_id = saved.map.id.trim();
            let customer = saved.map.customer_name.trim();
            (!order_id.is_empty() && !customer.is_empty())
                .then(|| (order_id.to_string(), customer.to_string()))
        })
        .collect();
    Ok(overlay)
}

pub(super) async fn training_map_for_principal(
    state: &AppState,
    principal: &Principal,
    order_id: &str,
    apparatus: &str,
) -> Result<Option<ProductionMapSaved>, TrainingWorkspaceError> {
    let order_id = order_id.trim();
    if !order_id.starts_with("training-") {
        return Ok(None);
    }
    let store = state
        .training_workspace
        .as_ref()
        .ok_or(TrainingWorkspaceError::StoreFailed)?;
    let apparatus = if apparatus.trim().is_empty() {
        None
    } else {
        Some(canonical_training_apparatus(apparatus)?)
    };
    if matches!(&principal.role, PrincipalRole::Aparatchi) {
        let overlay = worker_training_overlay(state, principal).await?;
        let Some(active_apparatus) = overlay.active_apparatuses.iter().find(|candidate| {
            apparatus
                .as_ref()
                .is_some_and(|id| canonical_apparatus_matches(candidate, id))
        }) else {
            return Err(TrainingWorkspaceError::MapNotFound);
        };
        let Some(saved) = overlay.maps.iter().find(|saved| {
            saved.map.id.trim() == order_id && training_map_has_apparatus(saved, active_apparatus)
        }) else {
            return Err(TrainingWorkspaceError::MapNotFound);
        };
        Ok(Some(saved.clone()))
    } else {
        let Some(saved) = store.map(order_id).await? else {
            return Err(TrainingWorkspaceError::MapNotFound);
        };
        if apparatus
            .as_ref()
            .is_some_and(|id| !training_map_has_apparatus(&saved, id))
        {
            return Err(TrainingWorkspaceError::MapNotFound);
        }
        Ok(Some(saved))
    }
}
