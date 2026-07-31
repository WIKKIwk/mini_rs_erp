use super::*;

use super::super::queue_state;

pub(super) async fn apparatus_capacity_profiles(
    store: &MemoryProductionMapStore,
) -> Result<Vec<ApparatusCapacityProfile>, ProductionMapError> {
    Ok(store
        .apparatus_capacity_profiles
        .read()
        .await
        .values()
        .cloned()
        .collect())
}

pub(super) async fn put_apparatus_capacity_profile(
    store: &MemoryProductionMapStore,
    profile: ApparatusCapacityProfile,
) -> Result<(), ProductionMapError> {
    store
        .apparatus_capacity_profiles
        .write()
        .await
        .insert(profile.apparatus_id.trim().to_string(), profile);
    Ok(())
}

pub(super) async fn apparatus_downtimes(
    store: &MemoryProductionMapStore,
) -> Result<Vec<ApparatusDowntime>, ProductionMapError> {
    Ok(store
        .apparatus_downtimes
        .read()
        .await
        .values()
        .cloned()
        .collect())
}

pub(super) async fn put_apparatus_downtime(
    store: &MemoryProductionMapStore,
    downtime: ApparatusDowntime,
) -> Result<(), ProductionMapError> {
    store
        .apparatus_downtimes
        .write()
        .await
        .insert(downtime.id.trim().to_string(), downtime);
    Ok(())
}

pub(super) async fn apparatus_schedule_reservations(
    store: &MemoryProductionMapStore,
) -> Result<Vec<ApparatusScheduleReservation>, ProductionMapError> {
    Ok(store
        .apparatus_schedule_reservations
        .read()
        .await
        .values()
        .cloned()
        .collect())
}

pub(super) async fn apparatus_schedule_reservation_by_idempotency_key(
    store: &MemoryProductionMapStore,
    idempotency_key: &str,
) -> Result<Option<ApparatusScheduleReservation>, ProductionMapError> {
    Ok(store
        .apparatus_schedule_reservations
        .read()
        .await
        .values()
        .find(|reservation| reservation.idempotency_key == idempotency_key.trim())
        .cloned())
}

pub(super) async fn put_apparatus_schedule_reservation(
    store: &MemoryProductionMapStore,
    reservation: ApparatusScheduleReservation,
    capacity_slots: u16,
) -> Result<ApparatusScheduleReservation, ProductionMapError> {
    let mut reservations = store.apparatus_schedule_reservations.write().await;
    if let Some(existing) = reservations
        .values()
        .find(|item| item.idempotency_key == reservation.idempotency_key)
        .cloned()
    {
        if existing.order_id != reservation.order_id
            || existing.apparatus_id != reservation.apparatus_id
        {
            return Err(ProductionMapError::ScheduleIdempotencyConflict);
        }
        return Ok(existing);
    }
    let conflicts = reservations
        .values()
        .filter(|item| {
            item.status.reserves_capacity()
                && item.apparatus_id.eq_ignore_ascii_case(&reservation.apparatus_id)
                && item.starts_at_unix < reservation.ends_at_unix
                && reservation.starts_at_unix < item.ends_at_unix
        })
        .count();
    if capacity_slots == 0 || conflicts >= usize::from(capacity_slots) {
        return Err(ProductionMapError::CapacityConflict);
    }
    reservations.insert(reservation.reservation_id.clone(), reservation.clone());
    Ok(reservation)
}

pub(super) async fn cancel_apparatus_schedule_reservation(
    store: &MemoryProductionMapStore,
    input: ApparatusScheduleCancelRequest,
) -> Result<ApparatusScheduleReservation, ProductionMapError> {
    let mut reservations = store.apparatus_schedule_reservations.write().await;
    let reservation = reservations
        .get_mut(input.reservation_id.trim())
        .ok_or(ProductionMapError::ScheduleReservationNotFound)?;
    if !matches!(reservation.status, ApparatusScheduleStatus::Planned) {
        return Err(ProductionMapError::ScheduleReservationLocked);
    }
    reservation.status = ApparatusScheduleStatus::Cancelled;
    let reason = input.reason.trim();
    if !reason.is_empty() {
        reservation.reason = format!("{}; cancelled: {reason}", reservation.reason);
    }
    reservation.actor = input.actor;
    Ok(reservation.clone())
}

#[allow(dead_code)]
fn _queue_state_is_active(value: &str) -> bool {
    queue_state::ApparatusQueueOrderState::parse(value)
        .is_some_and(queue_state::ApparatusQueueOrderState::is_active)
}
