use super::*;
use crate::core::production_map::ProductionMapError;

fn symbolic_state(selector: u8) -> ApparatusQueueOrderState {
    match selector % 3 {
        0 => ApparatusQueueOrderState::Pending,
        1 => ApparatusQueueOrderState::InProgress,
        _ => ApparatusQueueOrderState::Completed,
    }
}

#[kani::proof]
fn queue_state_transition_matches_policy() {
    let current = symbolic_state(kani::any::<u8>());
    let action = if kani::any::<bool>() {
        ApparatusQueueAction::Start
    } else {
        ApparatusQueueAction::Complete
    };
    let next = next_queue_state(current, action);
    match (current, action) {
        (ApparatusQueueOrderState::Pending, ApparatusQueueAction::Start) => {
            assert_eq!(next, Ok(ApparatusQueueOrderState::InProgress));
        }
        (ApparatusQueueOrderState::InProgress, ApparatusQueueAction::Complete) => {
            assert_eq!(next, Ok(ApparatusQueueOrderState::Completed));
        }
        _ => {
            assert_eq!(next, Err(ProductionMapError::QueueActionNotAllowed));
        }
    }
}
