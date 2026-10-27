//! A reversible, output-free stop of the most recent ordinary Resume.
use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) const CHECKPOINT_KEY: &str = "_resume_pause";

/// Additive presentation field: older clients reject unknown allowed_actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumePauseControl {
    pub resume_id: String,
    pub worker_role: String,
    pub worker_ref: String,
}

#[derive(Serialize, Deserialize)]
struct Checkpoint {
    control: ResumePauseControl,
    previous_status: OrderRunStatus,
    previous_payload: serde_json::Value,
    resumed_fingerprint: String,
}

fn fingerprint(session: &OrderRunSession) -> String {
    let mut snapshot = session.clone();
    if let Some(payload) = snapshot.payload_json.as_object_mut() {
        payload.remove(CHECKPOINT_KEY);
    }
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&snapshot).expect("session serializes"))
    )
}

/// Ordinary work continuation is reversible by the worker who resumed it,
/// including when a different worker owned the paused session. Input claims,
/// explicit shift handoffs, unfreezes and resource reacquisition retain their
/// existing stop workflows.
pub(crate) fn attach_checkpoint(
    previous: &OrderRunSession,
    prepared: &mut PreparedApparatusQueueAction,
) {
    use queue_state::{ApparatusQueueAction as Action, ApparatusQueueOrderState as State};
    if prepared.event.action != Action::Resume
        || prepared.event.from_state != State::Paused
        || !matches!(
            previous.status,
            OrderRunStatus::Paused | OrderRunStatus::RollDetached
        )
        || previous.worker_role != "aparatchi"
        || previous.worker_ref.trim().is_empty()
        || previous.worker_role != prepared.event.actor.role
        || prepared.event.actor.ref_.trim().is_empty()
        || prepared.progress_batch.is_some()
        || !prepared.progress_batches.is_empty()
        || !prepared.progress_batch_updates.is_empty()
        || !prepared.opening_wip_batch_updates.is_empty()
        || [
            "worker_handoff",
            "frozen_order",
            "freeze_with_issue",
            "requeued_at_tail",
            "lamination_astatka_shift_closed",
            "lamination_astatka_raw_released",
            "qolip_released_on_freeze",
        ]
        .iter()
        .any(|key| {
            previous
                .payload_json
                .get(key)
                .and_then(serde_json::Value::as_bool)
                == Some(true)
        })
    {
        return;
    }
    let Some(session) = &mut prepared.session else {
        return;
    };
    if session.session_id != previous.session_id
        || session.status != OrderRunStatus::Active
        || session.worker_role != prepared.event.actor.role
        || session.worker_ref != prepared.event.actor.ref_
    {
        return;
    }
    let mut previous_payload = previous.payload_json.clone();
    if let Some(payload) = previous_payload.as_object_mut() {
        payload.remove(CHECKPOINT_KEY);
    }
    let checkpoint = Checkpoint {
        control: ResumePauseControl {
            resume_id: prepared.event.event_id.clone(),
            worker_role: session.worker_role.clone(),
            worker_ref: session.worker_ref.clone(),
        },
        previous_status: previous.status,
        previous_payload,
        resumed_fingerprint: fingerprint(session),
    };
    session.payload_json[CHECKPOINT_KEY] =
        serde_json::to_value(checkpoint).expect("checkpoint serializes");
    // The storage transaction must compare the pre-resume session as well;
    // a stale preparation must never overwrite newly received material.
    prepared.event.payload_json["resume_pause_expected_session"] =
        serde_json::to_value(previous).expect("session serializes");
}

fn checkpoint(session: &OrderRunSession) -> Option<Checkpoint> {
    let checkpoint: Checkpoint =
        serde_json::from_value(session.payload_json.get(CHECKPOINT_KEY)?.clone()).ok()?;
    (session.status == OrderRunStatus::Active
        && checkpoint.control.worker_role == session.worker_role
        && checkpoint.control.worker_ref == session.worker_ref
        && checkpoint.resumed_fingerprint == fingerprint(session))
    .then_some(checkpoint)
}

pub(crate) fn control(session: &OrderRunSession) -> Option<ResumePauseControl> {
    checkpoint(session).map(|checkpoint| checkpoint.control)
}

pub(crate) fn restore(
    session: &OrderRunSession,
    resume_id: &str,
    actor: &QueueActionActor,
) -> Result<OrderRunSession, ProductionMapError> {
    let checkpoint = checkpoint(session).ok_or(ProductionMapError::QueueActionNotAllowed)?;
    if checkpoint.control.resume_id != resume_id
        || actor.role != "aparatchi"
        || checkpoint.control.worker_role != actor.role
        || checkpoint.control.worker_ref != actor.ref_
        || !matches!(
            checkpoint.previous_status,
            OrderRunStatus::Paused | OrderRunStatus::RollDetached
        )
    {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    let mut restored = OrderRunSession {
        status: checkpoint.previous_status,
        payload_json: checkpoint.previous_payload,
        updated_at_unix: super::progress::unix_seconds(),
        ..session.clone()
    };
    // Keep the cutting optimistic revision monotonic across resume/cancel cycles.
    if let Some(revision) = session
        .payload_json
        .get("rezka_output_revision")
        .and_then(serde_json::Value::as_u64)
    {
        restored.payload_json["rezka_output_revision"] = serde_json::json!(revision + 1);
    }
    Ok(restored)
}

pub(crate) fn cancellation_write(
    current: &OrderRunSession,
    resume_id: &str,
    actor: &QueueActionActor,
    assigned_apparatus: &[String],
    mut states: std::collections::BTreeMap<String, String>,
    policy: ApparatusQueuePolicy,
) -> Result<QueueActionProgressWrite, ProductionMapError> {
    use queue_state::{ApparatusQueueAction as Action, ApparatusQueueOrderState as State};
    let session = restore(current, resume_id, actor)?;
    states.insert(current.order_id.clone(), "paused".into());
    let payload = serde_json::json!({"event":"re_pause", "resume_id":resume_id});
    let mut progress_event = super::service_progress_support::zero_quantity_event(
        super::service_progress_support::ProgressRecordContext {
            session: &session,
            apparatus: &current.apparatus,
            order_id: &current.order_id,
            action: Action::Pause,
            actor,
        },
        String::new(),
        String::new(),
        payload.clone(),
    );
    let event_id = format!("re-pause:{resume_id}");
    progress_event.event_id = format!("progress:{event_id}");
    Ok(QueueActionProgressWrite {
        apparatus: current.apparatus.clone(),
        states,
        event: ApparatusQueueActionEvent {
            event_id,
            apparatus: current.apparatus.clone(),
            order_id: current.order_id.clone(),
            stage_node_id: current.stage_node_id.clone(),
            action: Action::Pause,
            from_state: State::InProgress,
            to_state: State::Paused,
            policy,
            actor: actor.clone(),
            assigned_apparatus: assigned_apparatus.to_vec(),
            payload_json: payload,
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

impl ProductionMapService {
    pub async fn re_pause_resumed_work(
        &self,
        apparatus: &str,
        order_id: &str,
        resume_id: &str,
        assigned_apparatus: &[String],
        actor: QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        if actor.role != "aparatchi"
            || actor.ref_.trim().is_empty()
            || order_id.trim().is_empty()
            || order_id.starts_with("training-")
            || resume_id.is_empty()
            || resume_id.len() > 512
            || !assigned_apparatus.iter().any(|id| id == apparatus)
        {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        self.resolve_canonical_apparatus_text(apparatus).await?;
        let _guard = self.queue_action_guard().await;
        self.store
            .re_pause_resumed_work(apparatus, order_id, resume_id, &actor, assigned_apparatus)
            .await?;
        self.notify_live();
        Ok(())
    }
}
