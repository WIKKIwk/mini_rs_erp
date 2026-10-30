//! Input receipts and on-demand history. No history read is part of a queue snapshot.
use super::*;
use super::{cancel_start, stage_execution};
use crate::core::production_map::{scanned_wip as domain, *};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};

fn failed(error: sqlx::Error) -> ProductionMapError {
    tracing::error!(%error, "scanned WIP operation failed");
    ProductionMapError::StoreFailed
}
fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or_default()
}

#[derive(Clone, sqlx::FromRow)]
struct Claim {
    claim_id: String,
    session_id: String,
    batch_id: String,
    source_kind: String,
    qr_payload: String,
    source_apparatus: String,
    worker_ref: String,
    worker_display_name: String,
    linked_at_unix: i64,
    work_cursor: i64,
    material_cursor: i64,
    before_json: Value,
    claimed_json: Value,
    map_json: Value,
    returned_at_unix: Option<i64>,
}

pub(super) struct PendingClaim {
    source_kind: &'static str,
    batch_id: String,
    before: Value,
}

async fn input_row(
    tx: &mut Transaction<'_, Postgres>,
    kind: &str,
    batch: &str,
) -> Result<Value, ProductionMapError> {
    let table = match kind {
        "progress_batch" => "mini_progress_batches",
        "opening_wip" => "mini_opening_wip_batches",
        _ => return Err(ProductionMapError::QueueActionNotAllowed),
    };
    sqlx::query_scalar(&format!(
        "SELECT to_jsonb(b) FROM {table} b WHERE batch_id=$1 FOR UPDATE"
    ))
    .bind(batch)
    .fetch_optional(&mut **tx)
    .await
    .map_err(failed)?
    .ok_or(ProductionMapError::ProgressBatchNotFound)
}

pub(super) async fn before_claim_tx(
    tx: &mut Transaction<'_, Postgres>,
    write: &QueueActionProgressWrite,
) -> Result<Option<PendingClaim>, ProductionMapError> {
    if write.event.actor.role != "aparatchi"
        || write.session.is_none()
        || !matches!(
            write.event.action,
            queue_state::ApparatusQueueAction::Start
                | queue_state::ApparatusQueueAction::Resume
                | queue_state::ApparatusQueueAction::Merge
        )
    {
        return Ok(None);
    }
    let candidates = write
        .progress_batch_updates
        .iter()
        .filter(|b| b.wip_status == OrderProgressBatchWipStatus::InUse)
        .map(|b| ("progress_batch", b.batch_id.as_str()))
        .chain(
            write
                .opening_wip_batch_updates
                .iter()
                .filter(|b| b.wip_status == OpeningWipBatchStatus::InUse)
                .map(|b| ("opening_wip", b.batch_id.as_str())),
        );
    let mut claim = None;
    for (kind, id) in candidates {
        let before = input_row(tx, kind, id).await?;
        if text(&before, "wip_status") != "waiting" {
            continue;
        }
        if claim.is_some() {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        claim = Some(PendingClaim {
            source_kind: kind,
            batch_id: id.into(),
            before,
        });
    }
    Ok(claim)
}

pub(super) async fn after_claim_tx(
    tx: &mut Transaction<'_, Postgres>,
    write: &QueueActionProgressWrite,
    claim: Option<PendingClaim>,
) -> Result<(), ProductionMapError> {
    let Some(claim) = claim else {
        return Ok(());
    };
    let session = write
        .session
        .as_ref()
        .ok_or(ProductionMapError::StoreFailed)?;
    let after = input_row(tx, claim.source_kind, &claim.batch_id).await?;
    let map: Value = sqlx::query_scalar("SELECT map_json FROM mini_production_maps WHERE id=$1")
        .bind(&session.order_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(failed)?;
    let source = text(&claim.before, "canonical_apparatus_id");
    let source = if source.is_empty() {
        text(&claim.before, "apparatus")
    } else {
        source
    };
    sqlx::query("INSERT INTO mini_scanned_wip_inputs
        (claim_id,session_id,order_id,canonical_apparatus_id,stage_node_id,batch_id,source_kind,
         qr_payload,source_apparatus,worker_ref,worker_display_name,linked_at_unix,before_json,claimed_json,map_json,
         work_cursor,material_cursor)
        VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,
         (SELECT COALESCE(max(id),0) FROM mini_order_progress_events WHERE session_id=$2),
         (SELECT COALESCE(max(id),0) FROM mini_raw_material_events WHERE order_id=$3 AND canonical_apparatus_id=$4))
        ON CONFLICT(claim_id) DO NOTHING")
        .bind(&write.event.event_id).bind(&session.session_id).bind(&session.order_id)
        .bind(&session.apparatus).bind(&session.stage_node_id).bind(&claim.batch_id).bind(claim.source_kind)
        .bind(text(&claim.before,"qr_payload")).bind(source).bind(&write.event.actor.ref_)
        .bind(&write.event.actor.display_name).bind(session.updated_at_unix)
        .bind(&claim.before).bind(after).bind(map).execute(&mut **tx).await.map_err(failed)?;
    Ok(())
}

struct History {
    claims: Vec<Claim>,
    sessions: Vec<OrderRunSession>,
    outputs: Vec<Value>,
    work: Vec<Value>,
    map: Value,
    blocked: bool,
}

async fn history_tx(
    tx: &mut Transaction<'_, Postgres>,
    apparatus: &str,
    order: &str,
    stage: &str,
) -> Result<History, ProductionMapError> {
    let mut claims = sqlx::query_as::<_, Claim>("SELECT claim_id,session_id,batch_id,source_kind,qr_payload,
        source_apparatus,worker_ref,worker_display_name,linked_at_unix,work_cursor,material_cursor,
        before_json,claimed_json,map_json,returned_at_unix
        FROM mini_scanned_wip_inputs WHERE order_id=$1 AND canonical_apparatus_id=$2 AND stage_node_id=$3
        ORDER BY linked_at_unix DESC,claim_id DESC")
        .bind(order).bind(apparatus).bind(stage).fetch_all(&mut **tx).await.map_err(failed)?;
    let sessions = stage_execution::sessions_tx(tx, order)
        .await?
        .into_iter()
        .filter(|s| s.apparatus == apparatus && s.stage_node_id == stage)
        .collect::<Vec<_>>();
    // Legacy sessions remain visible; only their durable Start snapshot can
    // authorize a reversal. Never invent an old location or material state.
    for session in &sessions {
        let links = domain::scanned_input_links(session)?;
        for link in links {
            if claims.iter().any(|c| {
                c.session_id == session.session_id
                    && c.batch_id == link.input_batch_id
                    && c.returned_at_unix.is_none()
            }) {
                continue;
            }
            let resource = session.payload_json["_cancel_start"]["resources"]
                .as_array()
                .and_then(|resources| {
                    resources
                        .iter()
                        .find(|r| text(r, "key") == link.input_batch_id)
                });
            claims.push(Claim {
                claim_id: format!("legacy:{}:{}", session.session_id, link.input_batch_id),
                session_id: session.session_id.clone(),
                batch_id: link.input_batch_id,
                source_kind: link.source_kind.as_str().into(),
                qr_payload: link.input_qr_payload,
                source_apparatus: link.source_apparatus,
                worker_ref: session.worker_ref.clone(),
                worker_display_name: session.worker_display_name.clone(),
                linked_at_unix: link.linked_at_unix,
                work_cursor: 0,
                material_cursor: 0,
                before_json: resource.map(|r| r["before"].clone()).unwrap_or(Value::Null),
                claimed_json: resource.map(|r| r["after"].clone()).unwrap_or(Value::Null),
                map_json: session.payload_json["_cancel_start"]["map"].clone(),
                returned_at_unix: None,
            });
        }
    }
    let outputs = sqlx::query_scalar(
        "SELECT to_jsonb(b) FROM mini_progress_batches b
        WHERE order_id=$1 AND canonical_apparatus_id=$2",
    )
    .bind(order)
    .bind(apparatus)
    .fetch_all(&mut **tx)
    .await
    .map_err(failed)?;
    let work = sqlx::query_scalar(
        "SELECT to_jsonb(e) FROM mini_order_progress_events e
        WHERE order_id=$1 AND canonical_apparatus_id=$2",
    )
    .bind(order)
    .bind(apparatus)
    .fetch_all(&mut **tx)
    .await
    .map_err(failed)?;
    let map = sqlx::query_scalar("SELECT map_json FROM mini_production_maps WHERE id=$1 FOR SHARE")
        .bind(order)
        .fetch_optional(&mut **tx)
        .await
        .map_err(failed)?
        .ok_or(ProductionMapError::MissingId)?;
    let blocked: bool = sqlx::query_scalar("SELECT
        EXISTS(SELECT 1 FROM mini_order_control_states WHERE order_id=$1 AND (state<>'active' OR early_close IS NOT NULL)) OR
        EXISTS(SELECT 1 FROM mini_stage_order_controls WHERE order_id=$1 AND stage_node_id=$2 AND
          (record_json->>'state'<>'active' OR record_json->'early_close' IS NOT NULL AND record_json->'early_close'<>'null'::jsonb))")
        .bind(order).bind(stage).fetch_one(&mut **tx).await.map_err(failed)?;
    Ok(History {
        claims,
        sessions,
        outputs,
        work,
        map,
        blocked,
    })
}

fn source_evidence(output: &Value, batch: &str) -> Option<bool> {
    let payload = &output["payload_json"];
    if payload.get("source_input_links").is_some() {
        let links = progress_batch_input_links_from_payload(payload).ok()?;
        return (!links.is_empty()).then(|| links.iter().any(|l| l.input_batch_id == batch));
    }
    if payload.get("source_input_batch_ids").is_some() {
        let ids = payload["source_input_batch_ids"].as_array()?;
        if ids.is_empty()
            || ids
                .iter()
                .any(|id| id.as_str().is_none_or(|s| s.trim().is_empty()))
        {
            return None;
        }
        return Some(ids.iter().any(|id| id.as_str() == Some(batch)));
    }
    for key in [
        "parent_batch_id",
        "input_progress_batch_id",
        "input_batch_id",
    ] {
        let id = if key == "parent_batch_id" {
            text(output, key)
        } else {
            text(payload, key)
        };
        if !id.is_empty() {
            // A nominal parent does not exclude other legacy splice inputs.
            return (id == batch).then_some(true);
        }
    }
    None
}

fn consumption(claim: &Claim, history: &History) -> &'static str {
    let mut unknown = false;
    for output in history
        .outputs
        .iter()
        .filter(|b| text(b, "session_id") == claim.session_id)
    {
        match source_evidence(output, &claim.batch_id) {
            Some(true) => return "consumed",
            Some(false) => {}
            None => unknown = true,
        }
    }
    for work in history
        .work
        .iter()
        .filter(|e| text(e, "session_id") == claim.session_id)
    {
        if work["id"]
            .as_i64()
            .is_some_and(|id| id <= claim.work_cursor)
        {
            continue;
        }
        let measured = [
            "produced_qty",
            "total_waste",
            "finished_goods_kg",
            "finished_goods_meter",
            "rezka_bosma_waste",
            "rezka_lamination_waste",
            "rezka_edge_waste",
        ]
        .iter()
        .any(|key| work[*key].as_f64().is_some_and(|q| q != 0.0));
        if !measured {
            continue;
        }
        if let Some(matches) = source_evidence(work, &claim.batch_id) {
            if matches {
                return "consumed";
            }
        } else if text(work, "batch_id") == claim.batch_id {
            return "consumed";
        } else {
            // A legacy measurement without provenance cannot establish that
            // this input was untouched, even if its current WIP says waiting.
            unknown = true;
        }
    }
    if unknown { "unknown" } else { "unused" }
}

fn comparable(mut row: Value) -> Value {
    if let Some(fields) = row.as_object_mut() {
        for key in [
            "wip_status",
            "status_detail",
            "current_apparatus",
            "canonical_current_apparatus_id",
            "current_location",
            "used_by_session_id",
            "used_by_apparatus",
            "canonical_used_by_apparatus_id",
            "processed_by_session_id",
            "processed_by_apparatus",
            "canonical_processed_by_apparatus_id",
            "revision",
            "updated_at",
        ] {
            fields.remove(key);
        }
    }
    if let Some(payload) = row["payload_json"].as_object_mut() {
        payload.remove("wip_processed_at_unix");
        payload.remove("wip_in_use_at_unix");
    }
    row
}

fn ownership(row: &Value, session: &OrderRunSession) -> bool {
    matches!(text(row, "wip_status"), "in_use" | "processed")
        && text(row, "used_by_session_id") == session.session_id
        && ["", session.session_id.as_str()].contains(&text(row, "processed_by_session_id"))
        && ["canonical_used_by_apparatus_id", "used_by_apparatus"]
            .iter()
            .any(|key| text(row, key) == session.apparatus)
}

fn return_token(
    claim: &Claim,
    row: &Value,
    session: &OrderRunSession,
    actor: &QueueActionActor,
) -> String {
    format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&json!([
                claim.claim_id,
                claim.batch_id,
                row,
                session,
                actor.ref_,
                claim.map_json
            ]))
            .expect("WIP receipt serializes")
        )
    )
}

async fn project(
    tx: &mut Transaction<'_, Postgres>,
    claim: &Claim,
    h: &History,
    actor: &QueueActionActor,
) -> Result<ScannedWipInput, ProductionMapError> {
    let consumed = consumption(claim, h);
    let mut item = ScannedWipInput {
        session_id: claim.session_id.clone(),
        batch_id: claim.batch_id.clone(),
        qr_payload: claim.qr_payload.clone(),
        source_apparatus: claim.source_apparatus.clone(),
        source_kind: claim.source_kind.clone(),
        linked_at_unix: claim.linked_at_unix,
        worker_display_name: claim.worker_display_name.clone(),
        state: "unprocessed".into(),
        consume_state: consumed.into(),
        return_token: None,
        blocked_reason: String::new(),
    };
    if claim.returned_at_unix.is_some() {
        item.state = "returned".into();
        return Ok(item);
    }
    let Some(session) = h.sessions.iter().find(|s| s.session_id == claim.session_id) else {
        item.state = "returned".into();
        return Ok(item);
    };
    let links = domain::scanned_input_links(session)?;
    let link = links.iter().find(|l| l.input_batch_id == claim.batch_id);
    if consumed == "consumed" {
        item.state = "consumed".into();
    } else if link.is_some_and(|l| l.status == OrderRunInputStatus::InUse) {
        item.state = "active".into();
    }
    let reason = if consumed != "unused" {
        "work_recorded"
    } else if h.blocked {
        "order_blocked"
    } else if session.worker_ref != actor.ref_ || claim.worker_ref != actor.ref_ {
        "other_worker"
    } else if session.status != OrderRunStatus::Active || link.is_none() {
        "session_changed"
    } else if !claim.before_json.is_object() || claim.map_json != h.map {
        "checkpoint_unavailable"
    } else {
        ""
    };
    if !reason.is_empty() {
        item.blocked_reason = reason.into();
        return Ok(item);
    }
    let row = input_row(tx, &claim.source_kind, &claim.batch_id).await?;
    if !ownership(&row, session)
        || comparable(row.clone()) != comparable(claim.claimed_json.clone())
    {
        item.blocked_reason = "input_changed".into();
        return Ok(item);
    }
    if links.len() == 1 && !cancel_start::can_finish_scanned_session_tx(tx, session).await? {
        item.blocked_reason = "work_recorded".into();
        return Ok(item);
    }
    // Quantity-only raw-material consumption without a WIP output also blocks
    // reversal. This read is confined to the history request/return transaction.
    let material_work: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM mini_raw_material_events
        WHERE order_id=$1 AND canonical_apparatus_id=$2 AND event_type='consumption_posted'
        AND id>$3)",
    )
    .bind(&session.order_id)
    .bind(&session.apparatus)
    .bind(claim.material_cursor)
    .fetch_one(&mut **tx)
    .await
    .map_err(failed)?;
    if material_work {
        item.blocked_reason = "work_recorded".into();
    } else {
        item.return_token = Some(return_token(claim, &row, session, actor));
    }
    Ok(item)
}

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
