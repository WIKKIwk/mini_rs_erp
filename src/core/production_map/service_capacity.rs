use std::collections::BTreeSet;

use super::service::ProductionMapService;
use super::service_capacity_scheduler::{
    ScheduledCandidate, candidate_allowed_for_order, effective_duration_minutes,
    find_schedule_slot, profile_for_apparatus,
};
use super::types::*;
use super::*;
use crate::core::apparatus_groups::{apparatus_id_for_name, apparatus_master_data_for_name};

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
            let profile =
                profile_for_apparatus(&profiles, &candidate.apparatus_id, &candidate.apparatus);
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
    if profile.capacity_slots == 0
        || profile.capacity_slots > 64
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
        .filter(|requirement| !requirement.code.is_empty() && seen.insert(requirement.code.clone()))
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

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}
