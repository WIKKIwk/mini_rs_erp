pub(super) async fn save_order_control_state_tx(
    tx: &mut Transaction<'_, Postgres>,
    record: &OrderControlRecord,
) -> Result<(), ProductionMapError> {
    lock_order_tx(tx, &record.order_id).await?;
    if !record.stage_node_id.is_empty() {
        return save_stage_control_tx(tx, record).await;
    }
    if record.state == OrderControlState::Frozen {
        // Immediate stops and safe-stop finalization share this writer. Safe
        // stops have already frozen their session and queue in this transaction.
        let running = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM mini_order_run_sessions WHERE order_id=$1 AND status='active')
                OR EXISTS(SELECT 1 FROM mini_queue_states WHERE order_id=$1 AND state IN ('in_progress','print_preflight'))
                OR EXISTS(SELECT 1 FROM mini_print_preflight_holds WHERE order_id=$1 AND status IN ('held','running'))",
        ).bind(&record.order_id).fetch_one(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        if running { return Err(ProductionMapError::OrderControlActionNotAllowed); }
    }
    if record.early_close.is_some() {
        let lifecycle = sqlx::query_scalar::<_, String>(
            "SELECT lifecycle_status FROM mini_production_maps WHERE id = $1 FOR UPDATE")
            .bind(record.order_id.trim()).fetch_optional(&mut **tx).await
            .map_err(|_| ProductionMapError::StoreFailed)?
            .ok_or(ProductionMapError::MapNotFound)?;
        if matches!(lifecycle.as_str(), "production_completed" | "closed") {
            return Err(ProductionMapError::OrderAlreadyCompleted);
        }
        if record.freeze_request.is_none()
            && record.early_close.as_ref().is_some_and(|close| close.closed_at_unix.is_some())
        {
            // Recheck under the order lock: another process may have started
            // the previously untouched order after the service read it.
            let running = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (SELECT 1 FROM mini_order_run_sessions WHERE order_id = $1 AND status = 'active')
                     OR EXISTS (SELECT 1 FROM mini_queue_states WHERE order_id = $1 AND state IN ('in_progress', 'print_preflight'))
                     OR EXISTS (SELECT 1 FROM mini_print_preflight_holds WHERE order_id = $1 AND status IN ('held', 'running'))")
                .bind(record.order_id.trim()).fetch_one(&mut **tx).await
                .map_err(|_| ProductionMapError::StoreFailed)?;
            if running {
                return Err(ProductionMapError::OrderControlActionNotAllowed);
            }
        }
    }
    let current = sqlx::query_as::<_, (String, Option<String>)>(
        r#"SELECT state, freeze_request_id
           FROM mini_order_control_states
           WHERE order_id = $1
           FOR UPDATE"#,
    )
    .bind(record.order_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    if let Some(request) = &record.freeze_request {
        validate_freeze_transition(current.as_ref(), record.state, request)?;
        save_freeze_request_tx(tx, record, request).await?;
    } else if record.early_close.is_none() && current
        .as_ref()
        .is_some_and(|(state, _)| state != OrderControlState::Active.as_str())
    {
        return Err(ProductionMapError::OrderControlActionNotAllowed);
    }

    sqlx::query(
        r#"INSERT INTO mini_order_control_states
             (order_id, state, actor_role, actor_ref, actor_display_name,
              requested_at_unix, frozen_at_unix, freeze_request_id, early_close, updated_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, now())
           ON CONFLICT (order_id) DO UPDATE SET
              state = excluded.state,
              actor_role = excluded.actor_role,
              actor_ref = excluded.actor_ref,
              actor_display_name = excluded.actor_display_name,
              requested_at_unix = excluded.requested_at_unix,
              frozen_at_unix = excluded.frozen_at_unix,
              freeze_request_id = excluded.freeze_request_id,
              early_close = excluded.early_close,
              updated_at = excluded.updated_at"#,
    )
    .bind(record.order_id.trim())
    .bind(record.state.as_str())
    .bind(record.actor.role.trim())
    .bind(record.actor.ref_.trim())
    .bind(record.actor.display_name.trim())
    .bind(record.requested_at_unix)
    .bind(record.frozen_at_unix)
    .bind(
        record
            .freeze_request
            .as_ref()
            .map(|request| request.request_id.trim()),
    )
    .bind(record.early_close.as_ref().map(serde_json::to_value).transpose()
        .map_err(|_| ProductionMapError::StoreFailed)?)
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    apply_early_close_tx(tx, record).await?;
    Ok(())
}

async fn apply_early_close_tx(tx: &mut Transaction<'_ , Postgres>, record: &OrderControlRecord) -> Result<(), ProductionMapError> {
    if let Some(close) = &record.early_close {
        if let Some(closed_at) = close.closed_at_unix {
            // Cancel only the remaining work, never manufacture completion/output.
            // Audit + queue removal + lifecycle + worker's final output share this transaction.
            sqlx::query(
                "WITH previous AS (
                    SELECT id, lifecycle_status, lifecycle_version FROM mini_production_maps
                    WHERE id = $1 AND lifecycle_status NOT IN ('cancelled', 'closed', 'production_completed') FOR UPDATE
                 ), changed AS (
                    UPDATE mini_production_maps m SET lifecycle_status = 'cancelled',
                        completion_outcome = 'with_issue', closed_at = to_timestamp($2),
                        lifecycle_changed_at = to_timestamp($2), lifecycle_version = m.lifecycle_version + 1
                    FROM previous p WHERE m.id = p.id RETURNING m.id, m.lifecycle_version, p.lifecycle_status
                 ) INSERT INTO mini_production_order_lifecycle_events
                    (event_id, order_id, from_status, to_status, completion_outcome,
                     actor_role, actor_ref, actor_display_name, source_event_id, reason, lifecycle_version, created_at)
                 SELECT 'early-close:' || id, id, lifecycle_status, 'cancelled', 'with_issue',
                    $3, $4, $5, 'early-close:' || id, $6, lifecycle_version, to_timestamp($2)
                 FROM changed ON CONFLICT (event_id) DO NOTHING"
            )
            .bind(record.order_id.trim()).bind(closed_at as f64)
            .bind(&close.actor.role).bind(&close.actor.ref_).bind(&close.actor.display_name)
            .bind(&close.comment).execute(&mut **tx).await
            .map_err(|_| ProductionMapError::StoreFailed)?;
            sqlx::query("UPDATE mini_queue_sequences SET order_ids = order_ids - $1::text, updated_at = now() WHERE order_ids ? $1::text")
                .bind(record.order_id.trim()).execute(&mut **tx).await
                .map_err(|_| ProductionMapError::StoreFailed)?;
        }
    }
    Ok(())
}

fn validate_freeze_transition(
    current: Option<&(String, Option<String>)>,
    next_state: OrderControlState,
    request: &OrderFreezeRequest,
) -> Result<(), ProductionMapError> {
    let current_state = current.map(|(state, _)| state.as_str()).unwrap_or("active");
    let current_request_id = current
        .and_then(|(_, request_id)| request_id.as_deref())
        .unwrap_or_default();
    let same_request = current_request_id.is_empty() || current_request_id == request.request_id.trim();
    let replaying_same_request = !current_request_id.is_empty()
        && current_request_id == request.request_id.trim();
    let allowed = match request.status {
        OrderFreezeRequestStatus::Pending => {
            (current_state == "active" && next_state == OrderControlState::FreezeRequested)
                || (replaying_same_request
                    && current_state == "freeze_requested"
                    && next_state == OrderControlState::FreezeRequested)
        }
        OrderFreezeRequestStatus::Frozen => {
            next_state == OrderControlState::Frozen
                && (current_state == "active"
                    || (same_request && matches!(current_state, "freeze_requested" | "frozen")))
        }
        OrderFreezeRequestStatus::Cancelled => {
            replaying_same_request
                && next_state == OrderControlState::Active
                && matches!(current_state, "freeze_requested" | "active")
        }
        OrderFreezeRequestStatus::Unfrozen => {
            same_request
                && ((current_state == "frozen" && next_state == OrderControlState::Active)
                    || (replaying_same_request
                        && current_state == "active"
                        && next_state == OrderControlState::Active))
        }
    };
    allowed
        .then_some(())
        .ok_or(ProductionMapError::OrderControlActionNotAllowed)
}

async fn save_freeze_request_tx(
    tx: &mut Transaction<'_, Postgres>,
    control: &OrderControlRecord,
    request: &OrderFreezeRequest,
) -> Result<(), ProductionMapError> {
    let canonical_target_apparatus = if request.target_apparatus.trim().is_empty() {
        None
    } else {
        Some(
            ApparatusId::new(request.target_apparatus.trim().to_string())
                .map_err(|_| ProductionMapError::OrderControlActionNotAllowed)?,
        )
    };
    let previous_status = sqlx::query_scalar::<_, String>(
        "SELECT status FROM mini_order_freeze_requests WHERE request_id = $1 FOR UPDATE",
    )
    .bind(request.request_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    let status_allowed = matches!(
        (previous_status.as_deref(), request.status),
        (
            None,
            OrderFreezeRequestStatus::Pending
                | OrderFreezeRequestStatus::Frozen
                | OrderFreezeRequestStatus::Unfrozen,
        ) | (Some("pending"), OrderFreezeRequestStatus::Pending)
            | (Some("pending"), OrderFreezeRequestStatus::Frozen)
            | (Some("pending"), OrderFreezeRequestStatus::Cancelled)
            | (Some("frozen"), OrderFreezeRequestStatus::Frozen)
            | (Some("frozen"), OrderFreezeRequestStatus::Unfrozen)
            | (Some("cancelled"), OrderFreezeRequestStatus::Cancelled)
            | (Some("unfrozen"), OrderFreezeRequestStatus::Unfrozen)
    );
    if !status_allowed {
        return Err(ProductionMapError::OrderControlActionNotAllowed);
    }

    sqlx::query(
        r#"INSERT INTO mini_order_freeze_requests
             (request_id, order_id, status,
              requester_role, requester_ref, requester_display_name,
              target_session_id, target_apparatus, canonical_target_apparatus_id,
              target_worker_role,
              target_worker_ref, target_worker_display_name,
              requested_at_unix, transitioned_at_unix, stage_node_id, updated_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7,
                   COALESCE((SELECT name FROM mini_apparatus WHERE id = $8), $8),
                   NULLIF($8, ''), $9, $10, $11, $12, $13, $14, now())
           ON CONFLICT (request_id) DO UPDATE SET
              status = excluded.status,
              transitioned_at_unix = excluded.transitioned_at_unix,
              updated_at = excluded.updated_at"#,
    )
    .bind(request.request_id.trim())
    .bind(control.order_id.trim())
    .bind(request.status.as_str())
    .bind(control.actor.role.trim())
    .bind(control.actor.ref_.trim())
    .bind(control.actor.display_name.trim())
    .bind(request.target_session_id.trim())
    .bind(
        canonical_target_apparatus
            .as_ref()
            .map(ApparatusId::as_str)
            .unwrap_or_default(),
    )
    .bind(request.target_worker_role.trim())
    .bind(request.target_worker_ref.trim())
    .bind(request.target_worker_display_name.trim())
    .bind(request.requested_at_unix)
    .bind(request.transitioned_at_unix)
    .bind(&control.stage_node_id)
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    if !request.target_worker_ref.trim().is_empty() {
        let event_id = format!(
            "order-freeze-chat:{}:{}",
            request.request_id.trim(),
            request.status.as_str()
        );
        sqlx::query(
            r#"INSERT INTO mini_order_freeze_chat_outbox
                 (event_id, request_id, status)
               VALUES ($1, $2, $3)
               ON CONFLICT (event_id) DO NOTHING"#,
        )
        .bind(event_id)
        .bind(request.request_id.trim())
        .bind(request.status.as_str())
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    }
    Ok(())
}

fn canonical_freeze_target_apparatus(
    canonical: Option<String>,
    legacy_snapshot: Option<&str>,
) -> Result<String, ProductionMapError> {
    match canonical {
        Some(value) => ApparatusId::new(value.trim().to_string())
            .map(|id| id.to_string())
            .map_err(|_| ProductionMapError::StoreFailed),
        None if legacy_snapshot.unwrap_or_default().trim().is_empty() => Ok(String::new()),
        None => Err(ProductionMapError::StoreFailed),
    }
}
