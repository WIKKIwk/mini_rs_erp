use sqlx::{PgPool, Row};

use crate::core::production_map::*;

pub(super) async fn resolve_apparatus_identity(
    pool: &PgPool,
    apparatus_id: &str,
    apparatus: &str,
) -> Result<Option<ApparatusScheduleCandidate>, ProductionMapError> {
    let row = sqlx::query_as::<_, (String, String)>(
        "SELECT id, name
         FROM mini_apparatus
         WHERE ($1 <> '' OR $2 <> '')
           AND ($1 = '' OR lower(id) = lower($1))
           AND ($2 = '' OR lower(name) = lower($2))
         LIMIT 1",
    )
    .bind(apparatus_id.trim())
    .bind(apparatus.trim())
    .fetch_optional(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(row.map(|(apparatus_id, apparatus)| ApparatusScheduleCandidate {
        apparatus_id,
        apparatus,
    }))
}

pub(super) async fn load_apparatus_capacity_profiles(
    pool: &PgPool,
) -> Result<Vec<ApparatusCapacityProfile>, ProductionMapError> {
    let rows = sqlx::query(
        "SELECT apparatus_id, apparatus, capacity_slots, setup_minutes, cleanup_minutes,
                efficiency_percent, finite_capacity, working_windows, capabilities,
                capability_levels, notes, updated_at
         FROM (
             SELECT DISTINCT ON (
                        lower(COALESCE(
                            canonical_by_id.id,
                            canonical_by_name.id,
                            profile.apparatus_id
                        ))
                    )
                    COALESCE(canonical_by_id.id, canonical_by_name.id, profile.apparatus_id)
                        AS apparatus_id,
                    COALESCE(canonical_by_id.name, canonical_by_name.name, profile.apparatus)
                        AS apparatus,
                    profile.capacity_slots, profile.setup_minutes, profile.cleanup_minutes,
                    profile.efficiency_percent, profile.finite_capacity, profile.working_windows,
                    profile.capabilities, profile.capability_levels, profile.notes,
                    EXTRACT(EPOCH FROM profile.updated_at)::BIGINT AS updated_at
             FROM mini_apparatus_capacity_profiles profile
             LEFT JOIN mini_apparatus canonical_by_id
               ON lower(canonical_by_id.id) = lower(profile.apparatus_id)
             LEFT JOIN mini_apparatus canonical_by_name
               ON canonical_by_id.id IS NULL
              AND lower(canonical_by_name.name) = lower(profile.apparatus)
             ORDER BY
                 lower(COALESCE(
                     canonical_by_id.id,
                     canonical_by_name.id,
                     profile.apparatus_id
                 )),
                 profile.updated_at DESC,
                 profile.apparatus_id
         ) canonical_profiles
         ORDER BY lower(apparatus)",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    rows.into_iter().map(profile_from_row).collect()
}

pub(super) async fn put_apparatus_capacity_profile(
    pool: &PgPool,
    profile: ApparatusCapacityProfile,
) -> Result<(), ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    sqlx::query(
        "DELETE FROM mini_apparatus_capacity_profiles legacy
         WHERE lower(legacy.apparatus_id) <> lower($1)
           AND lower(legacy.apparatus) = lower($2)
           AND NOT EXISTS (
               SELECT 1 FROM mini_apparatus catalog
               WHERE lower(catalog.id) = lower(legacy.apparatus_id)
           )",
    )
    .bind(profile.apparatus_id.trim())
    .bind(profile.apparatus.trim())
    .execute(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    sqlx::query(
        "INSERT INTO mini_apparatus_capacity_profiles (
            apparatus_id, apparatus, capacity_slots, setup_minutes, cleanup_minutes,
            efficiency_percent, finite_capacity, working_windows, capabilities,
            capability_levels, notes, updated_at
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, to_timestamp($12))
         ON CONFLICT (apparatus_id) DO UPDATE SET
            apparatus = EXCLUDED.apparatus,
            capacity_slots = EXCLUDED.capacity_slots,
            setup_minutes = EXCLUDED.setup_minutes,
            cleanup_minutes = EXCLUDED.cleanup_minutes,
            efficiency_percent = EXCLUDED.efficiency_percent,
            finite_capacity = EXCLUDED.finite_capacity,
            working_windows = EXCLUDED.working_windows,
            capabilities = EXCLUDED.capabilities,
            capability_levels = EXCLUDED.capability_levels,
            notes = EXCLUDED.notes,
            updated_at = EXCLUDED.updated_at",
    )
    .bind(profile.apparatus_id.trim())
    .bind(profile.apparatus.trim())
    .bind(i32::from(profile.capacity_slots))
    .bind(i32::try_from(profile.setup_minutes).map_err(|_| ProductionMapError::CapacityProfileInvalid)?)
    .bind(i32::try_from(profile.cleanup_minutes).map_err(|_| ProductionMapError::CapacityProfileInvalid)?)
    .bind(i32::from(profile.efficiency_percent))
    .bind(profile.finite_capacity)
    .bind(serde_json::to_value(&profile.working_windows).map_err(|_| ProductionMapError::StoreFailed)?)
    .bind(serde_json::to_value(&profile.capabilities).map_err(|_| ProductionMapError::StoreFailed)?)
    .bind(serde_json::to_value(&profile.capability_levels).map_err(|_| ProductionMapError::StoreFailed)?)
    .bind(profile.notes.trim())
    .bind(profile.updated_at_unix as f64)
    .execute(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)
}

pub(super) async fn load_apparatus_downtimes(
    pool: &PgPool,
) -> Result<Vec<ApparatusDowntime>, ProductionMapError> {
    let rows = sqlx::query(
        "SELECT downtime.id,
                COALESCE(canonical_by_id.id, canonical_by_name.id, downtime.apparatus_id)
                    AS apparatus_id,
                COALESCE(canonical_by_id.name, canonical_by_name.name, downtime.apparatus)
                    AS apparatus,
                EXTRACT(EPOCH FROM downtime.starts_at)::BIGINT AS starts_at,
                EXTRACT(EPOCH FROM downtime.ends_at)::BIGINT AS ends_at,
                downtime.reason, downtime.active, downtime.actor_json,
                EXTRACT(EPOCH FROM downtime.created_at)::BIGINT AS created_at
         FROM mini_apparatus_downtimes downtime
         LEFT JOIN mini_apparatus canonical_by_id
           ON lower(canonical_by_id.id) = lower(downtime.apparatus_id)
         LEFT JOIN mini_apparatus canonical_by_name
           ON canonical_by_id.id IS NULL
          AND lower(canonical_by_name.name) = lower(downtime.apparatus)
         ORDER BY downtime.starts_at DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    rows.into_iter().map(downtime_from_row).collect()
}

pub(super) async fn put_apparatus_downtime(
    pool: &PgPool,
    downtime: ApparatusDowntime,
) -> Result<(), ProductionMapError> {
    sqlx::query(
        "INSERT INTO mini_apparatus_downtimes (
            id, apparatus_id, apparatus, starts_at, ends_at, reason, active,
            actor_json, created_at
         ) VALUES ($1, $2, $3, to_timestamp($4), to_timestamp($5), $6, $7, $8, to_timestamp($9))
         ON CONFLICT (id) DO UPDATE SET
            apparatus_id = EXCLUDED.apparatus_id,
            apparatus = EXCLUDED.apparatus,
            starts_at = EXCLUDED.starts_at,
            ends_at = EXCLUDED.ends_at,
            reason = EXCLUDED.reason,
            active = EXCLUDED.active,
            actor_json = EXCLUDED.actor_json",
    )
    .bind(downtime.id.trim())
    .bind(downtime.apparatus_id.trim())
    .bind(downtime.apparatus.trim())
    .bind(downtime.starts_at_unix as f64)
    .bind(downtime.ends_at_unix as f64)
    .bind(downtime.reason.trim())
    .bind(downtime.active)
    .bind(serde_json::to_value(&downtime.actor).map_err(|_| ProductionMapError::StoreFailed)?)
    .bind(downtime.created_at_unix as f64)
    .execute(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

pub(super) async fn load_apparatus_schedule_reservations(
    pool: &PgPool,
) -> Result<Vec<ApparatusScheduleReservation>, ProductionMapError> {
    let rows = sqlx::query(
        "SELECT reservation.reservation_id, reservation.idempotency_key, reservation.order_id,
                COALESCE(canonical_by_id.id, canonical_by_name.id, reservation.apparatus_id)
                    AS apparatus_id,
                COALESCE(canonical_by_id.name, canonical_by_name.name, reservation.apparatus)
                    AS apparatus,
                EXTRACT(EPOCH FROM reservation.starts_at)::BIGINT AS starts_at,
                EXTRACT(EPOCH FROM reservation.ends_at)::BIGINT AS ends_at,
                reservation.requested_duration_minutes, reservation.reserved_duration_minutes,
                reservation.status, reservation.priority, reservation.source, reservation.reason,
                reservation.capability_requirements, reservation.actor_json,
                EXTRACT(EPOCH FROM reservation.created_at)::BIGINT AS created_at
         FROM mini_apparatus_schedule_reservations reservation
         LEFT JOIN mini_apparatus canonical_by_id
           ON lower(canonical_by_id.id) = lower(reservation.apparatus_id)
         LEFT JOIN mini_apparatus canonical_by_name
           ON canonical_by_id.id IS NULL
          AND lower(canonical_by_name.name) = lower(reservation.apparatus)
         ORDER BY reservation.starts_at, reservation.priority DESC, reservation.reservation_id",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    rows.into_iter().map(reservation_from_row).collect()
}

pub(super) async fn load_apparatus_schedule_reservation_by_idempotency_key(
    pool: &PgPool,
    idempotency_key: &str,
) -> Result<Option<ApparatusScheduleReservation>, ProductionMapError> {
    let row = sqlx::query(
        "SELECT reservation.reservation_id, reservation.idempotency_key, reservation.order_id,
                COALESCE(canonical_by_id.id, canonical_by_name.id, reservation.apparatus_id)
                    AS apparatus_id,
                COALESCE(canonical_by_id.name, canonical_by_name.name, reservation.apparatus)
                    AS apparatus,
                EXTRACT(EPOCH FROM reservation.starts_at)::BIGINT AS starts_at,
                EXTRACT(EPOCH FROM reservation.ends_at)::BIGINT AS ends_at,
                reservation.requested_duration_minutes, reservation.reserved_duration_minutes,
                reservation.status, reservation.priority, reservation.source, reservation.reason,
                reservation.capability_requirements, reservation.actor_json,
                EXTRACT(EPOCH FROM reservation.created_at)::BIGINT AS created_at
         FROM mini_apparatus_schedule_reservations reservation
         LEFT JOIN mini_apparatus canonical_by_id
           ON lower(canonical_by_id.id) = lower(reservation.apparatus_id)
         LEFT JOIN mini_apparatus canonical_by_name
           ON canonical_by_id.id IS NULL
          AND lower(canonical_by_name.name) = lower(reservation.apparatus)
         WHERE reservation.idempotency_key = $1",
    )
    .bind(idempotency_key.trim())
    .fetch_optional(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    row.map(reservation_from_row).transpose()
}

pub(super) async fn put_apparatus_schedule_reservation(
    pool: &PgPool,
    reservation: ApparatusScheduleReservation,
    capacity_slots: u16,
    finite_capacity: bool,
) -> Result<ApparatusScheduleReservation, ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(reservation.apparatus_id.trim())
        .execute(&mut *tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let existing = sqlx::query(
        "SELECT reservation_id, idempotency_key, order_id, apparatus_id, apparatus,
                EXTRACT(EPOCH FROM starts_at)::BIGINT AS starts_at,
                EXTRACT(EPOCH FROM ends_at)::BIGINT AS ends_at,
                requested_duration_minutes, reserved_duration_minutes, status, priority,
                source, reason, capability_requirements, actor_json,
                EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at
         FROM mini_apparatus_schedule_reservations
         WHERE idempotency_key = $1
         FOR UPDATE",
    )
    .bind(reservation.idempotency_key.trim())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if let Some(existing) = existing {
        let existing = reservation_from_row(existing)?;
        if existing.order_id != reservation.order_id
            || existing.apparatus_id != reservation.apparatus_id
        {
            return Err(ProductionMapError::ScheduleIdempotencyConflict);
        }
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        return Ok(existing);
    }
    let conflicts = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)
         FROM mini_apparatus_schedule_reservations
         WHERE apparatus_id = $1
           AND status IN ('planned', 'active')
           AND starts_at < to_timestamp($2)
           AND to_timestamp($3) < ends_at",
    )
    .bind(reservation.apparatus_id.trim())
    .bind(reservation.ends_at_unix as f64)
    .bind(reservation.starts_at_unix as f64)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if finite_capacity && (capacity_slots == 0 || conflicts >= i64::from(capacity_slots)) {
        return Err(ProductionMapError::CapacityConflict);
    }
    sqlx::query(
        "INSERT INTO mini_apparatus_schedule_reservations (
            reservation_id, idempotency_key, order_id, apparatus_id, apparatus,
            starts_at, ends_at, requested_duration_minutes, reserved_duration_minutes,
            status, priority, source, reason, capability_requirements, actor_json, created_at
         ) VALUES ($1, $2, $3, $4, $5, to_timestamp($6), to_timestamp($7), $8, $9,
                   $10, $11, $12, $13, $14, $15, to_timestamp($16))",
    )
    .bind(reservation.reservation_id.trim())
    .bind(reservation.idempotency_key.trim())
    .bind(reservation.order_id.trim())
    .bind(reservation.apparatus_id.trim())
    .bind(reservation.apparatus.trim())
    .bind(reservation.starts_at_unix as f64)
    .bind(reservation.ends_at_unix as f64)
    .bind(i32::try_from(reservation.requested_duration_minutes).map_err(|_| ProductionMapError::ScheduleInputInvalid)?)
    .bind(i32::try_from(reservation.reserved_duration_minutes).map_err(|_| ProductionMapError::ScheduleInputInvalid)?)
    .bind(reservation.status.as_str())
    .bind(reservation.priority)
    .bind(reservation.source.trim())
    .bind(reservation.reason.trim())
    .bind(serde_json::to_value(&reservation.capability_requirements).map_err(|_| ProductionMapError::StoreFailed)?)
    .bind(serde_json::to_value(&reservation.actor).map_err(|_| ProductionMapError::StoreFailed)?)
    .bind(reservation.created_at_unix as f64)
    .execute(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(reservation)
}

pub(super) async fn cancel_apparatus_schedule_reservation(
    pool: &PgPool,
    input: ApparatusScheduleCancelRequest,
) -> Result<ApparatusScheduleReservation, ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let row = sqlx::query(
        "SELECT reservation_id, idempotency_key, order_id, apparatus_id, apparatus,
                EXTRACT(EPOCH FROM starts_at)::BIGINT AS starts_at,
                EXTRACT(EPOCH FROM ends_at)::BIGINT AS ends_at,
                requested_duration_minutes, reserved_duration_minutes, status, priority,
                source, reason, capability_requirements, actor_json,
                EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at
         FROM mini_apparatus_schedule_reservations
         WHERE reservation_id = $1
         FOR UPDATE",
    )
    .bind(input.reservation_id.trim())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    .ok_or(ProductionMapError::ScheduleReservationNotFound)?;
    let mut reservation = reservation_from_row(row)?;
    if !matches!(reservation.status, ApparatusScheduleStatus::Planned) {
        return Err(ProductionMapError::ScheduleReservationLocked);
    }
    reservation.status = ApparatusScheduleStatus::Cancelled;
    if !input.reason.trim().is_empty() {
        reservation.reason = format!("{}; cancelled: {}", reservation.reason, input.reason.trim());
    }
    reservation.actor = input.actor;
    sqlx::query(
        "UPDATE mini_apparatus_schedule_reservations
         SET status = $1, reason = $2, actor_json = $3
         WHERE reservation_id = $4",
    )
    .bind(reservation.status.as_str())
    .bind(reservation.reason.trim())
    .bind(serde_json::to_value(&reservation.actor).map_err(|_| ProductionMapError::StoreFailed)?)
    .bind(reservation.reservation_id.trim())
    .execute(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(reservation)
}

pub(super) async fn update_apparatus_schedule_reservation_status_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    order_id: &str,
    apparatus: &str,
    status: ApparatusScheduleStatus,
    actor: &QueueActionActor,
) -> Result<(), ProductionMapError> {
    sqlx::query(
        "UPDATE mini_apparatus_schedule_reservations AS reservation
         SET status = $1, actor_json = $2
         WHERE reservation.order_id = $3
           AND (
                EXISTS (
                    SELECT 1
                    FROM mini_apparatus target
                    WHERE lower(target.name) = lower($4)
                      AND (
                           lower(reservation.apparatus_id) = lower(target.id)
                           OR (
                               NOT EXISTS (
                                   SELECT 1 FROM mini_apparatus stored
                                   WHERE lower(stored.id) = lower(reservation.apparatus_id)
                               )
                               AND lower(reservation.apparatus) = lower(target.name)
                           )
                      )
                )
                OR (
                    NOT EXISTS (
                        SELECT 1 FROM mini_apparatus target
                        WHERE lower(target.name) = lower($4)
                    )
                    AND (
                        lower(reservation.apparatus) = lower($4)
                        OR lower(reservation.apparatus_id) = lower($4)
                    )
                )
           )
           AND (
                reservation.status = $1
                OR ($1 = 'active' AND reservation.status IN ('planned', 'paused'))
                OR ($1 = 'paused' AND reservation.status = 'active')
                OR ($1 = 'completed' AND reservation.status IN ('planned', 'active', 'paused'))
           )",
    )
    .bind(status.as_str())
    .bind(serde_json::to_value(actor).map_err(|_| ProductionMapError::StoreFailed)?)
    .bind(order_id.trim())
    .bind(apparatus.trim())
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

fn profile_from_row(row: sqlx::postgres::PgRow) -> Result<ApparatusCapacityProfile, ProductionMapError> {
    Ok(ApparatusCapacityProfile {
        apparatus_id: row.try_get("apparatus_id").map_err(|_| ProductionMapError::StoreFailed)?,
        apparatus: row.try_get("apparatus").map_err(|_| ProductionMapError::StoreFailed)?,
        capacity_slots: row.try_get::<i32, _>("capacity_slots").map_err(|_| ProductionMapError::StoreFailed)?.clamp(1, 64) as u16,
        setup_minutes: row.try_get::<i32, _>("setup_minutes").map_err(|_| ProductionMapError::StoreFailed)?.max(0) as u32,
        cleanup_minutes: row.try_get::<i32, _>("cleanup_minutes").map_err(|_| ProductionMapError::StoreFailed)?.max(0) as u32,
        efficiency_percent: row.try_get::<i32, _>("efficiency_percent").map_err(|_| ProductionMapError::StoreFailed)?.clamp(1, 200) as u16,
        finite_capacity: row.try_get("finite_capacity").map_err(|_| ProductionMapError::StoreFailed)?,
        working_windows: json_field(&row, "working_windows")?,
        capabilities: json_field(&row, "capabilities")?,
        capability_levels: json_field(&row, "capability_levels")?,
        notes: row.try_get("notes").map_err(|_| ProductionMapError::StoreFailed)?,
        updated_at_unix: row.try_get("updated_at").map_err(|_| ProductionMapError::StoreFailed)?,
    })
}

fn downtime_from_row(row: sqlx::postgres::PgRow) -> Result<ApparatusDowntime, ProductionMapError> {
    Ok(ApparatusDowntime {
        id: row.try_get("id").map_err(|_| ProductionMapError::StoreFailed)?,
        apparatus_id: row.try_get("apparatus_id").map_err(|_| ProductionMapError::StoreFailed)?,
        apparatus: row.try_get("apparatus").map_err(|_| ProductionMapError::StoreFailed)?,
        starts_at_unix: row.try_get("starts_at").map_err(|_| ProductionMapError::StoreFailed)?,
        ends_at_unix: row.try_get("ends_at").map_err(|_| ProductionMapError::StoreFailed)?,
        reason: row.try_get("reason").map_err(|_| ProductionMapError::StoreFailed)?,
        active: row.try_get("active").map_err(|_| ProductionMapError::StoreFailed)?,
        actor: json_field(&row, "actor_json")?,
        created_at_unix: row.try_get("created_at").map_err(|_| ProductionMapError::StoreFailed)?,
    })
}

fn reservation_from_row(row: sqlx::postgres::PgRow) -> Result<ApparatusScheduleReservation, ProductionMapError> {
    Ok(ApparatusScheduleReservation {
        reservation_id: row.try_get("reservation_id").map_err(|_| ProductionMapError::StoreFailed)?,
        idempotency_key: row.try_get("idempotency_key").map_err(|_| ProductionMapError::StoreFailed)?,
        order_id: row.try_get("order_id").map_err(|_| ProductionMapError::StoreFailed)?,
        apparatus_id: row.try_get("apparatus_id").map_err(|_| ProductionMapError::StoreFailed)?,
        apparatus: row.try_get("apparatus").map_err(|_| ProductionMapError::StoreFailed)?,
        starts_at_unix: row.try_get("starts_at").map_err(|_| ProductionMapError::StoreFailed)?,
        ends_at_unix: row.try_get("ends_at").map_err(|_| ProductionMapError::StoreFailed)?,
        requested_duration_minutes: row.try_get::<i32, _>("requested_duration_minutes").map_err(|_| ProductionMapError::StoreFailed)?.max(0) as u32,
        reserved_duration_minutes: row.try_get::<i32, _>("reserved_duration_minutes").map_err(|_| ProductionMapError::StoreFailed)?.max(0) as u32,
        status: ApparatusScheduleStatus::parse(&row.try_get::<String, _>("status").map_err(|_| ProductionMapError::StoreFailed)?)
            .unwrap_or(ApparatusScheduleStatus::Planned),
        priority: row.try_get("priority").map_err(|_| ProductionMapError::StoreFailed)?,
        source: row.try_get("source").map_err(|_| ProductionMapError::StoreFailed)?,
        reason: row.try_get("reason").map_err(|_| ProductionMapError::StoreFailed)?,
        capability_requirements: json_field(&row, "capability_requirements")?,
        actor: json_field(&row, "actor_json")?,
        created_at_unix: row.try_get("created_at").map_err(|_| ProductionMapError::StoreFailed)?,
    })
}

fn json_field<T: serde::de::DeserializeOwned>(
    row: &sqlx::postgres::PgRow,
    column: &str,
) -> Result<T, ProductionMapError> {
    let value: serde_json::Value = row
        .try_get(column)
        .map_err(|_| ProductionMapError::StoreFailed)?;
    serde_json::from_value(value).map_err(|_| ProductionMapError::StoreFailed)
}
