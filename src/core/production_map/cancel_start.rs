//! Undo only an untouched, worker-owned WIP Start with a durable checkpoint.
use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) const CHECKPOINT: &str = "_cancel_start";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelStartControl {
    pub session_id: String,
    pub token: String,
    pub worker_ref: String,
    pub previous_state: queue_state::ApparatusQueueOrderState,
}

pub(crate) fn fingerprint(session: &OrderRunSession) -> String {
    let mut session = session.clone();
    if let Some(payload) = session.payload_json.as_object_mut() {
        payload.remove(CHECKPOINT);
    }
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&session).expect("session serializes"))
    )
}

pub(crate) fn control(session: &OrderRunSession) -> Option<CancelStartControl> {
    let checkpoint = session.payload_json.get(CHECKPOINT)?;
    let control: CancelStartControl =
        serde_json::from_value(checkpoint.get("control")?.clone()).ok()?;
    (session.status == OrderRunStatus::Active
        && session.worker_role == "aparatchi"
        && !session.worker_ref.is_empty()
        && control.worker_ref == session.worker_ref
        && control.session_id == session.session_id
        && matches!(
            control.previous_state,
            queue_state::ApparatusQueueOrderState::Pending
                | queue_state::ApparatusQueueOrderState::Completed
        )
        && control.token == fingerprint(session))
    .then_some(control)
}

pub(crate) fn audit_event(
    session: &OrderRunSession,
    actor: &QueueActionActor,
    payload: serde_json::Value,
) -> OrderProgressEvent {
    super::service_progress_support::zero_quantity_event(
        super::service_progress_support::ProgressRecordContext {
            session,
            apparatus: &session.apparatus,
            order_id: &session.order_id,
            action: queue_state::ApparatusQueueAction::Pause,
            actor,
        },
        String::new(),
        String::new(),
        payload,
    )
}

impl ProductionMapService {
    #[allow(clippy::too_many_arguments)]
    pub async fn cancel_started_work(
        &self,
        apparatus: &str,
        order_id: &str,
        session_id: &str,
        token: &str,
        assigned: &[String],
        actor: QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        if !super::temporary_pause::valid_actor(
            apparatus, order_id, session_id, token, &actor, assigned,
        ) {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let canonical = self.resolve_canonical_apparatus_text(apparatus).await?;
        if !super::apparatus::requires_previous_stage(&canonical) {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let _guard = self.apparatus_action_guard(apparatus).await;
        self.store
            .cancel_started_work(apparatus, order_id, session_id, token, &actor, assigned)
            .await?;
        self.notify_live();
        Ok(())
    }
}
