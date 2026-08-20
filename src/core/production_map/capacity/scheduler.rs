use super::capacity::*;
use super::types::*;
use super::*;
use crate::core::apparatus_standard::ApparatusId;

const MAX_SCHEDULE_HORIZON_MINUTES: u64 = 366 * 24 * 60;

#[derive(Debug, Clone)]
pub(super) struct ScheduledCandidate {
    pub(super) index: usize,
    pub(super) candidate: ApparatusScheduleCandidate,
    pub(super) profile: ApparatusCapacityProfile,
    pub(super) reserved_duration_minutes: u32,
    pub(super) starts_at_unix: i64,
    pub(super) ends_at_unix: i64,
}

pub(super) fn candidate_allowed_for_order(
    map: &ProductionMapDefinition,
    source_id: &ApparatusId,
    candidate_id: &ApparatusId,
) -> bool {
    let source_is_in_route = map
        .nodes
        .iter()
        .filter(|node| node.kind == ProductionMapNodeKind::Apparatus)
        .any(|node| canonical_apparatus_id(&node.apparatus_id).is_some_and(|id| id == *source_id));
    let candidate_is_in_route = map
        .nodes
        .iter()
        .filter(|node| node.kind == ProductionMapNodeKind::Apparatus)
        .any(|node| {
            canonical_apparatus_id(&node.apparatus_id).is_some_and(|id| id == *candidate_id)
        });
    source_is_in_route && candidate_is_in_route
}

pub(super) fn same_apparatus_id(left_id: &ApparatusId, right_id: &ApparatusId) -> bool {
    left_id == right_id
}

pub(super) fn canonical_apparatus_id(value: &str) -> Option<ApparatusId> {
    ApparatusId::new(value.trim().to_string()).ok()
}

#[cfg(test)]
pub(super) fn profile_for_apparatus(
    profiles: &[ApparatusCapacityProfile],
    apparatus_id: &ApparatusId,
) -> Option<ApparatusCapacityProfile> {
    profiles
        .iter()
        .find(|profile| same_apparatus_id(&profile.apparatus_id, apparatus_id))
        .cloned()
}

pub(super) fn effective_duration_minutes(
    profile: &ApparatusCapacityProfile,
    duration_minutes: u32,
) -> Result<u32, ProductionMapError> {
    let run = (u64::from(duration_minutes) * 100).div_ceil(u64::from(profile.efficiency_percent));
    let total = run
        .saturating_add(u64::from(profile.setup_minutes))
        .saturating_add(u64::from(profile.cleanup_minutes));
    u32::try_from(total).map_err(|_| ProductionMapError::ScheduleInputInvalid)
}

pub(super) fn reservations_with_active_sessions(
    reservations: &[ApparatusScheduleReservation],
    sessions: &[OrderRunSession],
) -> Vec<ApparatusScheduleReservation> {
    let mut result = reservations.to_vec();
    for session in sessions
        .iter()
        .filter(|session| session.status == OrderRunStatus::Active)
    {
        let Some(apparatus_id) = canonical_apparatus_id(&session.apparatus) else {
            continue;
        };
        if result.iter().any(|reservation| {
            reservation.status == ApparatusScheduleStatus::Active
                && reservation.order_id.trim() == session.order_id.trim()
                && same_apparatus_id(&reservation.apparatus_id, &apparatus_id)
        }) {
            continue;
        }
        result.push(ApparatusScheduleReservation {
            reservation_id: format!("active-session:{}", session.session_id.trim()),
            idempotency_key: format!("active-session:{}", session.session_id.trim()),
            order_id: session.order_id.trim().to_string(),
            apparatus_id,
            apparatus: session.apparatus.trim().to_string(),
            starts_at_unix: session.started_at_unix.max(60),
            ends_at_unix: i64::MAX,
            requested_duration_minutes: 1,
            reserved_duration_minutes: 1,
            status: ApparatusScheduleStatus::Active,
            priority: i32::MAX,
            source: "active_run_session".to_string(),
            reason: "active execution without a schedule reservation".to_string(),
            capability_requirements: Vec::new(),
            actor: QueueActionActor {
                role: session.worker_role.clone(),
                ref_: session.worker_ref.clone(),
                display_name: session.worker_display_name.clone(),
            },
            created_at_unix: session.started_at_unix,
        });
    }
    result
}

pub(super) fn find_schedule_slot(
    profile: &ApparatusCapacityProfile,
    input: &ApparatusScheduleRequest,
    apparatus_id: &ApparatusId,
    duration_minutes: u32,
    downtimes: &[ApparatusDowntime],
    reservations: &[ApparatusScheduleReservation],
) -> Result<(i64, i64), ProductionMapError> {
    let mut cursor = input.earliest_start_unix.max(60);
    cursor = ((cursor + 59) / 60) * 60;
    let horizon = cursor + (MAX_SCHEDULE_HORIZON_MINUTES as i64 * 60);
    while cursor < horizon {
        let end = cursor + i64::from(duration_minutes) * 60;
        if let Some(latest_end) = input.latest_end_unix
            && end > latest_end
        {
            return Err(ProductionMapError::CapacityNoWorkingWindow);
        }
        if !fits_working_window(profile, cursor, end) {
            cursor += 60;
            continue;
        }
        if downtimes.iter().any(|downtime| {
            downtime.active
                && same_apparatus_id(&downtime.apparatus_id, apparatus_id)
                && intervals_overlap(cursor, end, downtime.starts_at_unix, downtime.ends_at_unix)
        }) {
            cursor += 60;
            continue;
        }
        let conflicts = reservations
            .iter()
            .filter(|reservation| {
                reservation.status.reserves_capacity()
                    && same_apparatus_id(&reservation.apparatus_id, apparatus_id)
                    && intervals_overlap(
                        cursor,
                        end,
                        reservation.starts_at_unix,
                        reservation.ends_at_unix,
                    )
            })
            .count();
        if profile.finite_capacity && conflicts >= usize::from(profile.capacity_slots) {
            cursor += 60;
            continue;
        }
        return Ok((cursor, end));
    }
    Err(ProductionMapError::CapacityNoWorkingWindow)
}

pub(super) fn fits_working_window(
    profile: &ApparatusCapacityProfile,
    start: i64,
    end: i64,
) -> bool {
    if profile.working_windows.is_empty() {
        return true;
    }
    let Some(start_time) = time::OffsetDateTime::from_unix_timestamp(start).ok() else {
        return false;
    };
    let Some(end_time) = time::OffsetDateTime::from_unix_timestamp(end - 1).ok() else {
        return false;
    };
    let start_weekday = start_time.weekday().number_from_monday();
    let end_weekday = end_time.weekday().number_from_monday();
    if start_weekday != end_weekday {
        return false;
    }
    let start_minute = u16::from(start_time.hour()) * 60 + u16::from(start_time.minute());
    let end_minute = u16::from(end_time.hour()) * 60 + u16::from(end_time.minute()) + 1;
    profile.working_windows.iter().any(|window| {
        window.weekday == start_weekday
            && start_minute >= window.start_minute
            && end_minute <= window.end_minute
    })
}

fn intervals_overlap(left_start: i64, left_end: i64, right_start: i64, right_end: i64) -> bool {
    left_start < right_end && right_start < left_end
}

#[cfg(test)]
mod identity_tests {
    use std::collections::BTreeMap;

    use super::{profile_for_apparatus, same_apparatus_id};
    use crate::core::apparatus_standard::ApparatusId;
    use crate::core::production_map::{
        ApparatusCapacityProfile, ApparatusDowntime, ApparatusScheduleRequest,
        ApparatusScheduleReservation, ApparatusScheduleStatus, ApparatusWorkingWindow,
        QueueActionActor,
    };

    fn apparatus_id(value: &str) -> ApparatusId {
        ApparatusId::new(value).expect("canonical apparatus id")
    }

    #[test]
    fn canonical_ids_are_authoritative_over_display_text() {
        let left = apparatus_id("apparatus:catalog:a");
        let right = apparatus_id("apparatus:catalog:a");
        assert!(same_apparatus_id(&left, &right));
        let other = apparatus_id("apparatus:catalog:b");
        assert!(!same_apparatus_id(&left, &other));
    }

    #[test]
    fn missing_ids_never_match_by_display_name() {
        assert!(super::canonical_apparatus_id("").is_none());
        assert!(super::canonical_apparatus_id("Laminatsiya 1").is_none());
    }

    #[test]
    fn capacity_profile_lookup_requires_the_canonical_id() {
        let profile = ApparatusCapacityProfile {
            apparatus_id: apparatus_id("apparatus:catalog:flexo-001"),
            apparatus: "Renamed Flexo".to_string(),
            capacity_slots: 2,
            setup_minutes: 3,
            cleanup_minutes: 4,
            efficiency_percent: 100,
            finite_capacity: true,
            working_windows: vec![ApparatusWorkingWindow {
                weekday: 1,
                start_minute: 0,
                end_minute: 1_440,
            }],
            capabilities: vec!["flexo".to_string()],
            capability_levels: BTreeMap::new(),
            notes: String::new(),
            updated_at_unix: 1,
        };
        assert!(
            profile_for_apparatus(
                std::slice::from_ref(&profile),
                &apparatus_id("apparatus:catalog:flexo-001")
            )
            .is_some()
        );
        assert!(
            profile_for_apparatus(&[profile], &apparatus_id("apparatus:catalog:renamed-flexo"))
                .is_none()
        );
    }

    #[test]
    fn downtime_and_reservation_matching_ignores_display_snapshots() {
        let profile = ApparatusCapacityProfile {
            apparatus_id: apparatus_id("apparatus:catalog:flexo-001"),
            apparatus: "Current Flexo name".to_string(),
            capacity_slots: 1,
            setup_minutes: 0,
            cleanup_minutes: 0,
            efficiency_percent: 100,
            finite_capacity: true,
            working_windows: Vec::new(),
            capabilities: Vec::new(),
            capability_levels: BTreeMap::new(),
            notes: String::new(),
            updated_at_unix: 1,
        };
        let input = ApparatusScheduleRequest {
            order_id: "order-1".to_string(),
            apparatus_id: profile.apparatus_id.as_str().to_string(),
            apparatus: "Renamed Flexo".to_string(),
            earliest_start_unix: 1_700_000_040,
            latest_end_unix: None,
            duration_minutes: 10,
            priority: 0,
            source: String::new(),
            reason: String::new(),
            idempotency_key: "key-1".to_string(),
            capability_requirements: Vec::new(),
            candidate_apparatuses: Vec::new(),
            actor: QueueActionActor {
                role: String::new(),
                ref_: String::new(),
                display_name: String::new(),
            },
        };
        let downtime = ApparatusDowntime {
            id: "downtime-1".to_string(),
            apparatus_id: profile.apparatus_id.clone(),
            apparatus: "Historical Flexo name".to_string(),
            starts_at_unix: input.earliest_start_unix,
            ends_at_unix: input.earliest_start_unix + 600,
            reason: "maintenance".to_string(),
            active: true,
            actor: input.actor.clone(),
            created_at_unix: input.earliest_start_unix,
        };
        let reservation = ApparatusScheduleReservation {
            reservation_id: "reservation-1".to_string(),
            idempotency_key: "reservation-key-1".to_string(),
            order_id: "other-order".to_string(),
            apparatus_id: apparatus_id("apparatus:catalog:other-001"),
            apparatus: "Renamed Flexo".to_string(),
            starts_at_unix: input.earliest_start_unix,
            ends_at_unix: input.earliest_start_unix + 600,
            requested_duration_minutes: 10,
            reserved_duration_minutes: 10,
            status: ApparatusScheduleStatus::Planned,
            priority: 0,
            source: String::new(),
            reason: String::new(),
            capability_requirements: Vec::new(),
            actor: input.actor.clone(),
            created_at_unix: input.earliest_start_unix,
        };
        let delayed = super::find_schedule_slot(
            &profile,
            &input,
            &profile.apparatus_id,
            10,
            &[downtime],
            &[],
        )
        .expect("slot after downtime");
        assert_eq!(delayed.0, input.earliest_start_unix + 600);
        let not_delayed_by_same_name = super::find_schedule_slot(
            &profile,
            &input,
            &profile.apparatus_id,
            10,
            &[],
            &[reservation],
        )
        .expect("different ID is not a conflict");
        assert_eq!(not_delayed_by_same_name.0, input.earliest_start_unix);
    }
}
