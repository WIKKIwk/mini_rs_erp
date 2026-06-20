use super::super::pechat;
use super::super::types::{ApparatusQueuePolicy, ApparatusQueuePolicyRecord};

pub(in crate::core::production_map) fn effective_apparatus_queue_policy(
    apparatus: &str,
    stored: Option<ApparatusQueuePolicy>,
) -> ApparatusQueuePolicy {
    if pechat::pechat_color_count(apparatus).is_some() {
        ApparatusQueuePolicy::StrictSequence
    } else {
        stored.unwrap_or(ApparatusQueuePolicy::StrictSequence)
    }
}

pub(in crate::core::production_map) fn effective_apparatus_queue_policy_record(
    apparatus: &str,
    stored: ApparatusQueuePolicy,
) -> ApparatusQueuePolicyRecord {
    let locked = pechat::pechat_color_count(apparatus).is_some();
    ApparatusQueuePolicyRecord {
        apparatus: apparatus.trim().to_string(),
        policy: if locked {
            ApparatusQueuePolicy::StrictSequence
        } else {
            stored
        },
        locked,
        reason: if locked {
            "pechat_always_strict".to_string()
        } else {
            String::new()
        },
    }
}
