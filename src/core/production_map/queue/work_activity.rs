use serde::Serialize;

use super::progress::{OrderRunSession, OrderRunStatus};
use crate::core::production_map::queue_state::ApparatusQueueOrderState;

/// Actor-independent projection, shared by REST, live snapshots and mutations.
/// The client compares this durable owner with its authenticated worker; the
/// shared snapshot cache must never depend on the requesting worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApparatusQueueWorkActivity {
    pub worker_role: String,
    pub worker_ref: String,
    pub state: ApparatusQueueOrderState,
}

impl ApparatusQueueWorkActivity {
    pub fn from_session(
        session: Option<&OrderRunSession>,
        state: ApparatusQueueOrderState,
        stage_node_id: &str,
    ) -> Option<Self> {
        let session = session?;
        if !matches!(
            state,
            ApparatusQueueOrderState::InProgress | ApparatusQueueOrderState::Paused
        ) || !matches!(
            session.status,
            OrderRunStatus::Active | OrderRunStatus::Paused | OrderRunStatus::RollDetached
        )
            || session.worker_role.trim().is_empty()
            || session.worker_ref.trim().is_empty()
            || (!session.stage_node_id.trim().is_empty()
                && session.stage_node_id.trim() != stage_node_id.trim())
            || session.payload_json.get("requeued_at_tail")
                .and_then(serde_json::Value::as_bool) == Some(true)
        {
            return None;
        }
        Some(Self {
            worker_role: session.worker_role.trim().to_string(),
            worker_ref: session.worker_ref.trim().to_string(),
            state,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> OrderRunSession {
        OrderRunSession {
            session_id: "run-1".into(),
            apparatus: "apparatus:test:1".into(),
            order_id: "order-1".into(),
            stage_node_id: "stage-1".into(),
            status: OrderRunStatus::Active,
            worker_role: "aparatchi".into(),
            worker_ref: "worker-1".into(),
            worker_display_name: "Same name".into(),
            started_at_unix: 1,
            updated_at_unix: 1,
            payload_json: serde_json::json!({}),
        }
    }

    #[test]
    fn work_activity_is_owned_and_independent_of_apparatus_type() {
        for apparatus in [
            "apparatus:print", "apparatus:lamination", "apparatus:rezka", "apparatus:other",
        ] {
            let mut run = session();
            run.apparatus = apparatus.into();
            for (status, state) in [
                (OrderRunStatus::Active, ApparatusQueueOrderState::InProgress),
                (OrderRunStatus::Paused, ApparatusQueueOrderState::Paused),
                (OrderRunStatus::RollDetached, ApparatusQueueOrderState::Paused),
            ] {
                run.status = status;
                let activity = ApparatusQueueWorkActivity::from_session(
                    Some(&run), state, "stage-1",
                ).unwrap();
                assert_eq!(activity.worker_ref, "worker-1");
                assert_eq!(activity.state, state);
                run.worker_ref = "worker-2".into();
                assert_ne!(Some(activity), ApparatusQueueWorkActivity::from_session(
                    Some(&run), state, "stage-1",
                ));
                run.worker_ref = "worker-1".into();
            }
        }
    }

    #[test]
    fn work_activity_ignores_missing_released_completed_and_wrong_stage_sessions() {
        let mut run = session();
        let project = |run: &OrderRunSession| ApparatusQueueWorkActivity::from_session(
            Some(run), ApparatusQueueOrderState::InProgress, "stage-1",
        );
        assert!(ApparatusQueueWorkActivity::from_session(
            None, ApparatusQueueOrderState::InProgress, "stage-1",
        ).is_none());
        for state in [
            ApparatusQueueOrderState::Pending, ApparatusQueueOrderState::Completed,
            ApparatusQueueOrderState::Frozen,
        ] {
            assert!(ApparatusQueueWorkActivity::from_session(
                Some(&run), state, "stage-1",
            ).is_none());
        }
        for status in [OrderRunStatus::Completed, OrderRunStatus::Frozen] {
            run.status = status;
            assert!(project(&run).is_none());
        }
        run = session();
        run.payload_json = serde_json::json!({"requeued_at_tail": true});
        assert!(project(&run).is_none());
        run = session();
        run.stage_node_id = "old-stage".into();
        assert!(project(&run).is_none());
        run = session();
        run.worker_ref.clear();
        assert!(project(&run).is_none());
        run = session();
        run.worker_role.clear();
        assert!(project(&run).is_none());
    }
}
