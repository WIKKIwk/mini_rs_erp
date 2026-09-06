use super::{ApparatusQueueAction, ApparatusQueueOrderState, next_queue_state};
use crate::core::production_map::OrderControlState;

#[derive(Debug, Clone, Copy)]
pub(crate) enum QueueActionPolicyProfile {
    Live {
        order_control: OrderControlState,
        is_rezka: bool,
        merge_ready: bool,
    },
    Training {
        is_rezka: bool,
    },
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct QueueActionPolicyInput {
    pub(crate) state: ApparatusQueueOrderState,
    pub(crate) profile: QueueActionPolicyProfile,
    pub(crate) requeued_session: bool,
    pub(crate) pending_actionable: bool,
    pub(crate) queue_actionable: bool,
    pub(crate) start_ready: bool,
}

/// Produces the backend-owned action presentation contract from already
/// resolved runtime facts. State compatibility comes from the same transition
/// policy used by queue execution; apparatus, material, sequencing, and order
/// control facts only narrow that policy.
pub(crate) fn allowed_actions_for_control(
    input: QueueActionPolicyInput,
) -> Vec<ApparatusQueueAction> {
    use ApparatusQueueAction::*;
    let candidates: &[(ApparatusQueueAction, bool)] = match input.state {
        ApparatusQueueOrderState::Pending if input.requeued_session => {
            // Requeued work resumes from Pending without a normal Start transition.
            return if input.pending_actionable {
                vec![Resume]
            } else {
                Vec::new()
            };
        }
        ApparatusQueueOrderState::Pending => {
            &[(Start, input.pending_actionable && input.start_ready)]
        }
        ApparatusQueueOrderState::InProgress => match input.profile {
            QueueActionPolicyProfile::Live {
                order_control, is_rezka, merge_ready,
            } => &[
                (Pause, matches!(order_control,
                    OrderControlState::Active | OrderControlState::FreezeRequested)),
                (Freeze, order_control == OrderControlState::Active),
                (Merge, order_control == OrderControlState::Active && merge_ready),
                (RollComplete, order_control == OrderControlState::Active && is_rezka),
                (Complete, order_control == OrderControlState::Active),
            ],
            QueueActionPolicyProfile::Training { is_rezka } => &[
                (Pause, true),
                (DetachRoll, true),
                (Complete, true),
                (RollComplete, is_rezka),
            ],
        },
        ApparatusQueueOrderState::Paused => {
            let resume_allowed = match input.profile {
                QueueActionPolicyProfile::Live { order_control, .. } => {
                    order_control == OrderControlState::Active
                }
                QueueActionPolicyProfile::Training { .. } => true,
            };
            &[(Resume, input.queue_actionable && resume_allowed)]
        }
        ApparatusQueueOrderState::Frozen | ApparatusQueueOrderState::Completed => &[],
    };
    candidates
        .iter()
        .filter_map(|&(action, enabled)| {
            (enabled && next_queue_state(input.state, action).is_ok()).then_some(action)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(state: ApparatusQueueOrderState) -> QueueActionPolicyInput {
        QueueActionPolicyInput {
            state,
            profile: QueueActionPolicyProfile::Live {
                order_control: OrderControlState::Active,
                is_rezka: false,
                merge_ready: false,
            },
            requeued_session: false,
            pending_actionable: true,
            queue_actionable: true,
            start_ready: true,
        }
    }

    #[test]
    fn standard_controls_follow_execution_transition_order() {
        assert_eq!(
            allowed_actions_for_control(input(ApparatusQueueOrderState::Pending)),
            vec![ApparatusQueueAction::Start]
        );
        assert_eq!(
            allowed_actions_for_control(input(ApparatusQueueOrderState::InProgress)),
            vec![
                ApparatusQueueAction::Pause,
                ApparatusQueueAction::Freeze,
                ApparatusQueueAction::Complete,
            ]
        );
        assert_eq!(
            allowed_actions_for_control(input(ApparatusQueueOrderState::Paused)),
            vec![ApparatusQueueAction::Resume]
        );
        assert!(allowed_actions_for_control(input(ApparatusQueueOrderState::Frozen)).is_empty());
        assert!(allowed_actions_for_control(input(ApparatusQueueOrderState::Completed)).is_empty());
    }

    #[test]
    fn runtime_facts_only_narrow_state_compatible_actions() {
        let mut pending = input(ApparatusQueueOrderState::Pending);
        pending.start_ready = false;
        assert!(allowed_actions_for_control(pending).is_empty());

        let mut freeze_requested = input(ApparatusQueueOrderState::InProgress);
        freeze_requested.profile = QueueActionPolicyProfile::Live {
            order_control: OrderControlState::FreezeRequested,
            is_rezka: false,
            merge_ready: false,
        };
        assert_eq!(
            allowed_actions_for_control(freeze_requested),
            vec![ApparatusQueueAction::Pause]
        );

        let mut rezka = input(ApparatusQueueOrderState::InProgress);
        rezka.profile = QueueActionPolicyProfile::Live {
            order_control: OrderControlState::Active,
            is_rezka: true,
            merge_ready: true,
        };
        assert_eq!(
            allowed_actions_for_control(rezka),
            vec![
                ApparatusQueueAction::Pause,
                ApparatusQueueAction::Freeze,
                ApparatusQueueAction::Merge,
                ApparatusQueueAction::RollComplete,
                ApparatusQueueAction::Complete,
            ]
        );

        let mut laminatsiya = input(ApparatusQueueOrderState::InProgress);
        laminatsiya.profile = QueueActionPolicyProfile::Live {
            order_control: OrderControlState::Active,
            is_rezka: false,
            merge_ready: true,
        };
        assert_eq!(
            allowed_actions_for_control(laminatsiya),
            vec![
                ApparatusQueueAction::Pause,
                ApparatusQueueAction::Freeze,
                ApparatusQueueAction::Merge,
                ApparatusQueueAction::Complete,
            ]
        );
    }

    #[test]
    fn requeued_pending_resume_is_explicit_exception() {
        let mut requeued = input(ApparatusQueueOrderState::Pending);
        requeued.requeued_session = true;
        assert_eq!(
            allowed_actions_for_control(requeued),
            vec![ApparatusQueueAction::Resume]
        );
        requeued.pending_actionable = false;
        assert!(allowed_actions_for_control(requeued).is_empty());
    }

    #[test]
    fn training_profile_preserves_its_explicit_action_contract() {
        let mut training = input(ApparatusQueueOrderState::InProgress);
        training.profile = QueueActionPolicyProfile::Training { is_rezka: true };
        assert_eq!(
            allowed_actions_for_control(training),
            vec![
                ApparatusQueueAction::Pause,
                ApparatusQueueAction::DetachRoll,
                ApparatusQueueAction::Complete,
                ApparatusQueueAction::RollComplete,
            ]
        );
    }
}
