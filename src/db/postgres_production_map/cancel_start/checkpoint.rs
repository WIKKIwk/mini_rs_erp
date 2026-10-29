use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Kind {
    Progress,
    Opening,
    Intake,
    Stock,
    Schedule,
    Configuration,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Resource {
    pub kind: Kind,
    pub key: String,
    pub before: Value,
    pub after: Value,
    pub last_material_event: Option<i64>,
}

#[derive(Serialize, Deserialize)]
pub(in super::super) struct Checkpoint {
    pub control: Option<domain::CancelStartControl>,
    pub start_event: String,
    pub progress_event: String,
    pub map: Value,
    pub assignments: Vec<Value>,
    pub reports: Vec<Value>,
    pub(super) resources: Vec<Resource>,
}

pub(super) async fn row(
    tx: &mut Transaction<'_, Postgres>,
    kind: &Kind,
    key: &str,
) -> Result<Value, ProductionMapError> {
    // Table/column names come solely from this internal enum.
    let (table, column) = match kind {
        Kind::Progress => ("mini_progress_batches", "batch_id"),
        Kind::Opening => ("mini_opening_wip_batches", "batch_id"),
        Kind::Intake => ("mini_opening_wip_intakes", "intake_id"),
        Kind::Stock => ("mini_raw_material_stock", "lower(barcode)"),
        Kind::Schedule => ("mini_apparatus_schedule_reservations", "reservation_id"),
        Kind::Configuration => ("mini_apparatus_queue_policies", "canonical_apparatus_id"),
    };
    sqlx::query_scalar(&format!(
        "SELECT to_jsonb(r) FROM {table} r WHERE {column}=$1 FOR UPDATE"
    ))
    .bind(key)
    .fetch_optional(&mut **tx)
    .await
    .map_err(failed)?
    .ok_or(ProductionMapError::QueueActionNotAllowed)
}

pub(super) async fn assignments(
    tx: &mut Transaction<'_, Postgres>,
    machine: &str,
    order: &str,
) -> Result<Vec<Value>, ProductionMapError> {
    sqlx::query_scalar(
        "SELECT to_jsonb(a) FROM mini_raw_material_assignments a
        WHERE order_id=$1 AND canonical_apparatus_id=$2 ORDER BY barcode FOR UPDATE",
    )
    .bind(order)
    .bind(machine)
    .fetch_all(&mut **tx)
    .await
    .map_err(failed)
}

pub(super) async fn schedule_ids(
    tx: &mut Transaction<'_, Postgres>,
    machine: &str,
    order: &str,
) -> Result<Vec<String>, ProductionMapError> {
    sqlx::query_scalar(
        "SELECT reservation_id FROM mini_apparatus_schedule_reservations
        WHERE order_id=$1 AND canonical_apparatus_id=$2 ORDER BY reservation_id FOR UPDATE",
    )
    .bind(order)
    .bind(machine)
    .fetch_all(&mut **tx)
    .await
    .map_err(failed)
}

pub(super) async fn reports(
    tx: &mut Transaction<'_, Postgres>,
    machine: &str,
    order: &str,
) -> Result<Vec<Value>, ProductionMapError> {
    let mut reports = Vec::new();
    for table in [
        "mini_laminatsiya_astatka_reports",
        "mini_rezka_astatka_reports",
    ] {
        let rows: Vec<Value> = sqlx::query_scalar(&format!(
            "SELECT jsonb_build_object('kind',$3::text,'report',to_jsonb(r)) FROM {table} r
            WHERE order_id=$1 AND canonical_apparatus_id=$2 ORDER BY report_id FOR UPDATE"
        ))
        .bind(order)
        .bind(machine)
        .bind(table)
        .fetch_all(&mut **tx)
        .await
        .map_err(failed)?;
        reports.extend(rows);
    }
    Ok(reports)
}

pub(super) async fn last_material_event(
    tx: &mut Transaction<'_, Postgres>,
    key: &str,
) -> Result<Option<i64>, ProductionMapError> {
    sqlx::query_scalar("SELECT max(id) FROM mini_raw_material_events WHERE barcode_key=$1")
        .bind(key)
        .fetch_one(&mut **tx)
        .await
        .map_err(failed)
}

pub(in super::super) async fn before_start_tx(
    tx: &mut Transaction<'_, Postgres>,
    write: &QueueActionProgressWrite,
) -> Result<Option<Checkpoint>, ProductionMapError> {
    let Some(session) = &write.session else {
        return Ok(None);
    };
    if write.event.action != Action::Start
        || write.event.payload_json.get("cancel_start_supported") != Some(&Value::Bool(true))
        || !matches!(write.event.from_state, State::Pending | State::Completed)
        || session.worker_role != "aparatchi"
        || session.worker_ref.is_empty()
        || session.worker_ref != write.event.actor.ref_
        || session.status != OrderRunStatus::Active
        || write.progress_batch_updates.len() + write.opening_wip_batch_updates.len() != 1
        || write.progress_batch.is_some()
        || !write.progress_batches.is_empty()
        || !write.qolip_checkouts.is_empty()
        || write.map_update.is_some()
        || write
            .raw_material_stock_transitions
            .iter()
            .any(|t| t.kind != RawMaterialStockTransitionKind::InUse)
    {
        return Ok(None);
    }
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM mini_order_run_sessions WHERE session_id=$1)",
    )
    .bind(&session.session_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(failed)?;
    if exists {
        return Ok(None);
    }
    let Some(progress) = &write.progress_event else {
        return Ok(None);
    };
    let (kind, key) = if let Some(batch) = write.progress_batch_updates.first() {
        (Kind::Progress, batch.batch_id.clone())
    } else {
        (
            Kind::Opening,
            write.opening_wip_batch_updates[0].batch_id.clone(),
        )
    };
    let before = row(tx, &kind, &key).await?;
    if text(&before, "wip_status") != "waiting" {
        return Ok(None);
    }
    let mut resources = vec![Resource {
        kind,
        key,
        before,
        after: Value::Null,
        last_material_event: None,
    }];
    if matches!(resources[0].kind, Kind::Opening) {
        let key = text(&resources[0].before, "intake_id").to_string();
        resources.push(Resource {
            before: row(tx, &Kind::Intake, &key).await?,
            kind: Kind::Intake,
            key,
            after: Value::Null,
            last_material_event: None,
        });
    }
    resources.push(Resource {
        before: row(tx, &Kind::Configuration, &write.apparatus).await?,
        kind: Kind::Configuration,
        key: write.apparatus.clone(),
        after: Value::Null,
        last_material_event: None,
    });
    let barcodes = write
        .raw_material_stock_transitions
        .iter()
        .flat_map(|t| &t.barcodes)
        .map(|key| key.trim().to_lowercase())
        .collect::<std::collections::BTreeSet<_>>();
    for key in barcodes {
        let before = row(tx, &Kind::Stock, &key).await?;
        // Already shared/in-use material has no reversible fresh claim.
        if text(&before, "status") != "available" {
            return Ok(None);
        }
        resources.push(Resource {
            kind: Kind::Stock,
            key,
            before,
            after: Value::Null,
            last_material_event: None,
        });
    }
    for key in schedule_ids(tx, &write.apparatus, &write.event.order_id).await? {
        resources.push(Resource {
            before: row(tx, &Kind::Schedule, &key).await?,
            kind: Kind::Schedule,
            key,
            after: Value::Null,
            last_material_event: None,
        });
    }
    let map = sqlx::query_scalar("SELECT map_json FROM mini_production_maps WHERE id=$1")
        .bind(&write.event.order_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(failed)?;
    Ok(Some(Checkpoint {
        control: None,
        start_event: write.event.event_id.clone(),
        progress_event: progress.event_id.clone(),
        map,
        assignments: assignments(tx, &write.apparatus, &write.event.order_id).await?,
        reports: reports(tx, &write.apparatus, &write.event.order_id).await?,
        resources,
    }))
}

pub(in super::super) async fn after_start_tx(
    tx: &mut Transaction<'_, Postgres>,
    write: &QueueActionProgressWrite,
    checkpoint: Option<Checkpoint>,
) -> Result<(), ProductionMapError> {
    let Some(mut checkpoint) = checkpoint else {
        return Ok(());
    };
    let session_id = &write
        .session
        .as_ref()
        .ok_or(ProductionMapError::StoreFailed)?
        .session_id;
    let session = stage_execution::sessions_tx(tx, &write.event.order_id)
        .await?
        .into_iter()
        .find(|session| &session.session_id == session_id)
        .ok_or(ProductionMapError::StoreFailed)?;
    for resource in &mut checkpoint.resources {
        resource.after = row(tx, &resource.kind, &resource.key).await?;
        if matches!(resource.kind, Kind::Stock) {
            resource.last_material_event = last_material_event(tx, &resource.key).await?;
        }
    }
    checkpoint.control = Some(domain::CancelStartControl {
        session_id: session.session_id.clone(),
        token: domain::fingerprint(&session),
        worker_ref: session.worker_ref.clone(),
        previous_state: write.event.from_state,
    });
    sqlx::query("UPDATE mini_order_run_sessions SET payload_json=jsonb_set(payload_json, '{_cancel_start}', $2) WHERE session_id=$1")
        .bind(session_id).bind(serde_json::to_value(checkpoint).map_err(|_| ProductionMapError::StoreFailed)?)
        .execute(&mut **tx).await.map_err(failed)?;
    Ok(())
}

pub(super) async fn restore(
    tx: &mut Transaction<'_, Postgres>,
    resource: &Resource,
) -> Result<(), ProductionMapError> {
    let sql = match resource.kind {
        Kind::Intake | Kind::Configuration => return Ok(()), // Start never changes these witnesses.
        Kind::Progress => "UPDATE mini_progress_batches SET
            wip_status=$2->>'wip_status',current_apparatus=$2->>'current_apparatus',
            canonical_current_apparatus_id=$2->>'canonical_current_apparatus_id',current_location=$2->>'current_location',
            used_by_session_id=$2->>'used_by_session_id',used_by_apparatus=$2->>'used_by_apparatus',
            canonical_used_by_apparatus_id=$2->>'canonical_used_by_apparatus_id',
            processed_by_session_id=$2->>'processed_by_session_id',processed_by_apparatus=$2->>'processed_by_apparatus',
            canonical_processed_by_apparatus_id=$2->>'canonical_processed_by_apparatus_id',
            payload_json=$2->'payload_json',revision=revision+1,updated_at=now() WHERE batch_id=$1",
        Kind::Opening => "UPDATE mini_opening_wip_batches SET wip_status=$2->>'wip_status',
            used_by_session_id=$2->>'used_by_session_id',used_by_apparatus=$2->>'used_by_apparatus',
            processed_by_session_id=$2->>'processed_by_session_id',processed_by_apparatus=$2->>'processed_by_apparatus',
            updated_at=now() WHERE batch_id=$1",
        Kind::Stock => "UPDATE mini_raw_material_stock SET status=$2->>'status',reserved_order_id=$2->>'reserved_order_id',
            payload_json=$2->'payload_json',updated_at=now() WHERE lower(barcode)=$1",
        Kind::Schedule => "UPDATE mini_apparatus_schedule_reservations SET status=$2->>'status',actor_json=$2->'actor_json' WHERE reservation_id=$1",
    };
    let count = sqlx::query(sql)
        .bind(&resource.key)
        .bind(&resource.before)
        .execute(&mut **tx)
        .await
        .map_err(failed)?
        .rows_affected();
    if count != 1 {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    Ok(())
}
