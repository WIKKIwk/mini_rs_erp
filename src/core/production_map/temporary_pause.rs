//! Temporarily remove unfinished work without producing an output roll.
use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) const MARKER: &str = "_temporary_pause";
pub(crate) const REVISION: &str = "_temporary_pause_revision";
pub(crate) const EXPECTED_SESSION: &str = "temporary_pause_expected_session";

/// Separate from allowed_actions so older clients retain their strict parser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemporaryPauseControl {
    pub session_id: String,
    pub token: String,
}

pub(crate) fn control(session: &OrderRunSession) -> Option<TemporaryPauseControl> {
    if session.status != OrderRunStatus::Active
        || session.worker_role != "aparatchi"
        || session.worker_ref.trim().is_empty()
        || session.order_id.starts_with("training-")
        || !session.payload_json.is_object()
        || session.payload_json.get(MARKER).is_some()
        || ["lamination_astatka_raw_released"].iter().any(|key| {
            session
                .payload_json
                .get(key)
                .and_then(serde_json::Value::as_bool)
                == Some(true)
        })
    {
        return None;
    }
    let token = format!("{:x}", Sha256::digest(serde_json::to_vec(session).ok()?));
    Some(TemporaryPauseControl {
        session_id: session.session_id.clone(),
        token,
    })
}

pub(crate) fn is_temporarily_paused(session: &OrderRunSession) -> bool {
    session.status == OrderRunStatus::Paused
        && session.payload_json.get(MARKER).is_some_and(|marker| {
            marker
                .get("revision")
                .and_then(serde_json::Value::as_u64)
                .is_some_and(|revision| {
                    revision > 0
                        && session
                            .payload_json
                            .get(REVISION)
                            .and_then(serde_json::Value::as_u64)
                            == Some(revision)
                })
                && marker
                    .get("paused_at_unix")
                    .and_then(serde_json::Value::as_i64)
                    .is_some()
        })
}

pub(crate) fn receipt_id(token: &str) -> String {
    format!("temporary-pause:{token}")
}

/// The temporary anchor opens one visible neighbour, never another paused job.
pub(crate) fn next_order_after_suspension<'a>(
    sequence: &'a [String],
    states: &std::collections::BTreeMap<String, queue_state::ApparatusQueueOrderState>,
    anchors: &std::collections::BTreeSet<String>,
) -> Option<&'a str> {
    use queue_state::ApparatusQueueOrderState as S;
    // A passed colour trial still has PrintPreflight queue state. Its own
    // official Start must remain reachable; the ordinary occupancy and hold
    // checks reject a trial belonging to any other order.
    if states.values().any(|state| *state == S::InProgress) {
        return None;
    }
    let index = sequence.iter().position(|id| anchors.contains(id.trim()))?;
    let next = sequence[index + 1..]
        .iter()
        .map(|id| id.trim())
        .find(|id| {
            !id.is_empty()
                && !matches!(
                    states.get(*id).copied().unwrap_or(S::Pending),
                    S::Completed | S::Frozen
                )
        })?;
    matches!(
        states.get(next).copied().unwrap_or(S::Pending),
        S::Pending | S::PrintPreflight
    )
    .then_some(next)
}

/// A stopped cycle keeps its inputs reserved; Resume must not claim a new roll.
pub(crate) async fn held_inputs(
    service: &ProductionMapService,
    session: &OrderRunSession,
) -> Result<serde_json::Value, ProductionMapError> {
    let lineage = order_run_input_links_from_payload(&session.payload_json)
        .map_err(|_| ProductionMapError::ProgressBatchNotResumable)?;
    let current = super::service_progress_support::session_progress_links(session);
    let mut inputs: Vec<(String, String)> = lineage
        .into_iter()
        .filter(|link| link.status == OrderRunInputStatus::InUse)
        .map(|link| (link.source_kind.as_str().to_string(), link.input_batch_id))
        .collect::<Vec<_>>();
    if inputs.is_empty() && !current.batch_id.is_empty() {
        inputs.push((current.source_kind.clone(), current.batch_id.clone()));
    }
    if let Some(ids) = session
        .payload_json
        .get("_reactivation_reserved_outputs")
        .and_then(serde_json::Value::as_array)
    {
        for id in ids {
            inputs.push((
                "progress_batch".into(),
                id.as_str()
                    .ok_or(ProductionMapError::ProgressBatchNotResumable)?
                    .into(),
            ));
        }
    }
    let mut witnesses = Vec::new();
    for (kind, id) in inputs {
        let valid = if kind == "opening_wip" {
            service
                .store
                .opening_wip_batch(&id, "")
                .await?
                .is_some_and(|record| {
                    record.intake.status == OpeningWipIntakeStatus::Confirmed
                        && record.intake.order_id == session.order_id
                        && record.batch.wip_status == OpeningWipBatchStatus::InUse
                        && record.batch.used_by_session_id == session.session_id
                        && record.batch.used_by_apparatus == session.apparatus
                })
        } else {
            service
                .store
                .progress_batch(&id)
                .await?
                .is_some_and(|batch| {
                    batch.order_id == session.order_id
                        && batch.wip_status == OrderProgressBatchWipStatus::InUse
                        && batch.used_by_session_id == session.session_id
                        && batch.used_by_apparatus == session.apparatus
                })
        };
        if !valid {
            return Err(ProductionMapError::ProgressBatchNotResumable);
        }
        witnesses.push(serde_json::json!({"source_kind":kind,"batch_id":id}));
    }
    Ok(serde_json::json!(witnesses))
}

pub(crate) fn valid_actor(
    apparatus: &str,
    order_id: &str,
    session_id: &str,
    token: &str,
    actor: &QueueActionActor,
    assigned: &[String],
) -> bool {
    actor.role == "aparatchi"
        && !actor.ref_.trim().is_empty()
        && !order_id.trim().is_empty()
        && !order_id.starts_with("training-")
        && !session_id.is_empty()
        && session_id.len() <= 512
        && token.len() == 64
        && token.bytes().all(|b| b.is_ascii_hexdigit())
        && assigned.iter().any(|id| id == apparatus)
}

pub(crate) fn suspension_write(
    current: &OrderRunSession,
    session_id: &str,
    token: &str,
    actor: &QueueActionActor,
    assigned: &[String],
    mut states: std::collections::BTreeMap<String, String>,
    policy: ApparatusQueuePolicy,
) -> Result<QueueActionProgressWrite, ProductionMapError> {
    let expected = control(current).ok_or(ProductionMapError::QueueActionNotAllowed)?;
    if expected.session_id != session_id
        || expected.token != token
        || !valid_actor(
            &current.apparatus,
            &current.order_id,
            session_id,
            token,
            actor,
            assigned,
        )
        || states.get(&current.order_id).map(String::as_str) != Some("in_progress")
    {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    let now = super::progress::unix_seconds();
    let revision = current
        .payload_json
        .get(REVISION)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(ProductionMapError::QueueActionNotAllowed)?;
    let mut session = current.clone();
    // Legacy reversible resumes may not yet have their physical-roll clock.
    // Capture it before changing updated_at or discarding the Resume checkpoint.
    super::service_progress_support::ensure_wip_started_at(&mut session);
    session.status = OrderRunStatus::Paused;
    session.updated_at_unix = now;
    session.payload_json[REVISION] = serde_json::json!(revision);
    session.payload_json[MARKER] = serde_json::json!({"revision": revision, "paused_at_unix": now});
    session
        .payload_json
        .as_object_mut()
        .map(|payload| payload.remove(super::re_pause::CHECKPOINT_KEY));
    states.insert(current.order_id.clone(), "paused".into());
    let payload = serde_json::json!({"event":"temporary_pause", "token":token,
        "session_id":session_id, "paused_at_unix":now, "revision":revision});
    let mut progress_event = super::service_progress_support::zero_quantity_event(
        super::service_progress_support::ProgressRecordContext {
            session: &session,
            apparatus: &current.apparatus,
            order_id: &current.order_id,
            action: queue_state::ApparatusQueueAction::Pause,
            actor,
        },
        String::new(),
        String::new(),
        payload.clone(),
    );
    let event_id = receipt_id(token);
    progress_event.event_id = format!("progress:{event_id}");
    let mut event_payload = payload;
    event_payload[EXPECTED_SESSION] =
        serde_json::to_value(current).map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(QueueActionProgressWrite {
        apparatus: current.apparatus.clone(),
        states,
        event: ApparatusQueueActionEvent {
            event_id,
            apparatus: current.apparatus.clone(),
            order_id: current.order_id.clone(),
            stage_node_id: current.stage_node_id.clone(),
            action: queue_state::ApparatusQueueAction::Pause,
            from_state: queue_state::ApparatusQueueOrderState::InProgress,
            to_state: queue_state::ApparatusQueueOrderState::Paused,
            policy,
            actor: actor.clone(),
            assigned_apparatus: assigned.to_vec(),
            payload_json: event_payload,
        },
        session: Some(session),
        progress_event: Some(progress_event),
        map_update: None,
        sequence_updates: Default::default(),
        progress_batch: None,
        progress_batches: Vec::new(),
        progress_batch_updates: Vec::new(),
        opening_wip_batch_updates: Vec::new(),
        raw_material_stock_transitions: Vec::new(),
        qolip_checkouts: Vec::new(),
        returned_paint_report: None,
        order_control_update: None,
        schedule_reservation_status: Some(ApparatusScheduleStatus::Paused),
        print_preflight_hold_id: None,
        print_preflight_cancel_hold_id: None,
    })
}

/// Preserve every cycle/input/report field; only occupancy and audit change.
pub(crate) fn resume(
    session: &mut OrderRunSession,
    actor: &QueueActionActor,
    now: i64,
) -> Result<serde_json::Value, ProductionMapError> {
    if !is_temporarily_paused(session) {
        return Err(ProductionMapError::ProgressBatchNotResumable);
    }
    let marker = session
        .payload_json
        .get(MARKER)
        .cloned()
        .ok_or(ProductionMapError::ProgressBatchNotResumable)?;
    session.payload_json.as_object_mut().map(|payload| {
        payload.remove(MARKER);
        payload.remove(super::re_pause::CHECKPOINT_KEY);
    });
    session.payload_json["_temporary_pause_last_interval"] = serde_json::json!({
        "paused_at_unix":marker["paused_at_unix"], "resumed_at_unix":now, "revision":marker["revision"]});
    session.status = OrderRunStatus::Active;
    session.worker_role = actor.role.clone();
    session.worker_ref = actor.ref_.clone();
    session.worker_display_name = actor.display_name.clone();
    session.updated_at_unix = now;
    Ok(
        serde_json::json!({"event":"temporary_resume", "interval":session.payload_json["_temporary_pause_last_interval"]}),
    )
}

impl ProductionMapService {
    pub async fn temporarily_pause_work(
        &self,
        apparatus: &str,
        order_id: &str,
        session_id: &str,
        token: &str,
        assigned: &[String],
        actor: QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        if !valid_actor(apparatus, order_id, session_id, token, &actor, assigned) {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let began = std::time::Instant::now();
        let canonical = self.resolve_canonical_apparatus_text(apparatus).await?;
        let resolved = std::time::Instant::now();
        // Detaching one unfinished roll changes only this apparatus's occupancy.
        // Keep broad map/freeze mutations excluded without waiting for unrelated
        // worker actions on other machines. Store-side order/resource locks and
        // the expected session still protect the actual commit.
        let _guard = self.apparatus_action_guard(apparatus).await;
        let locked = std::time::Instant::now();
        self.store
            .temporarily_pause_work(
                apparatus,
                order_id,
                session_id,
                token,
                &actor,
                assigned,
                super::progress::effective_apparatus_queue_policy(&canonical),
            )
            .await?;
        self.notify_live();
        tracing::info!(
            apparatus,
            order_id,
            resolve_ms = resolved.duration_since(began).as_secs_f64() * 1_000.0,
            guard_wait_ms = locked.duration_since(resolved).as_secs_f64() * 1_000.0,
            store_ms = locked.elapsed().as_secs_f64() * 1_000.0,
            total_ms = began.elapsed().as_secs_f64() * 1_000.0,
            "temporary pause committed"
        );
        Ok(())
    }
}
