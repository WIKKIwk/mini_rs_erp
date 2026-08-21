use super::super::pechat;
use super::super::types::{ApparatusQueuePolicy, ApparatusQueuePolicyRecord};
use crate::core::apparatus_standard::{QueueDiscipline, RuntimeApparatusConfiguration};

fn canonical_queue_policy(apparatus: &RuntimeApparatusConfiguration) -> ApparatusQueuePolicy {
    match apparatus.queue.discipline {
        QueueDiscipline::StrictSequence => ApparatusQueuePolicy::StrictSequence,
        QueueDiscipline::FreePick => ApparatusQueuePolicy::FreePick,
    }
}

pub(in crate::core::production_map) fn effective_apparatus_queue_policy(
    apparatus: &RuntimeApparatusConfiguration,
) -> ApparatusQueuePolicy {
    canonical_queue_policy(apparatus)
}

pub(in crate::core::production_map) fn effective_apparatus_queue_policy_record(
    apparatus: &RuntimeApparatusConfiguration,
) -> ApparatusQueuePolicyRecord {
    let canonical_policy = canonical_queue_policy(apparatus);
    let locked = pechat::is_pechat_apparatus(apparatus);
    ApparatusQueuePolicyRecord {
        apparatus_id: apparatus.runtime.apparatus_id.clone(),
        apparatus: apparatus.runtime.display.display_name.clone(),
        policy: canonical_policy,
        locked,
        reason: if locked {
            "pechat_always_strict".to_string()
        } else {
            String::new()
        },
    }
}
