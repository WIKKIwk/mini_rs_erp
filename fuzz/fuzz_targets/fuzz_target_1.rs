#![no_main]

use std::collections::BTreeMap;

use libfuzzer_sys::fuzz_target;
use mini_rs_erp::core::production_map::queue_state::{
    ApparatusQueueAction, ApparatusQueueOrderState, apply_queue_action,
    apply_unordered_queue_action, effective_apparatus_sequence,
};

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }
    let order_count = (data[0] as usize % 8) + 1;
    let mut visible = Vec::with_capacity(order_count);
    for index in 0..order_count {
        visible.push(format!("zakaz-{index}"));
    }
    let mut stored = Vec::with_capacity(order_count);
    for byte in data.iter().skip(1).take(order_count * 2) {
        stored.push(format!("zakaz-{}", *byte as usize % order_count));
    }
    let sequence = effective_apparatus_sequence(&stored, &visible);
    if sequence.is_empty() {
        return;
    }

    let mut states = BTreeMap::new();
    for (index, id) in visible.iter().enumerate() {
        let raw = data.get(index + 2).copied().unwrap_or_default() % 3;
        let state = match raw {
            0 => ApparatusQueueOrderState::Pending,
            1 => ApparatusQueueOrderState::InProgress,
            _ => ApparatusQueueOrderState::Completed,
        };
        states.insert(id.clone(), state);
    }
    let target_index = data.last().copied().unwrap_or_default() as usize % sequence.len();
    let action = if data.get(1).copied().unwrap_or_default() % 2 == 0 {
        ApparatusQueueAction::Start
    } else {
        ApparatusQueueAction::Complete
    };
    let target = sequence[target_index].clone();
    let _ = apply_queue_action(&sequence, &mut states.clone(), &target, action);
    let _ = apply_unordered_queue_action(&mut states, &target, action);
});
