use std::collections::{BTreeMap, BTreeSet};

use super::service::ProductionMapService;
use super::service_capacity_scheduler::{
    ScheduledCandidate, candidate_allowed_for_order, canonical_apparatus_id,
    effective_duration_minutes, find_schedule_slot, fits_working_window,
    reservations_with_active_sessions, same_apparatus_id,
};
use super::store_port::ApparatusQueueStateMap;
use super::types::*;
use super::*;
use crate::core::apparatus_groups::ApparatusGroupService;
use crate::core::apparatus_standard::{CanonicalApparatus, CapabilityCode, WorkingWindow};

impl ProductionMapService {
    pub(super) async fn ensure_apparatus_execution_capacity(
        &self,
        apparatus: &str,
        order_id: &str,
        all_states: &ApparatusQueueStateMap,
    ) -> Result<(), ProductionMapError> {
        let order_id = order_id.trim();
        let apparatus_id =
            canonical_apparatus_id(apparatus).ok_or(ProductionMapError::CapacityProfileNotFound)?;
        let profile = self.canonical_capacity_profile_for(&apparatus_id).await?;
        let now = unix_seconds();
        if self
            .store
            .apparatus_downtimes()
            .await?
            .iter()
            .any(|downtime| {
                downtime.active
                    && same_apparatus_id(&downtime.apparatus_id, &profile.apparatus_id)
                    && downtime.starts_at_unix <= now
                    && now < downtime.ends_at_unix
            })
        {
            return Err(ProductionMapError::CapacityUnavailable);
        }
        if !fits_working_window(&profile, now, now + 60) {
            return Err(ProductionMapError::CapacityNoWorkingWindow);
        }

        let mut occupied_orders = BTreeSet::new();
        for (candidate_apparatus, states) in all_states {
            let candidate_id = canonical_apparatus_id(candidate_apparatus)
                .ok_or(ProductionMapError::StoreFailed)?;
            if !same_apparatus_id(&candidate_id, &profile.apparatus_id) {
                continue;
            }
            for (candidate_order_id, state) in states {
                if queue_state::ApparatusQueueOrderState::parse(state)
                    == Some(queue_state::ApparatusQueueOrderState::InProgress)
                    && !candidate_order_id.eq_ignore_ascii_case(order_id)
                {
                    occupied_orders.insert(candidate_order_id.trim().to_string());
                }
            }
        }
        for session in self.store.order_run_sessions_for_audit().await? {
            let session_apparatus_id = canonical_apparatus_id(&session.apparatus)
                .ok_or(ProductionMapError::StoreFailed)?;
            if session.status == OrderRunStatus::Active
                && same_apparatus_id(&session_apparatus_id, &profile.apparatus_id)
                && !session.order_id.eq_ignore_ascii_case(order_id)
            {
                occupied_orders.insert(session.order_id.trim().to_string());
            }
        }
        for reservation in self.store.apparatus_schedule_reservations().await? {
            if reservation.status.reserves_capacity()
                && same_apparatus_id(&reservation.apparatus_id, &profile.apparatus_id)
                && reservation.starts_at_unix <= now
                && now < reservation.ends_at_unix
                && !reservation.order_id.eq_ignore_ascii_case(order_id)
            {
                occupied_orders.insert(reservation.order_id.trim().to_string());
            }
        }
        if profile.finite_capacity && occupied_orders.len() >= usize::from(profile.capacity_slots) {
            return Err(ProductionMapError::CapacityConflict);
        }
        Ok(())
    }

    pub async fn apparatus_capacity_snapshot(
        &self,
    ) -> Result<ApparatusCapacitySnapshot, ProductionMapError> {
        let downtimes = self.store.apparatus_downtimes().await?;
        let reservations = self.store.apparatus_schedule_reservations().await?;
        let mut apparatus_ids = BTreeSet::new();
        for map in self.store.maps().await? {
            for stage in super::chain::linear_work_stages(&map) {
                let Some(apparatus) = stage.apparatus_id.as_deref() else {
                    continue;
                };
                apparatus_ids.insert(
                    canonical_apparatus_id(apparatus).ok_or(ProductionMapError::StoreFailed)?,
                );
            }
        }
        apparatus_ids.extend(
            downtimes
                .iter()
                .map(|downtime| downtime.apparatus_id.clone()),
        );
        apparatus_ids.extend(
            reservations
                .iter()
                .map(|reservation| reservation.apparatus_id.clone()),
        );
        let mut profiles = Vec::with_capacity(apparatus_ids.len());
        for apparatus_id in apparatus_ids {
            profiles.push(self.canonical_capacity_profile_for(&apparatus_id).await?);
        }
        Ok(ApparatusCapacitySnapshot {
            profiles,
            downtimes,
            reservations,
        })
    }

    pub async fn put_apparatus_capacity_profile(
        &self,
        profile: ApparatusCapacityProfile,
        apparatus_groups: &ApparatusGroupService,
    ) -> Result<ApparatusCapacityProfile, ProductionMapError> {
        let profile = normalize_capacity_profile(self.store.as_ref(), profile).await?;
        let canonical_profile = self
            .canonical_capacity_profile_for(&profile.apparatus_id)
            .await?;
        let current = apparatus_groups
            .canonical_apparatus_by_id(&profile.apparatus_id)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?
            .ok_or(ProductionMapError::StoreFailed)?;
        if profile.capabilities.iter().collect::<BTreeSet<_>>()
            != canonical_profile
                .capabilities
                .iter()
                .collect::<BTreeSet<_>>()
            || profile.capability_levels != canonical_profile.capability_levels
        {
            return Err(ProductionMapError::CapacityProfileInvalid);
        }
        let updated = apparatus_groups
            .mutate_canonical_apparatus(
                &profile.apparatus_id,
                current.versioning.revision,
                |canonical| {
                    canonical.capacity.capacity_slots = profile.capacity_slots;
                    canonical.capacity.setup_minutes = profile.setup_minutes;
                    canonical.capacity.cleanup_minutes = profile.cleanup_minutes;
                    canonical.capacity.efficiency_percent = profile.efficiency_percent;
                    canonical.capacity.finite_capacity = profile.finite_capacity;
                    canonical.capacity.working_windows = profile
                        .working_windows
                        .iter()
                        .map(|window| WorkingWindow {
                            weekday: window.weekday,
                            start_minute: window.start_minute,
                            end_minute: window.end_minute,
                        })
                        .collect();
                    Ok(())
                },
            )
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let profile = canonical_capacity_profile(&updated, unix_seconds())?;
        self.notify_live();
        Ok(profile)
    }

    pub async fn put_apparatus_downtime(
        &self,
        mut downtime: ApparatusDowntime,
    ) -> Result<ApparatusDowntime, ProductionMapError> {
        let canonical = self
            .validated_canonical_apparatus(&downtime.apparatus_id)
            .await?;
        downtime.apparatus = canonical.identity.display.display_name.clone();
        let downtime = normalize_downtime(self.store.as_ref(), downtime).await?;
        self.store.put_apparatus_downtime(downtime.clone()).await?;
        self.notify_live();
        Ok(downtime)
    }

    pub async fn schedule_apparatus_order(
        &self,
        input: ApparatusScheduleRequest,
    ) -> Result<ApparatusScheduleResult, ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        let input = normalize_schedule_request(self.store.as_ref(), input).await?;
        let apparatus_id = canonical_apparatus_id(&input.apparatus_id)
            .ok_or(ProductionMapError::ScheduleInputInvalid)?;
        self.validated_canonical_apparatus(&apparatus_id).await?;
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
            if existing.order_id.trim() != input.order_id || existing.apparatus_id != apparatus_id {
                return Err(ProductionMapError::ScheduleIdempotencyConflict);
            }
            return Ok(ApparatusScheduleResult {
                reservation: existing,
                conflicts: Vec::new(),
            });
        }

        let downtimes = self.store.apparatus_downtimes().await?;
        let audit_sessions = self.store.order_run_sessions_for_audit().await?;
        for session in &audit_sessions {
            let session_apparatus_id = canonical_apparatus_id(&session.apparatus)
                .ok_or(ProductionMapError::StoreFailed)?;
            self.validated_canonical_apparatus(&session_apparatus_id)
                .await?;
        }
        let reservations = reservations_with_active_sessions(
            &self.store.apparatus_schedule_reservations().await?,
            &audit_sessions,
        );
        let mut candidates = Vec::with_capacity(1 + input.candidate_apparatuses.len());
        candidates.push(ApparatusScheduleCandidate {
            apparatus_id: apparatus_id.clone(),
            apparatus: String::new(),
        });
        candidates.extend(input.candidate_apparatuses.clone());

        let mut route_candidate_count = 0;
        let mut supported_candidate_count = 0;
        let mut capability_not_supported = false;
        let mut capability_level_insufficient = false;
        let mut best_slot = None;
        for (candidate_index, candidate) in candidates.iter().enumerate() {
            if !candidate_allowed_for_order(&map, &apparatus_id, &candidate.apparatus_id) {
                continue;
            }
            route_candidate_count += 1;
            let canonical = self
                .validated_canonical_apparatus(&candidate.apparatus_id)
                .await?;
            let profile = canonical_capacity_profile(&canonical, unix_seconds())?;
            let candidate = ApparatusScheduleCandidate {
                apparatus_id: candidate.apparatus_id.clone(),
                apparatus: canonical.identity.display.display_name.clone(),
            };
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

impl ProductionMapService {
    async fn validated_canonical_apparatus(
        &self,
        apparatus_id: &crate::core::apparatus_standard::ApparatusId,
    ) -> Result<std::sync::Arc<CanonicalApparatus>, ProductionMapError> {
        let canonical = self.resolve_canonical_apparatus(apparatus_id).await?;
        if canonical.identity.id != *apparatus_id || canonical.validate().is_err() {
            return Err(ProductionMapError::StoreFailed);
        }
        Ok(canonical)
    }

    async fn canonical_capacity_profile_for(
        &self,
        apparatus_id: &crate::core::apparatus_standard::ApparatusId,
    ) -> Result<ApparatusCapacityProfile, ProductionMapError> {
        let canonical = self.validated_canonical_apparatus(apparatus_id).await?;
        canonical_capacity_profile(&canonical, unix_seconds())
    }
}

fn canonical_capacity_profile(
    canonical: &CanonicalApparatus,
    now_unix: i64,
) -> Result<ApparatusCapacityProfile, ProductionMapError> {
    if canonical.validate().is_err() {
        return Err(ProductionMapError::StoreFailed);
    }

    let mut capabilities = Vec::new();
    let mut capability_levels = BTreeMap::new();
    for capability in &canonical.capabilities {
        let code = capability_code_name(*capability);
        let profiles = canonical
            .capability_profiles
            .iter()
            .filter(|profile| profile.code == *capability)
            .collect::<Vec<_>>();
        if profiles.is_empty() {
            capabilities.push(code.to_string());
            capability_levels.insert(code.to_string(), 1);
            continue;
        }

        let active = profiles
            .into_iter()
            .filter(|profile| {
                profile.enabled
                    && profile
                        .valid_from_unix
                        .is_none_or(|starts_at| now_unix >= starts_at)
                    && profile
                        .valid_to_unix
                        .is_none_or(|ends_at| now_unix < ends_at)
            })
            .collect::<Vec<_>>();
        if active.len() > 1 {
            return Err(ProductionMapError::StoreFailed);
        }
        if let Some(profile) = active.first() {
            capabilities.push(code.to_string());
            capability_levels.insert(code.to_string(), profile.level);
        }
    }

    Ok(ApparatusCapacityProfile {
        apparatus_id: canonical.identity.id.clone(),
        apparatus: canonical.identity.display.display_name.clone(),
        capacity_slots: canonical.capacity.capacity_slots,
        setup_minutes: canonical.capacity.setup_minutes,
        cleanup_minutes: canonical.capacity.cleanup_minutes,
        efficiency_percent: canonical.capacity.efficiency_percent,
        finite_capacity: canonical.capacity.finite_capacity,
        working_windows: canonical
            .capacity
            .working_windows
            .iter()
            .map(|window| ApparatusWorkingWindow {
                weekday: window.weekday,
                start_minute: window.start_minute,
                end_minute: window.end_minute,
            })
            .collect(),
        capabilities,
        capability_levels,
        notes: String::new(),
        updated_at_unix: now_unix,
    })
}

fn capability_code_name(code: CapabilityCode) -> &'static str {
    match code {
        CapabilityCode::Print => "print",
        CapabilityCode::Pechat => "pechat",
        CapabilityCode::Flexo => "flexo",
        CapabilityCode::Laminate => "laminate",
        CapabilityCode::Cut => "cut",
        CapabilityCode::Package => "package",
        CapabilityCode::Glue => "glue",
        CapabilityCode::Apparatus => "apparatus",
    }
}

async fn normalize_capacity_profile(
    _store: &dyn ProductionMapStorePort,
    mut profile: ApparatusCapacityProfile,
) -> Result<ApparatusCapacityProfile, ProductionMapError> {
    profile.apparatus = profile.apparatus.trim().to_string();
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
    let mut levels = profile
        .capability_levels
        .into_iter()
        .map(|(code, level)| (code.trim().to_ascii_lowercase(), level.max(1)))
        .filter(|(code, _)| !code.is_empty())
        .collect::<std::collections::BTreeMap<_, _>>();
    for code in &profile.capabilities {
        levels.entry(code.clone()).or_insert(1);
    }
    profile.capability_levels = levels;
    profile.updated_at_unix = unix_seconds();
    Ok(profile)
}

async fn normalize_downtime(
    _store: &dyn ProductionMapStorePort,
    mut downtime: ApparatusDowntime,
) -> Result<ApparatusDowntime, ProductionMapError> {
    downtime.id = downtime.id.trim().to_string();
    downtime.apparatus = downtime.apparatus.trim().to_string();
    downtime.reason = downtime.reason.trim().to_string();
    if downtime.id.is_empty() {
        downtime.id = format!("apparatus-downtime:{}", unix_seconds());
    }
    if downtime.starts_at_unix <= 0
        || downtime.ends_at_unix <= downtime.starts_at_unix
        || downtime.reason.is_empty()
    {
        return Err(ProductionMapError::CapacityProfileInvalid);
    }
    if downtime.created_at_unix <= 0 {
        downtime.created_at_unix = unix_seconds();
    }
    Ok(downtime)
}

async fn normalize_schedule_request(
    _store: &dyn ProductionMapStorePort,
    mut input: ApparatusScheduleRequest,
) -> Result<ApparatusScheduleRequest, ProductionMapError> {
    input.order_id = input.order_id.trim().to_string();
    input.apparatus_id = input.apparatus_id.trim().to_string();
    input.apparatus = input.apparatus.trim().to_string();
    input.source = input.source.trim().to_string();
    input.reason = input.reason.trim().to_string();
    input.idempotency_key = input.idempotency_key.trim().to_string();
    if input.order_id.is_empty()
        || input.apparatus_id.is_empty()
        || input.duration_minutes == 0
        || input.duration_minutes > 30 * 24 * 60
        || input.earliest_start_unix <= 0
        || input.idempotency_key.is_empty()
        || input.idempotency_key.len() > 200
    {
        return Err(ProductionMapError::ScheduleInputInvalid);
    }
    let primary_id = canonical_apparatus_id(&input.apparatus_id)
        .ok_or(ProductionMapError::ScheduleInputInvalid)?;
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
    let mut seen_candidates = BTreeSet::new();
    let mut candidates = Vec::new();
    for mut candidate in input.candidate_apparatuses {
        candidate.apparatus = candidate.apparatus.trim().to_string();
        let key = candidate.apparatus_id.as_str().to_string();
        if candidate.apparatus_id != primary_id && seen_candidates.insert(key) {
            candidates.push(candidate);
        }
    }
    input.candidate_apparatuses = candidates;
    Ok(input)
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}
