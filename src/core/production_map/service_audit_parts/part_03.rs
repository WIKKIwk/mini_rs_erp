
fn audit_capacity(
    known_orders: &BTreeSet<String>,
    profiles: &[ApparatusCapacityProfile],
    downtimes: &[ApparatusDowntime],
    reservations: &[ApparatusScheduleReservation],
    violations: &mut Vec<ProductionWorkflowAuditViolation>,
) {
    let mut profile_keys = BTreeSet::new();
    for profile in profiles {
        let key = profile.apparatus_id.as_str().to_string();
        if !profile_keys.insert(key) {
            violations.push(ProductionWorkflowAuditViolation::new(
                "duplicate_capacity_profile",
                "",
                &profile.apparatus,
                "each apparatus must have at most one capacity profile",
            ));
        }
        if profile.capacity_slots == 0
            || profile.efficiency_percent == 0
            || profile.efficiency_percent > 200
        {
            violations.push(ProductionWorkflowAuditViolation::new(
                "invalid_capacity_profile",
                "",
                &profile.apparatus,
                "capacity slots and efficiency must be within valid bounds",
            ));
        }
    }
    for downtime in downtimes {
        if downtime.id.trim().is_empty()
            || downtime.apparatus_id.as_str().trim().is_empty()
            || downtime.starts_at_unix <= 0
            || downtime.ends_at_unix <= downtime.starts_at_unix
        {
            violations.push(ProductionWorkflowAuditViolation::new(
                "invalid_apparatus_downtime",
                "",
                &downtime.id,
                "downtime must identify an apparatus and have a positive interval",
            ));
        }
    }

    let mut reservation_keys = BTreeSet::new();
    for reservation in reservations {
        let reservation_id = reservation.reservation_id.trim();
        let order_id = reservation.order_id.trim();
        if !known_orders.contains(order_id) {
            violations.push(ProductionWorkflowAuditViolation::new(
                "unknown_order_schedule_reservation",
                order_id,
                reservation_id,
                "schedule reservation references an order that is not present in production maps",
            ));
        }
        if reservation_id.is_empty()
            || reservation.idempotency_key.trim().is_empty()
            || reservation.apparatus_id.as_str().trim().is_empty()
            || reservation.starts_at_unix <= 0
            || reservation.ends_at_unix <= reservation.starts_at_unix
            || reservation.reserved_duration_minutes == 0
        {
            violations.push(ProductionWorkflowAuditViolation::new(
                "invalid_schedule_reservation",
                order_id,
                reservation_id,
                "schedule reservation identity, apparatus, and interval are required",
            ));
        }
        if !reservation_keys.insert(reservation.idempotency_key.trim().to_ascii_lowercase()) {
            violations.push(ProductionWorkflowAuditViolation::new(
                "duplicate_schedule_idempotency_key",
                order_id,
                reservation_id,
                "schedule idempotency keys must be unique",
            ));
        }
    }

    for reservation in reservations
        .iter()
        .filter(|reservation| reservation.status.reserves_capacity())
    {
        let same_apparatus = reservations.iter().filter(|other| {
            other.status.reserves_capacity()
                && other.apparatus_id == reservation.apparatus_id
                && other.starts_at_unix < reservation.ends_at_unix
                && reservation.starts_at_unix < other.ends_at_unix
        });
        let overlap_count = same_apparatus.count();
        let capacity_slots = profiles
            .iter()
            .find(|profile| profile.apparatus_id == reservation.apparatus_id)
            .map(|profile| profile.capacity_slots)
            .unwrap_or(1);
        if overlap_count > usize::from(capacity_slots) {
            violations.push(ProductionWorkflowAuditViolation::new(
                "capacity_overbooked",
                reservation.order_id.trim(),
                reservation.apparatus.trim(),
                "overlapping planned or active reservations exceed apparatus capacity",
            ));
        }
    }
}

fn queue_state_for_apparatus_order(
    queue_states: &BTreeMap<String, BTreeMap<String, String>>,
    apparatus: &str,
    order_id: &str,
) -> Option<ApparatusQueueOrderState> {
    queue_states
        .iter()
        .find(|(stored_apparatus, _)| queue_state::apparatus_ids_match(stored_apparatus, apparatus))
        .and_then(|(_, states)| states.get(order_id.trim()))
        .and_then(|state| ApparatusQueueOrderState::parse(state))
}
