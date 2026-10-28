use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::{
    OrderRunSession, OrderRunStatus, ProductionMapError, ProductionMapService,
    ProductionOrderLogEntry, chain, queue_state::ApparatusQueueAction,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StageParticipant {
    pub display_name: String,
    pub started: bool,
    pub state: &'static str,
    #[serde(skip)]
    first_at: i64,
}

impl ProductionMapService {
    /// Returns people recorded on work actions or a persisted run at this
    /// exact order, stage and physical apparatus.
    pub async fn stage_participants(
        &self,
        order_id: &str,
        stage_node_id: &str,
        apparatus: &str,
    ) -> Result<Option<Vec<StageParticipant>>, ProductionMapError> {
        let order_id = order_id.trim();
        let stage_node_id = stage_node_id.trim();
        let apparatus = apparatus.trim();
        let Some(map) = self.raw_map(order_id).await? else {
            return Ok(None);
        };
        let stages = chain::linear_work_stages(&map);
        if !stages.iter().any(|stage| {
            stage.node_id.trim() == stage_node_id
                && stage.apparatus_id.as_deref() == Some(apparatus)
        }) {
            return Ok(None);
        }
        // Old records without a node ID are attributable only when the same
        // machine occurs once in the map.
        let unambiguous = stages
            .iter()
            .filter(|stage| stage.apparatus_id.as_deref() == Some(apparatus))
            .map(|stage| stage.node_id.trim())
            .collect::<BTreeSet<_>>()
            .len()
            == 1;
        let (sessions, logs) = tokio::try_join!(
            self.store.order_run_sessions_for_order(order_id),
            self.queue_action_logs_for_order(order_id),
        )?;
        Ok(Some(project_participants(
            stage_node_id,
            apparatus,
            unambiguous,
            &sessions,
            &logs,
        )))
    }
}

fn project_participants(
    stage: &str,
    apparatus: &str,
    unambiguous: bool,
    sessions: &[OrderRunSession],
    logs: &[ProductionOrderLogEntry],
) -> Vec<StageParticipant> {
    let matches = |record_apparatus: &str, record_stage: &str| {
        record_apparatus.trim() == apparatus
            && (record_stage.trim() == stage || (record_stage.trim().is_empty() && unambiguous))
    };
    let mut people = BTreeMap::<(String, String, String), StageParticipant>::new();
    let mut record = |role: &str, reference: &str, name: &str, time: i64, started: bool, state| {
        let role = role.trim();
        let reference = reference.trim();
        let name = name.trim();
        if reference.is_empty() && name.is_empty() {
            return;
        }
        let key = (
            role.to_string(),
            reference.to_string(),
            if reference.is_empty() {
                name.to_string()
            } else {
                String::new()
            },
        );
        let person = people.entry(key).or_insert_with(|| StageParticipant {
            display_name: name.to_string(),
            started: false,
            state: "participated",
            first_at: time,
        });
        if !name.is_empty() {
            person.display_name = name.to_string();
        }
        person.started |= started;
        if time > 0 && (person.first_at <= 0 || time < person.first_at) {
            person.first_at = time;
        }
        if state_rank(state) < state_rank(person.state) {
            person.state = state;
        }
    };

    for log in logs
        .iter()
        .filter(|log| matches(&log.apparatus, &log.stage_node_id))
    {
        let started_or_resumed = matches!(
            log.action,
            ApparatusQueueAction::Start | ApparatusQueueAction::Resume
        );
        // An admin may approve a completion on behalf of an operator. Count
        // other work actions only when their actor is an apparatus worker.
        let worker_action = log.actor_role.trim() == "aparatchi"
            && matches!(
                log.action,
                ApparatusQueueAction::Pause
                    | ApparatusQueueAction::DetachRoll
                    | ApparatusQueueAction::Merge
                    | ApparatusQueueAction::RollComplete
                    | ApparatusQueueAction::Complete
            );
        if started_or_resumed || worker_action {
            record(
                &log.actor_role,
                &log.actor_ref,
                &log.actor_display_name,
                log.created_at_unix,
                log.action == ApparatusQueueAction::Start,
                "participated",
            );
        }
    }
    for session in sessions
        .iter()
        .filter(|session| matches(&session.apparatus, &session.stage_node_id))
    {
        let state = match session.status {
            OrderRunStatus::Active => "working",
            OrderRunStatus::Paused | OrderRunStatus::RollDetached => "paused",
            OrderRunStatus::Frozen => "stopped",
            OrderRunStatus::Completed => "participated",
        };
        record(
            &session.worker_role,
            &session.worker_ref,
            &session.worker_display_name,
            session.started_at_unix,
            false,
            state,
        );
    }
    let mut participants = people.into_values().collect::<Vec<_>>();
    participants.sort_by(|a, b| {
        (state_rank(a.state), a.first_at, &a.display_name).cmp(&(
            state_rank(b.state),
            b.first_at,
            &b.display_name,
        ))
    });
    participants
}

fn state_rank(state: &str) -> u8 {
    match state {
        "working" => 0,
        "paused" => 1,
        "stopped" => 2,
        _ => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn log(
        name: &str,
        id: &str,
        action: &str,
        stage: &str,
        apparatus: &str,
    ) -> ProductionOrderLogEntry {
        serde_json::from_value(json!({
            "event_id": id, "order_id": "order-1", "apparatus": apparatus,
            "stage_node_id": stage, "action": action, "from_state": "paused",
            "to_state": "in_progress", "actor_role": "aparatchi", "actor_ref": id,
            "actor_display_name": name, "created_at_unix": 10
        }))
        .unwrap()
    }

    fn session(
        name: &str,
        id: &str,
        status: &str,
        stage: &str,
        apparatus: &str,
    ) -> OrderRunSession {
        serde_json::from_value(json!({
            "session_id": id, "order_id": "order-1", "apparatus": apparatus,
            "stage_node_id": stage, "status": status, "worker_role": "aparatchi",
            "worker_ref": id, "worker_display_name": name, "started_at_unix": 10,
            "updated_at_unix": 20, "payload_json": {}
        }))
        .unwrap()
    }

    #[test]
    fn stage_participants_distinguishes_starter_handoff_and_current_work() {
        let machine = "apparatus:test:print";
        let mut approval = log("Approver", "seven", "complete", "print", machine);
        approval.actor_role = "admin".into();
        let logs = vec![
            log("Ali", "one", "start", "print", machine),
            log("Vali", "two", "resume", "print", machine),
            log("Vali", "two", "resume", "print", machine),
            log("Hasan", "six", "roll_complete", "print", machine),
            log("Stopper", "three", "freeze", "print", machine),
            log("Other stage", "four", "start", "second", machine),
            log(
                "Other machine",
                "five",
                "start",
                "print",
                "apparatus:test:other",
            ),
            approval,
        ];
        let sessions = vec![session("Vali", "two", "active", "print", machine)];
        let people = project_participants("print", machine, false, &sessions, &logs);
        assert_eq!(people.len(), 3);
        assert_eq!(
            (&people[0].display_name, people[0].state, people[0].started),
            (&"Vali".to_string(), "working", false)
        );
        assert_eq!(
            (&people[1].display_name, people[1].state, people[1].started),
            (&"Ali".to_string(), "participated", true)
        );
        assert_eq!(people[2].display_name, "Hasan");
    }

    #[test]
    fn stage_participants_only_include_legacy_records_when_machine_is_unambiguous() {
        let machine = "apparatus:test:print";
        let sessions = vec![session("Legacy", "one", "paused", "", machine)];
        let logs = vec![log("Legacy", "one", "start", "", machine)];
        assert!(project_participants("print", machine, false, &sessions, &logs).is_empty());
        let people = project_participants("print", machine, true, &sessions, &logs);
        assert_eq!(people.len(), 1);
        assert_eq!((people[0].state, people[0].started), ("paused", true));
    }
}
