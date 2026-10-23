use super::*;

impl ProductionMapLiveSnapshot {
    /// One display order for every reader. Stored sequences and their versions
    /// retain their scheduling meaning; role filters and limits run afterwards.
    pub(super) fn sort_free_pick_visible_orders(&mut self) {
        let ordered = self
            .queue_policies
            .iter()
            .filter_map(|policy| {
                if policy.policy != ApparatusQueuePolicy::FreePick {
                    return None;
                }
                let apparatus = policy.apparatus_id.as_str();
                let visible = self.visible_order_ids.get(apparatus)?;
                let mut ids = queue_state::effective_apparatus_sequence(
                    self.sequences
                        .get(apparatus)
                        .map(Vec::as_slice)
                        .unwrap_or(&[]),
                    visible,
                );
                // Stable ties keep the canonical order, including across accounts.
                ids.sort_by_key(|id| {
                    let control = self
                        .queue_action_controls
                        .get(apparatus)
                        .and_then(|c| c.get(id));
                    let state = self
                        .queue_states
                        .get(apparatus)
                        .and_then(|s| s.get(id))
                        .map(String::as_str)
                        .or_else(|| control.map(|c| c.state.as_str()))
                        .unwrap_or("pending");
                    let red = self
                        .order_controls
                        .get(id)
                        .is_some_and(|c| c.state != OrderControlState::Active)
                        || self.order_statuses.get(id).is_some_and(|s| {
                            matches!(s.order_status.as_str(), "frozen" | "completed_with_issue")
                                || s.completed_with_issue_count > 0
                        })
                        || state == "frozen";
                    let priority = if red {
                        3
                    } else if matches!(state, "in_progress" | "print_preflight") {
                        0
                    } else if state == "paused" {
                        1
                    } else {
                        2
                    };
                    (
                        priority,
                        std::cmp::Reverse(control.map(|c| c.last_worked_at_unix).unwrap_or(0)),
                    )
                });
                Some((apparatus.to_string(), ids))
            })
            .collect::<BTreeMap<_, _>>();
        self.visible_order_ids.extend(ordered);
    }

    pub(crate) fn worker_queue_order_hidden(&self, id: &str, hide_issues: bool) -> bool {
        // The operator must still be able to acknowledge a requested stop.
        let freeze_requested = self
            .order_controls
            .get(id)
            .is_some_and(|c| c.state == OrderControlState::FreezeRequested);
        self.order_controls
            .get(id)
            .is_some_and(|c| c.state == OrderControlState::Frozen)
            || self.order_statuses.get(id).is_some_and(|s| {
                s.order_status == "frozen"
                    || (hide_issues
                        && !freeze_requested
                        && (s.order_status == "completed_with_issue"
                            || s.completed_with_issue_count > 0))
            })
            || self
                .queue_states
                .values()
                .any(|states| states.get(id).is_some_and(|s| s == "frozen"))
            || self.queue_action_controls.values().any(|controls| {
                controls
                    .get(id)
                    .is_some_and(|c| c.state.as_str() == "frozen")
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::apparatus_standard::ApparatusId;

    #[test]
    fn display_groups_follow_machine_colors_and_keep_strict_sequences() {
        let apparatus = "apparatus:test:display";
        let sequence = [
            "frozen",
            "neutral-b",
            "issue",
            "paused",
            "preflight",
            "active",
            "neutral-a",
            "stop",
            "issue-count",
            "other-machine",
        ]
        .map(str::to_string)
        .to_vec();
        let visible = sequence.iter().rev().cloned().collect::<Vec<_>>();
        let mut stop = OrderControlRecord::active("stop");
        stop.state = OrderControlState::FreezeRequested;
        let mut snapshot = ProductionMapLiveSnapshot {
            maps: vec![],
            sequence_versions: BTreeMap::new(),
            sequence_revisions: BTreeMap::new(),
            sequence_positions_by_apparatus: BTreeMap::new(),
            stage_states: BTreeMap::new(),
            frozen_orders_by_apparatus: BTreeMap::new(),
            sequences: BTreeMap::from([(apparatus.into(), sequence.clone())]),
            visible_order_ids: BTreeMap::from([(apparatus.into(), visible.clone())]),
            queue_states: BTreeMap::from([(
                apparatus.into(),
                [
                    ("frozen", "frozen"),
                    ("paused", "paused"),
                    ("preflight", "print_preflight"),
                    ("active", "in_progress"),
                    ("stop", "in_progress"),
                ]
                .into_iter()
                .map(|(id, state)| (id.into(), state.into()))
                .collect(),
            )]),
            queue_policies: vec![ApparatusQueuePolicyRecord {
                apparatus_id: ApparatusId::new(apparatus).unwrap(),
                apparatus: apparatus.into(),
                policy: ApparatusQueuePolicy::FreePick,
                locked: false,
                reason: String::new(),
            }],
            queue_action_controls: BTreeMap::new(),
            order_controls: BTreeMap::from([("stop".into(), stop)]),
            order_statuses: BTreeMap::from([
                (
                    "issue".into(),
                    ProductionOrderStatusDetail {
                        order_status: "completed_with_issue".into(),
                        ..Default::default()
                    },
                ),
                (
                    "issue-count".into(),
                    ProductionOrderStatusDetail {
                        completed_with_issue_count: 1,
                        ..Default::default()
                    },
                ),
                (
                    "other-machine".into(),
                    ProductionOrderStatusDetail {
                        order_status: "in_progress".into(),
                        ..Default::default()
                    },
                ),
            ]),
        };
        snapshot.sort_free_pick_visible_orders();
        assert_eq!(
            snapshot.visible_order_ids[apparatus],
            [
                "preflight",
                "active",
                "paused",
                "neutral-b",
                "neutral-a",
                "other-machine",
                "frozen",
                "issue",
                "stop",
                "issue-count",
            ]
        );
        assert_eq!(snapshot.sequences[apparatus], sequence);
        assert!(snapshot.worker_queue_order_hidden("frozen", true));
        assert!(snapshot.worker_queue_order_hidden("issue", true));
        assert!(snapshot.worker_queue_order_hidden("issue-count", true));
        assert!(!snapshot.worker_queue_order_hidden("stop", true));
        assert!(!snapshot.worker_queue_order_hidden("issue", false));
        snapshot.queue_policies[0].policy = ApparatusQueuePolicy::StrictSequence;
        snapshot
            .visible_order_ids
            .insert(apparatus.into(), visible.clone());
        snapshot.sort_free_pick_visible_orders();
        assert_eq!(snapshot.visible_order_ids[apparatus], visible);
        assert_eq!(snapshot.sequences[apparatus], sequence);
    }
}
