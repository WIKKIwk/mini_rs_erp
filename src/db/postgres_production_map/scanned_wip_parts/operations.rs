pub(super) async fn load(
    store: &PostgresProductionMapStore,
    apparatus: &str,
    order: &str,
    stage: &str,
    actor: &QueueActionActor,
) -> Result<Vec<ScannedWipInput>, ProductionMapError> {
    let mut tx = store.pool.begin().await.map_err(failed)?;
    transaction_locks::lock_order_and_apparatuses_tx(&mut tx, order, &[apparatus]).await?;
    let h = history_tx(&mut tx, apparatus, order, stage).await?;
    let mut items = Vec::with_capacity(h.claims.len());
    for claim in &h.claims {
        items.push(project(&mut tx, claim, &h, actor).await?);
    }
    items.sort_by_key(|i| std::cmp::Reverse(i.linked_at_unix));
    tx.commit().await.map_err(failed)?;
    Ok(items)
}

pub(super) async fn execute(
    store: &PostgresProductionMapStore,
    target: &ScannedWipReturn,
    actor: &QueueActionActor,
    assigned: &[String],
) -> Result<(), ProductionMapError> {
    let mut tx = store.pool.begin().await.map_err(failed)?;
    transaction_locks::lock_order_and_apparatuses_tx(
        &mut tx,
        &target.order_id,
        &[&target.apparatus],
    )
    .await?;
    let receipt: Option<(String,String,String,String,String)>=sqlx::query_as(
        "SELECT order_id,canonical_apparatus_id,stage_node_id,session_id,worker_ref FROM mini_scanned_wip_inputs
         WHERE return_token=$1 AND batch_id=$2")
        .bind(&target.token).bind(&target.batch_id).fetch_optional(&mut *tx).await.map_err(failed)?;
    if let Some(receipt) = receipt {
        return if receipt
            == (
                target.order_id.clone(),
                target.apparatus.clone(),
                target.stage_node_id.clone(),
                target.session_id.clone(),
                actor.ref_.clone(),
            ) {
            Ok(())
        } else {
            Err(ProductionMapError::QueueActionNotAllowed)
        };
    }
    let h = history_tx(
        &mut tx,
        &target.apparatus,
        &target.order_id,
        &target.stage_node_id,
    )
    .await?;
    let claim = h
        .claims
        .iter()
        .find(|c| {
            c.session_id == target.session_id
                && c.batch_id == target.batch_id
                && c.returned_at_unix.is_none()
        })
        .ok_or(ProductionMapError::QueueActionNotAllowed)?;
    let item = project(&mut tx, claim, &h, actor).await?;
    if item.return_token.as_deref() != Some(target.token.as_str()) {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    let session = h
        .sessions
        .iter()
        .find(|s| s.session_id == target.session_id)
        .ok_or(ProductionMapError::QueueActionNotAllowed)?;
    let now = crate::core::production_map::stage_execution::report_now();
    let (mut next, links) = domain::detach_scanned_input(session, &target.batch_id, now)?;
    cancel_start::restore_scanned_input_tx(
        &mut tx,
        &claim.source_kind,
        &claim.batch_id,
        &claim.before_json,
    )
    .await?;
    let queue_state: String = sqlx::query_scalar(
        "SELECT state FROM mini_queue_states
        WHERE order_id=$1 AND canonical_apparatus_id=$2 FOR UPDATE",
    )
    .bind(&target.order_id)
    .bind(&target.apparatus)
    .fetch_one(&mut *tx)
    .await
    .map_err(failed)?;
    if queue_state != "in_progress" {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    let policy: String = sqlx::query_scalar(
        "SELECT policy FROM mini_queue_action_events
        WHERE order_id=$1 AND canonical_apparatus_id=$2 ORDER BY id DESC LIMIT 1",
    )
    .bind(&target.order_id)
    .bind(&target.apparatus)
    .fetch_one(&mut *tx)
    .await
    .map_err(failed)?;
    let policy =
        ApparatusQueuePolicy::parse(&policy).ok_or(ProductionMapError::QueueActionNotAllowed)?;
    let state = if links.is_empty() {
        cancel_start::finish_scanned_session_tx(&mut tx, session, actor, &target.token).await?
    } else {
        let active = links
            .iter()
            .find(|l| l.status == OrderRunInputStatus::InUse)
            .ok_or(ProductionMapError::QueueActionNotAllowed)?;
        let row = input_row(&mut tx, active.source_kind.as_str(), &active.input_batch_id).await?;
        if !ownership(&row, session) {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        if text(&row, "wip_status") == "processed" {
            promote_input_tx(&mut tx, active, session, now).await?;
        }
        next.payload_json["merge_count"] = json!(links.len().saturating_sub(1));
        put_order_run_session_tx(&mut tx, &next).await?;
        queue_state::ApparatusQueueOrderState::InProgress
    };
    let event_id = format!("return-wip:{}", target.token);
    let payload = json!({"event":"wip_input_returned","session_id":target.session_id,
        "input_batch_id":target.batch_id,"qr_payload":claim.qr_payload,"claim_id":claim.claim_id});
    let event = ApparatusQueueActionEvent {
        event_id: event_id.clone(),
        apparatus: target.apparatus.clone(),
        order_id: target.order_id.clone(),
        stage_node_id: target.stage_node_id.clone(),
        action: queue_state::ApparatusQueueAction::Pause,
        from_state: queue_state::ApparatusQueueOrderState::InProgress,
        to_state: state,
        policy,
        actor: actor.clone(),
        assigned_apparatus: assigned.to_vec(),
        payload_json: payload.clone(),
    };
    put_queue_action_state_tx(&mut tx, &event).await?;
    insert_queue_action_event_tx(&mut tx, &event).await?;
    let mut progress =
        crate::core::production_map::cancel_start::audit_event(session, actor, payload);
    progress.event_id = format!("progress:{event_id}");
    put_order_progress_event_tx(&mut tx, &progress).await?;
    // Persist a legacy receipt only as part of its successful authorized return.
    sqlx::query("INSERT INTO mini_scanned_wip_inputs
        (claim_id,session_id,order_id,canonical_apparatus_id,stage_node_id,batch_id,source_kind,
        qr_payload,source_apparatus,worker_ref,worker_display_name,linked_at_unix,before_json,claimed_json,map_json,
        returned_at_unix,returned_by,return_token) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$10,$17)
        ON CONFLICT(claim_id) DO UPDATE SET returned_at_unix=excluded.returned_at_unix,
        returned_by=excluded.returned_by,return_token=excluded.return_token")
        .bind(&claim.claim_id).bind(&claim.session_id).bind(&target.order_id).bind(&target.apparatus)
        .bind(&target.stage_node_id).bind(&claim.batch_id).bind(&claim.source_kind).bind(&claim.qr_payload)
        .bind(&claim.source_apparatus).bind(&actor.ref_).bind(&claim.worker_display_name).bind(claim.linked_at_unix)
        .bind(&claim.before_json).bind(&claim.claimed_json).bind(&claim.map_json).bind(now).bind(&target.token)
        .execute(&mut *tx).await.map_err(failed)?;
    refresh_production_order_lifecycle_tx(
        &mut tx,
        &target.order_id,
        actor,
        &event_id,
        "wip_input_returned",
    )
    .await?;
    tx.commit().await.map_err(failed)
}

async fn promote_input_tx(
    tx: &mut Transaction<'_, Postgres>,
    link: &OrderRunInputLink,
    session: &OrderRunSession,
    now: i64,
) -> Result<(), ProductionMapError> {
    // Only the immediately preceding source already owned by this session is
    // mounted again. Its immutable output/quantity/QR history is not rewritten.
    if link.source_kind == OrderRunInputSourceKind::ProgressBatch {
        let batch = load_progress_batch(&mut **tx, &link.input_batch_id)
            .await?
            .ok_or(ProductionMapError::ProgressBatchNotFound)?;
        let batch =
            domain::remount_progress_input(batch, &session.apparatus, &session.session_id, now);
        put_order_progress_batch_tx(tx, &batch).await
    } else {
        sqlx::query("UPDATE mini_opening_wip_batches SET wip_status='in_use',
            processed_by_session_id='',processed_by_apparatus='',updated_at=now() WHERE batch_id=$1")
            .bind(&link.input_batch_id).execute(&mut **tx).await.map_err(failed)?;
        Ok(())
    }
}
