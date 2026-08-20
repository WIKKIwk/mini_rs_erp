use super::*;
use crate::core::apparatus_standard::ApparatusId;

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
        .insert(profile.apparatus_id.as_str().to_string(), profile);
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
    finite_capacity: bool,
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
                && item.apparatus_id == reservation.apparatus_id
                && item.starts_at_unix < reservation.ends_at_unix
                && reservation.starts_at_unix < item.ends_at_unix
        })
        .count();
    if finite_capacity && (capacity_slots == 0 || conflicts >= usize::from(capacity_slots)) {
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

pub(super) async fn update_apparatus_schedule_reservation_status(
    store: &MemoryProductionMapStore,
    order_id: &str,
    apparatus_id: &ApparatusId,
    status: ApparatusScheduleStatus,
    actor: &QueueActionActor,
) -> Result<(), ProductionMapError> {
    let mut reservations = store.apparatus_schedule_reservations.write().await;
    let Some(reservation) = reservations.values_mut().find(|reservation| {
        reservation.order_id.trim() == order_id.trim() && reservation.apparatus_id == *apparatus_id
    }) else {
        return Ok(());
    };
    if reservation.status == status {
        return Ok(());
    }
    let allowed = match status {
        ApparatusScheduleStatus::Active => {
            matches!(
                reservation.status,
                ApparatusScheduleStatus::Planned | ApparatusScheduleStatus::Paused
            )
        }
        ApparatusScheduleStatus::Paused => reservation.status == ApparatusScheduleStatus::Active,
        ApparatusScheduleStatus::Completed => matches!(
            reservation.status,
            ApparatusScheduleStatus::Planned
                | ApparatusScheduleStatus::Active
                | ApparatusScheduleStatus::Paused
        ),
        ApparatusScheduleStatus::Planned | ApparatusScheduleStatus::Cancelled => false,
    };
    if !allowed {
        return Err(ProductionMapError::ScheduleReservationLocked);
    }
    reservation.status = status;
    reservation.actor = actor.clone();
    Ok(())
}
