use crate::core::apparatus_groups::{apparatus_id_for_name, apparatus_master_data_for_name};

use super::apparatus::move_allowed;
use super::capacity::*;
use super::types::*;
use super::*;

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
    source: &str,
    candidate: &str,
) -> bool {
    let source = source.trim();
    if source.is_empty() {
        return false;
    }
    let source_is_in_route = map
        .nodes
        .iter()
        .filter(|node| node.kind == ProductionMapNodeKind::Apparatus)
        .map(|node| node.title.trim())
        .filter(|title| !title.is_empty())
        .any(|title| queue_state::apparatus_titles_match(title, source));
    source_is_in_route
        && (queue_state::apparatus_titles_match(source, candidate)
            || move_allowed(map, source, candidate))
}

pub(super) fn profile_for_apparatus(
    profiles: &[ApparatusCapacityProfile],
    apparatus_id: &str,
    apparatus: &str,
) -> ApparatusCapacityProfile {
    if let Some(profile) = profiles.iter().find(|profile| {
        profile.apparatus_id.eq_ignore_ascii_case(apparatus_id)
            || (!apparatus.is_empty() && profile.apparatus.eq_ignore_ascii_case(apparatus))
    }) {
        return profile.clone();
    }
    let mut profile = ApparatusCapacityProfile::default_for(apparatus_id, apparatus);
    let inferred = apparatus_master_data_for_name(apparatus);
    profile.capabilities = inferred.capabilities.clone();
    profile.capability_levels = inferred
        .capability_profiles
        .iter()
        .filter(|capability| capability.is_valid_at(unix_seconds()))
        .map(|capability| (capability.code.clone(), capability.level))
        .collect();
    for capability in inferred.capabilities {
        profile.capability_levels.entry(capability).or_insert(1);
    }
    profile
}

pub(super) fn effective_duration_minutes(
    profile: &ApparatusCapacityProfile,
    duration_minutes: u32,
) -> Result<u32, ProductionMapError> {
    let run = (u64::from(duration_minutes) * 100 + u64::from(profile.efficiency_percent) - 1)
        / u64::from(profile.efficiency_percent);
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
        if result.iter().any(|reservation| {
            reservation.status == ApparatusScheduleStatus::Active
                && reservation.order_id.trim() == session.order_id.trim()
                && queue_state::apparatus_titles_match(
                    &reservation.apparatus,
                    &session.apparatus,
                )
        }) {
            continue;
        }
        result.push(ApparatusScheduleReservation {
            reservation_id: format!("active-session:{}", session.session_id.trim()),
            idempotency_key: format!("active-session:{}", session.session_id.trim()),
            order_id: session.order_id.trim().to_string(),
            apparatus_id: apparatus_id_for_name(&session.apparatus),
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
    apparatus_id: &str,
    apparatus: &str,
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
                && downtime.apparatus_id.eq_ignore_ascii_case(apparatus_id)
                && intervals_overlap(cursor, end, downtime.starts_at_unix, downtime.ends_at_unix)
        }) {
            cursor += 60;
            continue;
        }
        let conflicts = reservations
            .iter()
            .filter(|reservation| {
                reservation.status.reserves_capacity()
                    && (reservation.apparatus_id.eq_ignore_ascii_case(apparatus_id)
                        || queue_state::apparatus_titles_match(
                            &reservation.apparatus,
                            apparatus,
                        ))
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

fn fits_working_window(profile: &ApparatusCapacityProfile, start: i64, end: i64) -> bool {
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

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}
