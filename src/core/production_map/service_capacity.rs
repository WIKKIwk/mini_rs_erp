use std::collections::BTreeSet;

use crate::core::apparatus_groups::{apparatus_id_for_name, apparatus_master_data_for_name};
use super::apparatus::move_allowed;
use super::service::ProductionMapService;
use super::types::*;
use super::*;

const MAX_SCHEDULE_HORIZON_MINUTES: u64 = 366 * 24 * 60;

impl ProductionMapService {
    pub async fn apparatus_capacity_snapshot(
        &self,
    ) -> Result<ApparatusCapacitySnapshot, ProductionMapError> {
        Ok(ApparatusCapacitySnapshot {
            profiles: self.store.apparatus_capacity_profiles().await?,
            downtimes: self.store.apparatus_downtimes().await?,
            reservations: self.store.apparatus_schedule_reservations().await?,
        })
    }

    pub async fn put_apparatus_capacity_profile(
        &self,
        profile: ApparatusCapacityProfile,
    ) -> Result<ApparatusCapacityProfile, ProductionMapError> {
        let profile = normalize_capacity_profile(profile)?;
        self.store
            .put_apparatus_capacity_profile(profile.clone())
            .await?;
        self.notify_live();
        Ok(profile)
    }

    pub async fn put_apparatus_downtime(
        &self,
        downtime: ApparatusDowntime,
    ) -> Result<ApparatusDowntime, ProductionMapError> {
        let downtime = normalize_downtime(downtime)?;
        self.store.put_apparatus_downtime(downtime.clone()).await?;
        self.notify_live();
        Ok(downtime)
    }

    pub async fn schedule_apparatus_order(
        &self,
        input: ApparatusScheduleRequest,
    ) -> Result<ApparatusScheduleResult, ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        let input = normalize_schedule_request(input)?;
        let map = self
            .store
            .maps()
            .await?
            .into_iter()
            .find(|map| map.id.trim() == input.order_id)
            .ok_or(ProductionMapError::MapNotFound)?;

        if let Some(existing) = self
            .store
            .apparatus_schedule_reservation_by_idempotency_key(&input.idempotency_key)
            .await?
        {
            if existing.order_id.trim() != input.order_id
                || existing.apparatus_id.trim() != input.apparatus_id
            {
                return Err(ProductionMapError::ScheduleIdempotencyConflict);
            }
            return Ok(ApparatusScheduleResult {
                reservation: existing,
                conflicts: Vec::new(),
            });
        }

        let profiles = self.store.apparatus_capacity_profiles().await?;
        let downtimes = self.store.apparatus_downtimes().await?;
        let reservations = self.store.apparatus_schedule_reservations().await?;
        let mut candidates = Vec::with_capacity(1 + input.candidate_apparatuses.len());
        candidates.push(ApparatusScheduleCandidate {
            apparatus_id: input.apparatus_id.clone(),
            apparatus: input.apparatus.clone(),
        });
        candidates.extend(input.candidate_apparatuses.clone());

        let mut route_candidate_count = 0;
        let mut supported_candidate_count = 0;
        let mut capability_not_supported = false;
        let mut capability_level_insufficient = false;
        let mut best_slot = None;
        for (candidate_index, candidate) in candidates.iter().enumerate() {
            if !candidate_allowed_for_order(&map, &input.apparatus, &candidate.apparatus) {
                continue;
            }
            route_candidate_count += 1;
            let profile = profile_for_apparatus(
                &profiles,
                &candidate.apparatus_id,
                &candidate.apparatus,
            );
            if !profile.supports(&input.capability_requirements) {
                let missing = input
                    .capability_requirements
                    .iter()
                    .find(|requirement| profile.capability_level(&requirement.code) == 0);
                if missing.is_some() {
                    capability_not_supported = true;
                } else {
                    capability_level_insufficient = true;
                }
                continue;
            }
            supported_candidate_count += 1;
            let reserved_duration_minutes =
                effective_duration_minutes(&profile, input.duration_minutes)?;
            let Ok((starts_at_unix, ends_at_unix)) = find_schedule_slot(
                &profile,
                &input,
                &candidate.apparatus_id,
                reserved_duration_minutes,
                &downtimes,
                &reservations,
            ) else {
                continue;
            };
            let is_better = best_slot.as_ref().is_none_or(|best: &ScheduledCandidate| {
                (starts_at_unix, candidate_index) < (best.starts_at_unix, best.index)
            });
            if is_better {
                best_slot = Some(ScheduledCandidate {
                    index: candidate_index,
                    candidate: candidate.clone(),
                    profile,
                    reserved_duration_minutes,
                    starts_at_unix,
                    ends_at_unix,
                });
            }
        }
        if route_candidate_count == 0 {
            return Err(ProductionMapError::MoveNotAllowed);
        }
        if let Some(best) = best_slot {
            let candidate = best.candidate;
            let profile = best.profile;
            let now = unix_seconds();
            let reservation = ApparatusScheduleReservation {
                reservation_id: format!("apparatus-reservation:{}", input.idempotency_key),
                idempotency_key: input.idempotency_key,
                order_id: input.order_id,
                apparatus_id: candidate.apparatus_id,
                apparatus: candidate.apparatus,
                starts_at_unix: best.starts_at_unix,
                ends_at_unix: best.ends_at_unix,
                requested_duration_minutes: input.duration_minutes,
                reserved_duration_minutes: best.reserved_duration_minutes,
                status: ApparatusScheduleStatus::Planned,
                priority: input.priority,
                source: input.source,
                reason: input.reason,
                capability_requirements: input.capability_requirements,
                actor: input.actor,
                created_at_unix: now,
            };
            let reservation = self
                .store
                .put_apparatus_schedule_reservation(
                    reservation,
                    profile.capacity_slots,
                    profile.finite_capacity,
                )
                .await?;
            self.notify_live();
            return Ok(ApparatusScheduleResult {
                reservation,
                conflicts: Vec::new(),
            });
        }
        if supported_candidate_count == 0 {
            return Err(if capability_not_supported {
                ProductionMapError::CapabilityNotSupported
            } else if capability_level_insufficient {
                ProductionMapError::CapabilityLevelInsufficient
            } else {
                ProductionMapError::CapacityNoWorkingWindow
            });
        }
        Err(ProductionMapError::CapacityNoWorkingWindow)
    }

    pub async fn cancel_apparatus_schedule_reservation(
        &self,
        input: ApparatusScheduleCancelRequest,
    ) -> Result<ApparatusScheduleReservation, ProductionMapError> {
        let reservation = self
            .store
            .cancel_apparatus_schedule_reservation(input)
            .await?;
        self.notify_live();
        Ok(reservation)
    }
}

fn normalize_capacity_profile(
    mut profile: ApparatusCapacityProfile,
) -> Result<ApparatusCapacityProfile, ProductionMapError> {
    profile.apparatus_id = profile.apparatus_id.trim().to_string();
    profile.apparatus = profile.apparatus.trim().to_string();
    if profile.apparatus_id.is_empty() && profile.apparatus.is_empty() {
        return Err(ProductionMapError::CapacityProfileInvalid);
    }
    if profile.apparatus_id.is_empty() {
        profile.apparatus_id = apparatus_id_for_name(&profile.apparatus);
    }
    if profile.apparatus.is_empty() {
        profile.apparatus = profile.apparatus_id.clone();
    }
    if profile.capacity_slots == 0 || profile.capacity_slots > 64
        || profile.efficiency_percent == 0
        || profile.efficiency_percent > 200
    {
        return Err(ProductionMapError::CapacityProfileInvalid);
    }
    for window in &profile.working_windows {
        if !(1..=7).contains(&window.weekday)
            || window.start_minute >= window.end_minute
            || window.end_minute > 1_440
        {
            return Err(ProductionMapError::CapacityProfileInvalid);
        }
    }
    let mut capabilities = BTreeSet::new();
    profile.capabilities = profile
        .capabilities
        .into_iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty() && capabilities.insert(value.clone()))
        .collect();
    // A capacity record is allowed to override levels and calendars, but an
    // empty capability list must not erase the apparatus master-data family.
    // This is especially important for Flexo: it is a printing work centre
    // even though it has no 7/8/9 colour-count suffix in its name.
    let inferred = apparatus_master_data_for_name(&profile.apparatus);
    for capability in inferred.capabilities {
        if capabilities.insert(capability.clone()) {
            profile.capabilities.push(capability);
        }
    }
    let mut levels = profile
        .capability_levels
        .into_iter()
        .map(|(code, level)| (code.trim().to_ascii_lowercase(), level.max(1)))
        .filter(|(code, _)| !code.is_empty())
        .collect::<std::collections::BTreeMap<_, _>>();
    for code in &profile.capabilities {
        levels.entry(code.clone()).or_insert(1);
    }
    for capability in inferred.capability_profiles {
        if capability.is_valid_at(unix_seconds()) {
            levels
                .entry(capability.code.trim().to_ascii_lowercase())
                .or_insert(capability.level.max(1));
        }
    }
    profile.capability_levels = levels;
    profile.updated_at_unix = unix_seconds();
    Ok(profile)
}

fn normalize_downtime(
    mut downtime: ApparatusDowntime,
) -> Result<ApparatusDowntime, ProductionMapError> {
    downtime.id = downtime.id.trim().to_string();
    downtime.apparatus_id = downtime.apparatus_id.trim().to_string();
    downtime.apparatus = downtime.apparatus.trim().to_string();
    downtime.reason = downtime.reason.trim().to_string();
    if downtime.id.is_empty() {
        downtime.id = format!("apparatus-downtime:{}", unix_seconds());
    }
    if downtime.apparatus_id.is_empty() && downtime.apparatus.is_empty()
        || downtime.starts_at_unix <= 0
        || downtime.ends_at_unix <= downtime.starts_at_unix
        || downtime.reason.is_empty()
    {
        return Err(ProductionMapError::CapacityProfileInvalid);
    }
    if downtime.apparatus_id.is_empty() {
        downtime.apparatus_id = apparatus_id_for_name(&downtime.apparatus);
    }
    if downtime.apparatus.is_empty() {
        downtime.apparatus = downtime.apparatus_id.clone();
    }
    if downtime.created_at_unix <= 0 {
        downtime.created_at_unix = unix_seconds();
    }
    Ok(downtime)
}

fn normalize_schedule_request(
    mut input: ApparatusScheduleRequest,
) -> Result<ApparatusScheduleRequest, ProductionMapError> {
    input.order_id = input.order_id.trim().to_string();
    input.apparatus_id = input.apparatus_id.trim().to_string();
    input.apparatus = input.apparatus.trim().to_string();
    input.source = input.source.trim().to_string();
    input.reason = input.reason.trim().to_string();
    input.idempotency_key = input.idempotency_key.trim().to_string();
    if input.order_id.is_empty()
        || input.apparatus_id.is_empty() && input.apparatus.is_empty()
        || input.duration_minutes == 0
        || input.duration_minutes > 30 * 24 * 60
        || input.earliest_start_unix <= 0
        || input.idempotency_key.is_empty()
        || input.idempotency_key.len() > 200
    {
        return Err(ProductionMapError::ScheduleInputInvalid);
    }
    if input.apparatus_id.is_empty() {
        input.apparatus_id = apparatus_id_for_name(&input.apparatus);
    }
    if input.apparatus.is_empty() {
        input.apparatus = input.apparatus_id.clone();
    }
    if let Some(latest_end) = input.latest_end_unix
        && latest_end <= input.earliest_start_unix
    {
        return Err(ProductionMapError::ScheduleInputInvalid);
    }
    let mut seen = BTreeSet::new();
    input.capability_requirements = input
        .capability_requirements
        .into_iter()
        .map(|mut requirement| {
            requirement.code = requirement.code.trim().to_ascii_lowercase();
            requirement.min_level = requirement.min_level.max(1);
            requirement
        })
        .filter(|requirement| {
            !requirement.code.is_empty() && seen.insert(requirement.code.clone())
        })
        .collect();
    let primary_id = input.apparatus_id.to_ascii_lowercase();
    let mut seen_candidates = BTreeSet::new();
    input.candidate_apparatuses = input
        .candidate_apparatuses
        .into_iter()
        .map(|mut candidate| {
            candidate.apparatus_id = candidate.apparatus_id.trim().to_string();
            candidate.apparatus = candidate.apparatus.trim().to_string();
            if candidate.apparatus_id.is_empty() && !candidate.apparatus.is_empty() {
                candidate.apparatus_id = apparatus_id_for_name(&candidate.apparatus);
            }
            if candidate.apparatus.is_empty() && !candidate.apparatus_id.is_empty() {
                candidate.apparatus = candidate.apparatus_id.clone();
            }
            candidate
        })
        .filter(|candidate| {
            let key = if candidate.apparatus_id.is_empty() {
                candidate.apparatus.to_ascii_lowercase()
            } else {
                candidate.apparatus_id.to_ascii_lowercase()
            };
            !key.is_empty()
                && key != primary_id
                && !seen_candidates.contains(&key)
                && seen_candidates.insert(key)
        })
        .collect();
    Ok(input)
}

#[derive(Debug, Clone)]
struct ScheduledCandidate {
    index: usize,
    candidate: ApparatusScheduleCandidate,
    profile: ApparatusCapacityProfile,
    reserved_duration_minutes: u32,
    starts_at_unix: i64,
    ends_at_unix: i64,
}

fn candidate_allowed_for_order(
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

fn profile_for_apparatus(
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
        profile
            .capability_levels
            .entry(capability)
            .or_insert(1);
    }
    profile
}

fn effective_duration_minutes(
    profile: &ApparatusCapacityProfile,
    duration_minutes: u32,
) -> Result<u32, ProductionMapError> {
    let run = (u64::from(duration_minutes) * 100
        + u64::from(profile.efficiency_percent)
        - 1)
        / u64::from(profile.efficiency_percent);
    let total = run
        .saturating_add(u64::from(profile.setup_minutes))
        .saturating_add(u64::from(profile.cleanup_minutes));
    u32::try_from(total).map_err(|_| ProductionMapError::ScheduleInputInvalid)
}

fn find_schedule_slot(
    profile: &ApparatusCapacityProfile,
    input: &ApparatusScheduleRequest,
    apparatus_id: &str,
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
                && intervals_overlap(
                    cursor,
                    end,
                    downtime.starts_at_unix,
                    downtime.ends_at_unix,
                )
        }) {
            cursor += 60;
            continue;
        }
        let conflicts = reservations
            .iter()
            .filter(|reservation| {
                reservation.status.reserves_capacity()
                    && reservation.apparatus_id.eq_ignore_ascii_case(apparatus_id)
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
