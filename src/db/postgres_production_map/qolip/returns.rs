use std::collections::{BTreeMap, BTreeSet};

use sqlx::{Postgres, Transaction};

use crate::core::apparatus_standard::RuntimeApparatusConfiguration;
use crate::core::production_map::{
    OrderRunSession, OrderRunStatus, ProductionMapError, QueueActionProgressWrite,
    SequenceMoveState, queue_state::ApparatusQueueAction,
};
use crate::core::qolip::QolipError;

type Candidate = (String, String, String);

/// A Resume can have been prepared before the physical return acquired the
/// order/apparatus locks. Never let that stale command restore tooling ownership.
pub(super) async fn validate_returned_qolip_resume_tx(
    tx: &mut Transaction<'_, Postgres>,
    write: &QueueActionProgressWrite,
) -> Result<(), ProductionMapError> {
    if write.event.action != ApparatusQueueAction::Resume {
        return Ok(());
    }
    let Some(session) = &write.session else {
        return Ok(());
    };
    let released: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM mini_order_run_sessions s
         JOIN mini_apparatus a ON a.id=s.canonical_apparatus_id
         WHERE s.session_id=$1 AND s.order_id=$2 AND s.canonical_apparatus_id=$3
           AND a.execution_profile_json->>'operation'='print'
           AND s.payload_json->>'qolip_released_on_return'='true')",
    )
    .bind(&session.session_id)
    .bind(&write.event.order_id)
    .bind(&write.apparatus)
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if released
        && (write
            .event
            .payload_json
            .get("qolip_reacquired_after_freeze")
            .and_then(serde_json::Value::as_bool)
            != Some(true)
            || session.qolip_reacquisition_required()
            || session
                .payload_json
                .get("qolip_lock_owner")
                .and_then(serde_json::Value::as_bool)
                != Some(true))
    {
        return Err(ProductionMapError::QolipCodeMismatch);
    }
    Ok(())
}

async fn candidates(
    tx: &mut Transaction<'_, Postgres>,
    code: &str,
) -> Result<Vec<Candidate>, QolipError> {
    sqlx::query_as(
        "SELECT session_id, order_id, canonical_apparatus_id FROM mini_order_run_sessions s
         WHERE status IN ('active','paused','frozen','roll_detached')
           AND payload_json->>'qolip_lock_owner' = 'true'
           AND (lower(btrim(payload_json->>'qolip_code')) = lower(btrim($1))
             OR EXISTS (SELECT 1 FROM jsonb_array_elements_text(
                 CASE WHEN jsonb_typeof(payload_json->'qolip_codes') = 'array'
                      THEN payload_json->'qolip_codes' ELSE '[]'::jsonb END) c(code)
                 WHERE lower(btrim(c.code)) = lower(btrim($1))))
         ORDER BY order_id, canonical_apparatus_id, session_id",
    )
    .bind(code)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| QolipError::StoreFailed)
}

/// Lock in the same order as Start/Resume and sequence moves: orders, apparatus,
/// then physical molds. None asks the caller to retry with newly discovered
/// orders; taking their order locks after a mold lock would invert that order.
pub(crate) async fn prepare_qolip_return_tx(
    tx: &mut Transaction<'_, Postgres>,
    code: &str,
) -> Result<Option<Vec<OrderRunSession>>, QolipError> {
    let initial = candidates(tx, code).await?;
    let apparatuses = initial
        .iter()
        .map(|(_, _, apparatus)| apparatus.clone())
        .collect::<BTreeSet<_>>();
    let mut locked_orders = initial
        .iter()
        .map(|(_, order, _)| order.clone())
        .collect::<BTreeSet<_>>();
    for apparatus in &apparatuses {
        locked_orders.extend(queue_order_scope(tx, apparatus).await?);
    }
    let orders = locked_orders.iter().map(String::as_str).collect::<Vec<_>>();
    let apparatus_refs = apparatuses.iter().map(String::as_str).collect::<Vec<_>>();
    super::transaction_locks::lock_orders_and_apparatuses_tx(tx, &orders, &apparatus_refs)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("qolip:{}", code.trim().to_ascii_lowercase()))
        .execute(&mut **tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
    let current = candidates(tx, code).await?;
    if current.iter().any(|(_, order, apparatus)| {
        !locked_orders.contains(order) || !apparatuses.contains(apparatus)
    }) {
        return Ok(None);
    }
    for apparatus in &apparatuses {
        if queue_order_scope(tx, apparatus)
            .await?
            .iter()
            .any(|id| !locked_orders.contains(id))
        {
            return Ok(None);
        }
    }
    let ids = current
        .iter()
        .map(|(id, _, _)| id.clone())
        .collect::<Vec<_>>();
    sqlx::query("SELECT session_id FROM mini_order_run_sessions WHERE session_id = ANY($1) ORDER BY session_id FOR UPDATE")
        .bind(&ids).fetch_all(&mut **tx).await.map_err(|_| QolipError::StoreFailed)?;
    let mut sessions = Vec::new();
    for order in current
        .iter()
        .map(|(_, order, _)| order)
        .collect::<BTreeSet<_>>()
    {
        sessions.extend(
            super::stage_execution::sessions_tx(tx, order)
                .await
                .map_err(|_| QolipError::StoreFailed)?
                .into_iter()
                .filter(|s| ids.contains(&s.session_id)),
        );
    }
    for session in &sessions {
        if blocks_return(tx, session).await? {
            return Err(QolipError::QolipInUse);
        }
    }
    Ok(Some(sessions))
}

async fn blocks_return(
    tx: &mut Transaction<'_, Postgres>,
    session: &OrderRunSession,
) -> Result<bool, QolipError> {
    if matches!(
        session.status,
        OrderRunStatus::Frozen | OrderRunStatus::Completed
    ) {
        return Ok(false);
    }
    let (printing, stopped, queue): (bool, bool, Option<String>) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM mini_apparatus WHERE id=$2 AND execution_profile_json->>'operation'='print'),
            (EXISTS(SELECT 1 FROM mini_production_maps WHERE id=$1 AND lifecycle_status IN ('production_completed','closed','cancelled'))
             OR EXISTS(SELECT 1 FROM mini_order_control_states WHERE order_id=$1 AND state='frozen')
             OR EXISTS(SELECT 1 FROM mini_stage_order_controls WHERE order_id=$1 AND canonical_apparatus_id=$2
                       AND stage_node_id=$3 AND record_json->>'state'='frozen')),
            (SELECT state FROM mini_queue_states WHERE order_id=$1 AND canonical_apparatus_id=$2)",
    ).bind(&session.order_id).bind(&session.apparatus).bind(&session.stage_node_id)
        .fetch_one(&mut **tx).await.map_err(|_| QolipError::StoreFailed)?;
    if !printing || stopped || matches!(queue.as_deref(), Some("frozen" | "completed" | "pending"))
    {
        return Ok(false);
    }
    if matches!(queue.as_deref(), Some("in_progress" | "print_preflight"))
        || (queue.is_none() && session.status == OrderRunStatus::Active)
    {
        return Ok(true);
    }
    // DetachRoll is represented by `paused` in the apparatus queue as well.
    Ok(queue_head(tx, &session.apparatus).await?.as_deref() == Some(session.order_id.as_str()))
}

async fn configuration_tx(
    tx: &mut Transaction<'_, Postgres>,
    apparatus: &str,
) -> Result<Option<RuntimeApparatusConfiguration>, QolipError> {
    let configuration: Option<(
        serde_json::Value,
        serde_json::Value,
        serde_json::Value,
        serde_json::Value,
    )> = sqlx::query_as(
        "SELECT r.payload_json,q.payload_json,m.payload_json,c.payload_json FROM mini_apparatus r
         JOIN mini_apparatus_queue_policies q ON q.canonical_apparatus_id=r.id
           AND (q.source_revision,q.source_aasx_sha256)=(r.source_revision,r.source_aasx_sha256)
         JOIN mini_apparatus_material_rules m ON m.canonical_apparatus_id=r.id
           AND (m.source_revision,m.source_aasx_sha256)=(r.source_revision,r.source_aasx_sha256)
         JOIN mini_apparatus_capacity_profiles c ON c.canonical_apparatus_id=r.id
           AND (c.source_revision,c.source_aasx_sha256)=(r.source_revision,r.source_aasx_sha256)
         WHERE r.id=$1 AND r.source_revision IS NOT NULL",
    )
    .bind(apparatus)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
    let Some((runtime, queue, material, capacity)) = configuration else {
        return Ok(None);
    };
    Ok(Some(RuntimeApparatusConfiguration {
        runtime: serde_json::from_value(runtime).map_err(|_| QolipError::StoreFailed)?,
        queue: serde_json::from_value(queue).map_err(|_| QolipError::StoreFailed)?,
        material: serde_json::from_value(material).map_err(|_| QolipError::StoreFailed)?,
        capacity: serde_json::from_value(capacity).map_err(|_| QolipError::StoreFailed)?,
    }))
}

async fn queue_order_scope(
    tx: &mut Transaction<'_, Postgres>,
    apparatus: &str,
) -> Result<Vec<String>, QolipError> {
    let Some(configuration) = configuration_tx(tx, apparatus).await? else {
        // A historical stopped session can outlive its runtime projection.
        // It still needs its own order/apparatus locks, but has no live queue.
        return Ok(Vec::new());
    };
    let maps = super::sequence_move::load_maps_tx(tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
    Ok(SequenceMoveState::from_data(
        &configuration,
        &maps,
        &[],
        &BTreeMap::new(),
        &BTreeSet::new(),
        &BTreeSet::new(),
    )
    .order_ids)
}

async fn queue_head(
    tx: &mut Transaction<'_, Postgres>,
    apparatus: &str,
) -> Result<Option<String>, QolipError> {
    let configuration = configuration_tx(tx, apparatus)
        .await?
        .ok_or(QolipError::StoreFailed)?;
    let mut maps = super::sequence_move::load_maps_tx(tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
    let terminal: Vec<String> = sqlx::query_scalar("SELECT id FROM mini_production_maps WHERE lifecycle_status IN ('production_completed','closed','cancelled')")
        .fetch_all(&mut **tx).await.map_err(|_| QolipError::StoreFailed)?;
    maps.retain(|map| !terminal.contains(&map.id));
    let stored: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT order_ids FROM mini_queue_sequences WHERE canonical_apparatus_id=$1",
    )
    .bind(apparatus)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
    let stored: Vec<String> =
        serde_json::from_value(stored.unwrap_or_else(|| serde_json::json!([])))
            .map_err(|_| QolipError::StoreFailed)?;
    let states: Vec<(String, String)> = sqlx::query_as(
        "SELECT order_id,state FROM mini_queue_states WHERE canonical_apparatus_id=$1",
    )
    .bind(apparatus)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
    let states = states.into_iter().collect::<BTreeMap<_, _>>();
    let frozen: Vec<String> =
        sqlx::query_scalar("SELECT order_id FROM mini_order_control_states WHERE state='frozen'")
            .fetch_all(&mut **tx)
            .await
            .map_err(|_| QolipError::StoreFailed)?;
    let sequence = SequenceMoveState::from_data(
        &configuration,
        &maps,
        &stored,
        &states,
        &frozen.into_iter().collect(),
        &BTreeSet::new(),
    );
    Ok(sequence.order_ids.into_iter().find(|id| {
        !matches!(
            states.get(id).map(String::as_str),
            Some("completed" | "frozen")
        )
    }))
}

pub(crate) async fn release_returned_qolips_tx(
    tx: &mut Transaction<'_, Postgres>,
    sessions: Vec<OrderRunSession>,
    checkout_id: &str,
) -> Result<(), QolipError> {
    for mut session in sessions {
        session.release_returned_qolips(
            checkout_id,
            time::OffsetDateTime::now_utc().unix_timestamp(),
        );
        sqlx::query("UPDATE mini_order_run_sessions SET payload_json=$2,updated_at=now() WHERE session_id=$1")
            .bind(&session.session_id).bind(&session.payload_json).execute(&mut **tx)
            .await.map_err(|_| QolipError::StoreFailed)?;
    }
    sqlx::query("SELECT pg_notify('mini_production_live','state')")
        .execute(&mut **tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
    Ok(())
}
