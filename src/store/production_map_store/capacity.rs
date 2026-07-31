use rusqlite::{OptionalExtension, params};

use super::ProductionMapStore;
use crate::core::production_map::*;

pub(super) async fn apparatus_capacity_profiles(
    store: &ProductionMapStore,
) -> Result<Vec<ApparatusCapacityProfile>, ProductionMapError> {
    let conn = store
        .conn
        .lock()
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let mut stmt = conn
        .prepare(
            "SELECT apparatus_id, apparatus, capacity_slots, setup_minutes,
                    cleanup_minutes, efficiency_percent, finite_capacity,
                    working_windows_json, capabilities_json,
                    capability_levels_json, notes, updated_at
             FROM apparatus_capacity_profiles
             ORDER BY lower(apparatus)",
        )
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let rows = stmt
        .query_map([], |row| {
            let working_windows = decode::<Vec<ApparatusWorkingWindow>>(row.get(7)?)?;
            let capabilities = decode::<Vec<String>>(row.get(8)?)?;
            let capability_levels = decode::<std::collections::BTreeMap<String, u16>>(row.get(9)?)?;
            Ok(ApparatusCapacityProfile {
                apparatus_id: row.get(0)?,
                apparatus: row.get(1)?,
                capacity_slots: row.get::<_, i64>(2)?.clamp(1, 64) as u16,
                setup_minutes: row.get::<_, i64>(3)?.max(0) as u32,
                cleanup_minutes: row.get::<_, i64>(4)?.max(0) as u32,
                efficiency_percent: row.get::<_, i64>(5)?.clamp(1, 200) as u16,
                finite_capacity: row.get::<_, i64>(6)? != 0,
                working_windows,
                capabilities,
                capability_levels,
                notes: row.get(10)?,
                updated_at_unix: row.get(11)?,
            })
        })
        .map_err(|_| ProductionMapError::StoreFailed)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|_| ProductionMapError::StoreFailed)
}

pub(super) async fn put_apparatus_capacity_profile(
    store: &ProductionMapStore,
    profile: ApparatusCapacityProfile,
) -> Result<(), ProductionMapError> {
    let conn = store
        .conn
        .lock()
        .map_err(|_| ProductionMapError::StoreFailed)?;
    conn.execute(
        "INSERT INTO apparatus_capacity_profiles (
            apparatus_id, apparatus, capacity_slots, setup_minutes, cleanup_minutes,
            efficiency_percent, finite_capacity, working_windows_json,
            capabilities_json, capability_levels_json, notes, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT(apparatus_id) DO UPDATE SET
            apparatus = excluded.apparatus,
            capacity_slots = excluded.capacity_slots,
            setup_minutes = excluded.setup_minutes,
            cleanup_minutes = excluded.cleanup_minutes,
            efficiency_percent = excluded.efficiency_percent,
            finite_capacity = excluded.finite_capacity,
            working_windows_json = excluded.working_windows_json,
            capabilities_json = excluded.capabilities_json,
            capability_levels_json = excluded.capability_levels_json,
            notes = excluded.notes,
            updated_at = excluded.updated_at",
        params![
            profile.apparatus_id,
            profile.apparatus,
            i64::from(profile.capacity_slots),
            i64::from(profile.setup_minutes),
            i64::from(profile.cleanup_minutes),
            i64::from(profile.efficiency_percent),
            i64::from(profile.finite_capacity as u8),
            encode(&profile.working_windows)?,
            encode(&profile.capabilities)?,
            encode(&profile.capability_levels)?,
            profile.notes,
            profile.updated_at_unix,
        ],
    )
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

pub(super) async fn apparatus_downtimes(
    store: &ProductionMapStore,
) -> Result<Vec<ApparatusDowntime>, ProductionMapError> {
    let conn = store
        .conn
        .lock()
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, apparatus_id, apparatus, starts_at_unix, ends_at_unix,
                    reason, active, actor_json, created_at_unix
             FROM apparatus_downtimes
             ORDER BY starts_at_unix DESC",
        )
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(ApparatusDowntime {
                id: row.get(0)?,
                apparatus_id: row.get(1)?,
                apparatus: row.get(2)?,
                starts_at_unix: row.get(3)?,
                ends_at_unix: row.get(4)?,
                reason: row.get(5)?,
                active: row.get::<_, i64>(6)? != 0,
                actor: decode(row.get(7)?)?,
                created_at_unix: row.get(8)?,
            })
        })
        .map_err(|_| ProductionMapError::StoreFailed)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|_| ProductionMapError::StoreFailed)
}

pub(super) async fn put_apparatus_downtime(
    store: &ProductionMapStore,
    downtime: ApparatusDowntime,
) -> Result<(), ProductionMapError> {
    let conn = store
        .conn
        .lock()
        .map_err(|_| ProductionMapError::StoreFailed)?;
    conn.execute(
        "INSERT INTO apparatus_downtimes (
            id, apparatus_id, apparatus, starts_at_unix, ends_at_unix,
            reason, active, actor_json, created_at_unix
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(id) DO UPDATE SET
            apparatus_id = excluded.apparatus_id,
            apparatus = excluded.apparatus,
            starts_at_unix = excluded.starts_at_unix,
            ends_at_unix = excluded.ends_at_unix,
            reason = excluded.reason,
            active = excluded.active,
            actor_json = excluded.actor_json",
        params![
            downtime.id,
            downtime.apparatus_id,
            downtime.apparatus,
            downtime.starts_at_unix,
            downtime.ends_at_unix,
            downtime.reason,
            i64::from(downtime.active as u8),
            encode(&downtime.actor)?,
            downtime.created_at_unix,
        ],
    )
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

pub(super) async fn apparatus_schedule_reservations(
    store: &ProductionMapStore,
) -> Result<Vec<ApparatusScheduleReservation>, ProductionMapError> {
    let conn = store
        .conn
        .lock()
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let mut stmt = conn
        .prepare(
            "SELECT reservation_id, idempotency_key, order_id, apparatus_id, apparatus,
                    starts_at_unix, ends_at_unix, requested_duration_minutes,
                    reserved_duration_minutes, status, priority, source, reason,
                    capability_requirements_json, actor_json, created_at_unix
             FROM apparatus_schedule_reservations
             ORDER BY starts_at_unix, priority DESC, reservation_id",
        )
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(ApparatusScheduleReservation {
                reservation_id: row.get(0)?,
                idempotency_key: row.get(1)?,
                order_id: row.get(2)?,
                apparatus_id: row.get(3)?,
                apparatus: row.get(4)?,
                starts_at_unix: row.get(5)?,
                ends_at_unix: row.get(6)?,
                requested_duration_minutes: row.get::<_, i64>(7)?.max(0) as u32,
                reserved_duration_minutes: row.get::<_, i64>(8)?.max(0) as u32,
                status: ApparatusScheduleStatus::parse(&row.get::<_, String>(9)?)
                    .unwrap_or(ApparatusScheduleStatus::Planned),
                priority: row.get(10)?,
                source: row.get(11)?,
                reason: row.get(12)?,
                capability_requirements: decode(row.get(13)?)?,
                actor: decode(row.get(14)?)?,
                created_at_unix: row.get(15)?,
            })
        })
        .map_err(|_| ProductionMapError::StoreFailed)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|_| ProductionMapError::StoreFailed)
}

pub(super) async fn apparatus_schedule_reservation_by_idempotency_key(
    store: &ProductionMapStore,
    idempotency_key: &str,
) -> Result<Option<ApparatusScheduleReservation>, ProductionMapError> {
    let conn = store
        .conn
        .lock()
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let row = conn
        .query_row(
            "SELECT reservation_id, idempotency_key, order_id, apparatus_id, apparatus,
                    starts_at_unix, ends_at_unix, requested_duration_minutes,
                    reserved_duration_minutes, status, priority, source, reason,
                    capability_requirements_json, actor_json, created_at_unix
             FROM apparatus_schedule_reservations
             WHERE idempotency_key = ?1",
            params![idempotency_key.trim()],
            reservation_from_row,
        )
        .optional()
        .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(row)
}

pub(super) async fn put_apparatus_schedule_reservation(
    store: &ProductionMapStore,
    reservation: ApparatusScheduleReservation,
    capacity_slots: u16,
    finite_capacity: bool,
) -> Result<ApparatusScheduleReservation, ProductionMapError> {
    let conn = store
        .conn
        .lock()
        .map_err(|_| ProductionMapError::StoreFailed)?;
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let result = (|| {
        let existing = conn
            .query_row(
                "SELECT reservation_id, idempotency_key, order_id, apparatus_id, apparatus,
                        starts_at_unix, ends_at_unix, requested_duration_minutes,
                        reserved_duration_minutes, status, priority, source, reason,
                        capability_requirements_json, actor_json, created_at_unix
                 FROM apparatus_schedule_reservations
                 WHERE idempotency_key = ?1",
                params![reservation.idempotency_key.trim()],
                reservation_from_row,
            )
            .optional()
            .map_err(|_| ProductionMapError::StoreFailed)?;
        if let Some(existing) = existing {
            if existing.order_id != reservation.order_id
                || existing.apparatus_id != reservation.apparatus_id
            {
                return Err(ProductionMapError::ScheduleIdempotencyConflict);
            }
            return Ok(existing);
        }
        let conflicts: i64 = conn
            .query_row(
                "SELECT COUNT(*)
                 FROM apparatus_schedule_reservations
                 WHERE apparatus_id = ?1
                   AND status IN ('planned', 'active')
                   AND starts_at_unix < ?2
                   AND ?3 < ends_at_unix",
                params![
                    reservation.apparatus_id.trim(),
                    reservation.ends_at_unix,
                    reservation.starts_at_unix
                ],
                |row| row.get(0),
            )
            .map_err(|_| ProductionMapError::StoreFailed)?;
        if finite_capacity && (capacity_slots == 0 || conflicts >= i64::from(capacity_slots)) {
            return Err(ProductionMapError::CapacityConflict);
        }
        conn.execute(
            "INSERT INTO apparatus_schedule_reservations (
                reservation_id, idempotency_key, order_id, apparatus_id, apparatus,
                starts_at_unix, ends_at_unix, requested_duration_minutes,
                reserved_duration_minutes, status, priority, source, reason,
                capability_requirements_json, actor_json, created_at_unix
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                reservation.reservation_id,
                reservation.idempotency_key,
                reservation.order_id,
                reservation.apparatus_id,
                reservation.apparatus,
                reservation.starts_at_unix,
                reservation.ends_at_unix,
                i64::from(reservation.requested_duration_minutes),
                i64::from(reservation.reserved_duration_minutes),
                reservation.status.as_str(),
                reservation.priority,
                reservation.source,
                reservation.reason,
                encode(&reservation.capability_requirements)?,
                encode(&reservation.actor)?,
                reservation.created_at_unix,
            ],
        )
        .map_err(|_| ProductionMapError::StoreFailed)?;
        Ok(reservation)
    })();
    if result.is_ok() {
        conn.execute("COMMIT", [])
            .map_err(|_| ProductionMapError::StoreFailed)?;
    } else {
        let _ = conn.execute("ROLLBACK", []);
    }
    result
}

pub(super) async fn cancel_apparatus_schedule_reservation(
    store: &ProductionMapStore,
    input: ApparatusScheduleCancelRequest,
) -> Result<ApparatusScheduleReservation, ProductionMapError> {
    let conn = store
        .conn
        .lock()
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let mut reservation = conn
        .query_row(
            "SELECT reservation_id, idempotency_key, order_id, apparatus_id, apparatus,
                    starts_at_unix, ends_at_unix, requested_duration_minutes,
                    reserved_duration_minutes, status, priority, source, reason,
                    capability_requirements_json, actor_json, created_at_unix
             FROM apparatus_schedule_reservations
             WHERE reservation_id = ?1",
            params![input.reservation_id.trim()],
            reservation_from_row,
        )
        .optional()
        .map_err(|_| ProductionMapError::StoreFailed)?
        .ok_or(ProductionMapError::ScheduleReservationNotFound)?;
    if !matches!(reservation.status, ApparatusScheduleStatus::Planned) {
        return Err(ProductionMapError::ScheduleReservationLocked);
    }
    reservation.status = ApparatusScheduleStatus::Cancelled;
    if !input.reason.trim().is_empty() {
        reservation.reason = format!("{}; cancelled: {}", reservation.reason, input.reason.trim());
    }
    reservation.actor = input.actor;
    conn.execute(
        "UPDATE apparatus_schedule_reservations
         SET status = ?1, reason = ?2, actor_json = ?3
         WHERE reservation_id = ?4",
        params![
            reservation.status.as_str(),
            reservation.reason,
            encode(&reservation.actor)?,
            reservation.reservation_id,
        ],
    )
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(reservation)
}

fn reservation_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ApparatusScheduleReservation> {
    Ok(ApparatusScheduleReservation {
        reservation_id: row.get(0)?,
        idempotency_key: row.get(1)?,
        order_id: row.get(2)?,
        apparatus_id: row.get(3)?,
        apparatus: row.get(4)?,
        starts_at_unix: row.get(5)?,
        ends_at_unix: row.get(6)?,
        requested_duration_minutes: row.get::<_, i64>(7)?.max(0) as u32,
        reserved_duration_minutes: row.get::<_, i64>(8)?.max(0) as u32,
        status: ApparatusScheduleStatus::parse(&row.get::<_, String>(9)?)
            .unwrap_or(ApparatusScheduleStatus::Planned),
        priority: row.get(10)?,
        source: row.get(11)?,
        reason: row.get(12)?,
        capability_requirements: decode(row.get(13)?)?,
        actor: decode(row.get(14)?)?,
        created_at_unix: row.get(15)?,
    })
}

fn encode<T: serde::Serialize>(value: &T) -> Result<String, ProductionMapError> {
    serde_json::to_string(value).map_err(|_| ProductionMapError::StoreFailed)
}

fn decode<T: serde::de::DeserializeOwned>(value: String) -> rusqlite::Result<T> {
    serde_json::from_str(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}
