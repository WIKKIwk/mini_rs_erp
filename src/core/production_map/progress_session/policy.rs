use super::super::pechat;
use super::super::types::{ApparatusQueuePolicy, ApparatusQueuePolicyRecord};
use crate::core::apparatus_standard::{CanonicalApparatus, QueuePolicy};

fn canonical_queue_policy(apparatus: &CanonicalApparatus) -> Option<ApparatusQueuePolicy> {
    if apparatus.validate().is_err() {
        return None;
    }
    Some(match apparatus.policies.queue {
        QueuePolicy::StrictSequence => ApparatusQueuePolicy::StrictSequence,
        QueuePolicy::FreePick => ApparatusQueuePolicy::FreePick,
    })
}

pub(in crate::core::production_map) fn effective_apparatus_queue_policy(
    apparatus: &CanonicalApparatus,
    _stored: Option<ApparatusQueuePolicy>,
) -> ApparatusQueuePolicy {
    // The stored queue-policy row is a historical compatibility record. The
    // canonical apparatus policy is the only live authority; invalid
    // canonical data fails closed to the strictest queue behavior.
    canonical_queue_policy(apparatus).unwrap_or(ApparatusQueuePolicy::StrictSequence)
}

pub(in crate::core::production_map) fn effective_apparatus_queue_policy_record(
    apparatus: &CanonicalApparatus,
    _stored: ApparatusQueuePolicy,
) -> ApparatusQueuePolicyRecord {
    let canonical_policy = canonical_queue_policy(apparatus);
    let locked = canonical_policy.is_some_and(|_| pechat::is_pechat_apparatus(apparatus));
    ApparatusQueuePolicyRecord {
        apparatus_id: apparatus.identity.id.clone(),
        apparatus: apparatus.identity.display.display_name.clone(),
        policy: canonical_policy.unwrap_or(ApparatusQueuePolicy::StrictSequence),
        locked,
        reason: if canonical_policy.is_none() {
            "canonical_apparatus_invalid".to_string()
        } else if locked {
            "pechat_always_strict".to_string()
        } else {
            String::new()
        },
    }
}
