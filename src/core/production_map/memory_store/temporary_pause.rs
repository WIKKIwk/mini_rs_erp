use super::*;

pub(super) fn matching_receipt(
    event: &ApparatusQueueActionEvent,
    apparatus: &str,
    order_id: &str,
    session_id: &str,
    actor: &QueueActionActor,
) -> Result<(), ProductionMapError> {
    if event.apparatus != apparatus
        || event.order_id != order_id
        || event.actor.role != actor.role
        || event.actor.ref_ != actor.ref_
        || event
            .payload_json
            .get("session_id")
            .and_then(serde_json::Value::as_str)
            != Some(session_id)
    {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    Ok(())
}

pub(super) async fn validate_write(
    store: &MemoryProductionMapStore,
    write: &QueueActionProgressWrite,
) -> Result<bool, ProductionMapError> {
    let event = &write.event;
    if event
        .payload_json
        .get("event")
        .and_then(serde_json::Value::as_str)
        == Some("temporary_pause")
    {
        if let Some(receipt) = store
            .queue_events
            .read()
            .await
            .iter()
            .find(|e| e.event_id == event.event_id)
        {
            matching_receipt(
                receipt,
                &write.apparatus,
                &event.order_id,
                event
                    .payload_json
                    .get("session_id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default(),
                &event.actor,
            )?;
            return Ok(true);
        }
    }
    if let Some(expected) = event.payload_json.get("queue_expected_sequence") {
        let sequences = store.apparatus_sequences().await?;
        let states = store.apparatus_queue_states().await?;
        if *expected
            != serde_json::json!(sequences.get(&write.apparatus).cloned().unwrap_or_default())
            || event.payload_json.get("queue_expected_states")
                != Some(&serde_json::json!(
                    states.get(&write.apparatus).cloned().unwrap_or_default()
                ))
        {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        if let Some(anchors) = event
            .payload_json
            .get("queue_expected_anchor_sessions")
            .and_then(serde_json::Value::as_array)
        {
            for expected in anchors {
                let session: OrderRunSession = serde_json::from_value(expected.clone())
                    .map_err(|_| ProductionMapError::QueueActionNotAllowed)?;
                if store.order_run_session(&session.session_id).await?.as_ref() != Some(&session) {
                    return Err(ProductionMapError::QueueActionNotAllowed);
                }
            }
        }
    }
    if let Some(expected) = event
        .payload_json
        .get(super::super::temporary_pause::EXPECTED_SESSION)
    {
        let current = store
            .active_order_run_session(&write.apparatus, &event.order_id)
            .await?
            .ok_or(ProductionMapError::QueueActionNotAllowed)?;
        if serde_json::to_value(&current).map_err(|_| ProductionMapError::StoreFailed)? != *expected
            || store
                .queue_states
                .read()
                .await
                .get(&write.apparatus)
                .and_then(|s| s.get(&event.order_id))
                .map(String::as_str)
                != Some(event.from_state.as_str())
        {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        if store
            .order_controls
            .read()
            .await
            .get(&event.order_id)
            .is_some_and(|c| c.state != OrderControlState::Active || c.early_close.is_some())
            || store.stage_order_controls.read().await.values().any(|c| {
                c.order_id == event.order_id
                    && c.stage_node_id == current.stage_node_id
                    && c.state != OrderControlState::Active
            })
        {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        if event.action == queue_state::ApparatusQueueAction::Resume {
            let map = store
                .map_by_id(&event.order_id)
                .await?
                .ok_or(ProductionMapError::MapNotFound)?;
            if event
                .payload_json
                .get("temporary_pause_map_fingerprint")
                .and_then(serde_json::Value::as_str)
                != Some(super::super::wip_route::production_map_fingerprint(&map).as_str())
            {
                return Err(ProductionMapError::WipRouteChanged);
            }
            if let Some(inputs) = event
                .payload_json
                .get("temporary_pause_held_inputs")
                .and_then(serde_json::Value::as_array)
            {
                for input in inputs {
                    let id = input
                        .get("batch_id")
                        .and_then(serde_json::Value::as_str)
                        .ok_or(ProductionMapError::ProgressBatchNotResumable)?;
                    let valid = if input.get("source_kind").and_then(serde_json::Value::as_str)
                        == Some("opening_wip")
                    {
                        store.opening_wip_batch(id, "").await?.is_some_and(|r| {
                            r.intake.status == OpeningWipIntakeStatus::Confirmed
                                && r.batch.wip_status == OpeningWipBatchStatus::InUse
                                && r.batch.used_by_session_id == current.session_id
                                && r.batch.used_by_apparatus == current.apparatus
                        })
                    } else {
                        store.progress_batch(id).await?.is_some_and(|b| {
                            b.wip_status == OrderProgressBatchWipStatus::InUse
                                && b.used_by_session_id == current.session_id
                                && b.used_by_apparatus == current.apparatus
                        })
                    };
                    if !valid {
                        return Err(ProductionMapError::ProgressBatchNotResumable);
                    }
                }
            } else {
                return Err(ProductionMapError::ProgressBatchNotResumable);
            }
        }
    }
    Ok(false)
}
