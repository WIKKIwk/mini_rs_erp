use sqlx::{PgPool, Row};

use crate::core::apparatus_standard::ApparatusId;
use crate::core::production_map::*;

use super::transaction_locks::{
    lock_apparatus_tx, lock_schedule_idempotency_tx, lock_schedule_reservation_tx,
};

pub(super) async fn load_apparatus_downtimes(
    pool: &PgPool,
) -> Result<Vec<ApparatusDowntime>, ProductionMapError> {
    let rows = sqlx::query(
        "SELECT downtime.id,
                canonical.id AS apparatus_id,
                downtime.apparatus AS apparatus,
                EXTRACT(EPOCH FROM downtime.starts_at)::BIGINT AS starts_at,
                EXTRACT(EPOCH FROM downtime.ends_at)::BIGINT AS ends_at,
                downtime.reason, downtime.active, downtime.actor_json,
                EXTRACT(EPOCH FROM downtime.created_at)::BIGINT AS created_at
         FROM mini_apparatus_downtimes downtime
         INNER JOIN mini_apparatus canonical
           ON canonical.id = downtime.canonical_apparatus_id
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
            id, canonical_apparatus_id, apparatus_id, apparatus, starts_at, ends_at, reason, active,
            actor_json, created_at
         ) VALUES ($1, $2, $2, $3, to_timestamp($4), to_timestamp($5), $6, $7, $8, to_timestamp($9))
         ON CONFLICT (id) DO UPDATE SET
            canonical_apparatus_id = EXCLUDED.canonical_apparatus_id,
            apparatus_id = EXCLUDED.apparatus_id,
            apparatus = EXCLUDED.apparatus,
            starts_at = EXCLUDED.starts_at,
            ends_at = EXCLUDED.ends_at,
            reason = EXCLUDED.reason,
            active = EXCLUDED.active,
            actor_json = EXCLUDED.actor_json",
    )
    .bind(downtime.id.trim())
    .bind(downtime.apparatus_id.as_str())
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
                canonical.id AS apparatus_id,
                reservation.apparatus AS apparatus,
                EXTRACT(EPOCH FROM reservation.starts_at)::BIGINT AS starts_at,
                EXTRACT(EPOCH FROM reservation.ends_at)::BIGINT AS ends_at,
                reservation.requested_duration_minutes, reservation.reserved_duration_minutes,
                reservation.status, reservation.priority, reservation.source, reservation.reason,
                reservation.capability_requirements, reservation.actor_json,
                EXTRACT(EPOCH FROM reservation.created_at)::BIGINT AS created_at
         FROM mini_apparatus_schedule_reservations reservation
         INNER JOIN mini_apparatus canonical
           ON canonical.id = reservation.canonical_apparatus_id
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
                canonical.id AS apparatus_id,
                reservation.apparatus AS apparatus,
                EXTRACT(EPOCH FROM reservation.starts_at)::BIGINT AS starts_at,
                EXTRACT(EPOCH FROM reservation.ends_at)::BIGINT AS ends_at,
                reservation.requested_duration_minutes, reservation.reserved_duration_minutes,
                reservation.status, reservation.priority, reservation.source, reservation.reason,
                reservation.capability_requirements, reservation.actor_json,
                EXTRACT(EPOCH FROM reservation.created_at)::BIGINT AS created_at
         FROM mini_apparatus_schedule_reservations reservation
         INNER JOIN mini_apparatus canonical
           ON canonical.id = reservation.canonical_apparatus_id
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
    lock_schedule_idempotency_tx(&mut tx, &reservation.idempotency_key).await?;
    lock_schedule_reservation_tx(&mut tx, &reservation.reservation_id).await?;
    lock_apparatus_tx(&mut tx, reservation.apparatus_id.as_str()).await?;
    let existing = sqlx::query(
        "SELECT reservation.canonical_apparatus_id AS apparatus_id,
                reservation_id, idempotency_key, order_id, apparatus,
                EXTRACT(EPOCH FROM starts_at)::BIGINT AS starts_at,
                EXTRACT(EPOCH FROM ends_at)::BIGINT AS ends_at,
                requested_duration_minutes, reserved_duration_minutes, status, priority,
                source, reason, capability_requirements, actor_json,
                EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at
         FROM mini_apparatus_schedule_reservations reservation
         INNER JOIN mini_apparatus canonical
           ON canonical.id = reservation.canonical_apparatus_id
         WHERE reservation.idempotency_key = $1
         FOR UPDATE OF reservation",
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
         WHERE canonical_apparatus_id = $1
           AND status IN ('planned', 'active')
           AND starts_at < to_timestamp($2)
           AND to_timestamp($3) < ends_at",
    )
    .bind(reservation.apparatus_id.as_str())
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
            reservation_id, idempotency_key, order_id, canonical_apparatus_id,
            apparatus_id, apparatus,
            starts_at, ends_at, requested_duration_minutes, reserved_duration_minutes,
            status, priority, source, reason, capability_requirements, actor_json, created_at
         ) VALUES ($1, $2, $3, $4, $4, $5, to_timestamp($6), to_timestamp($7), $8, $9,
                   $10, $11, $12, $13, $14, $15, $16, to_timestamp($17))",
    )
    .bind(reservation.reservation_id.trim())
    .bind(reservation.idempotency_key.trim())
    .bind(reservation.order_id.trim())
    .bind(reservation.apparatus_id.as_str())
    .bind(reservation.apparatus_id.as_str())
    .bind(reservation.apparatus.trim())
    .bind(reservation.starts_at_unix as f64)
    .bind(reservation.ends_at_unix as f64)
    .bind(
        i32::try_from(reservation.requested_duration_minutes)
            .map_err(|_| ProductionMapError::ScheduleInputInvalid)?,
    )
    .bind(
        i32::try_from(reservation.reserved_duration_minutes)
            .map_err(|_| ProductionMapError::ScheduleInputInvalid)?,
    )
    .bind(reservation.status.as_str())
    .bind(reservation.priority)
    .bind(reservation.source.trim())
    .bind(reservation.reason.trim())
    .bind(
        serde_json::to_value(&reservation.capability_requirements)
            .map_err(|_| ProductionMapError::StoreFailed)?,
    )
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
    lock_schedule_reservation_tx(&mut tx, &input.reservation_id).await?;
    let apparatus_id = sqlx::query_scalar::<_, String>(
        "SELECT canonical_apparatus_id
         FROM mini_apparatus_schedule_reservations
         WHERE reservation_id = $1",
    )
    .bind(input.reservation_id.trim())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    .ok_or(ProductionMapError::ScheduleReservationNotFound)?;
    let apparatus_id = ApparatusId::new(apparatus_id)
        .map_err(|_| ProductionMapError::StoreFailed)?;
    lock_apparatus_tx(&mut tx, apparatus_id.as_str()).await?;
    let row = sqlx::query(
        "SELECT reservation.canonical_apparatus_id AS apparatus_id,
                reservation_id, idempotency_key, order_id, apparatus,
                EXTRACT(EPOCH FROM starts_at)::BIGINT AS starts_at,
                EXTRACT(EPOCH FROM ends_at)::BIGINT AS ends_at,
                requested_duration_minutes, reserved_duration_minutes, status, priority,
                source, reason, capability_requirements, actor_json,
                EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at
         FROM mini_apparatus_schedule_reservations reservation
         INNER JOIN mini_apparatus canonical
           ON canonical.id = reservation.canonical_apparatus_id
         WHERE reservation.reservation_id = $1
         FOR UPDATE OF reservation",
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
    apparatus_id: &ApparatusId,
    status: ApparatusScheduleStatus,
    actor: &QueueActionActor,
) -> Result<(), ProductionMapError> {
    lock_apparatus_tx(tx, apparatus_id.as_str()).await?;
    sqlx::query(
        "UPDATE mini_apparatus_schedule_reservations AS reservation
         SET status = $1, actor_json = $2
         WHERE reservation.order_id = $3
           AND reservation.canonical_apparatus_id = $4
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
    .bind(apparatus_id.as_str())
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

fn downtime_from_row(row: sqlx::postgres::PgRow) -> Result<ApparatusDowntime, ProductionMapError> {
    Ok(ApparatusDowntime {
        id: row
            .try_get("id")
            .map_err(|_| ProductionMapError::StoreFailed)?,
        apparatus_id: canonical_id_from_row(
            row.try_get("apparatus_id")
                .map_err(|_| ProductionMapError::StoreFailed)?,
        )?,
        apparatus: row
            .try_get("apparatus")
            .map_err(|_| ProductionMapError::StoreFailed)?,
        starts_at_unix: row
            .try_get("starts_at")
            .map_err(|_| ProductionMapError::StoreFailed)?,
        ends_at_unix: row
            .try_get("ends_at")
            .map_err(|_| ProductionMapError::StoreFailed)?,
        reason: row
            .try_get("reason")
            .map_err(|_| ProductionMapError::StoreFailed)?,
        active: row
            .try_get("active")
            .map_err(|_| ProductionMapError::StoreFailed)?,
        actor: json_field(&row, "actor_json")?,
        created_at_unix: row
            .try_get("created_at")
            .map_err(|_| ProductionMapError::StoreFailed)?,
    })
}

fn reservation_from_row(
    row: sqlx::postgres::PgRow,
) -> Result<ApparatusScheduleReservation, ProductionMapError> {
    Ok(ApparatusScheduleReservation {
        reservation_id: row
            .try_get("reservation_id")
            .map_err(|_| ProductionMapError::StoreFailed)?,
        idempotency_key: row
            .try_get("idempotency_key")
            .map_err(|_| ProductionMapError::StoreFailed)?,
        order_id: row
            .try_get("order_id")
            .map_err(|_| ProductionMapError::StoreFailed)?,
        apparatus_id: canonical_id_from_row(
            row.try_get("apparatus_id")
                .map_err(|_| ProductionMapError::StoreFailed)?,
        )?,
        apparatus: row
            .try_get("apparatus")
            .map_err(|_| ProductionMapError::StoreFailed)?,
        starts_at_unix: row
            .try_get("starts_at")
            .map_err(|_| ProductionMapError::StoreFailed)?,
        ends_at_unix: row
            .try_get("ends_at")
            .map_err(|_| ProductionMapError::StoreFailed)?,
        requested_duration_minutes: row
            .try_get::<i32, _>("requested_duration_minutes")
            .map_err(|_| ProductionMapError::StoreFailed)?
            .max(0) as u32,
        reserved_duration_minutes: row
            .try_get::<i32, _>("reserved_duration_minutes")
            .map_err(|_| ProductionMapError::StoreFailed)?
            .max(0) as u32,
        status: ApparatusScheduleStatus::parse(
            &row.try_get::<String, _>("status")
                .map_err(|_| ProductionMapError::StoreFailed)?,
        )
        .unwrap_or(ApparatusScheduleStatus::Planned),
        priority: row
            .try_get("priority")
            .map_err(|_| ProductionMapError::StoreFailed)?,
        source: row
            .try_get("source")
            .map_err(|_| ProductionMapError::StoreFailed)?,
        reason: row
            .try_get("reason")
            .map_err(|_| ProductionMapError::StoreFailed)?,
        capability_requirements: json_field(&row, "capability_requirements")?,
        actor: json_field(&row, "actor_json")?,
        created_at_unix: row
            .try_get("created_at")
            .map_err(|_| ProductionMapError::StoreFailed)?,
    })
}

fn canonical_id_from_row(value: String) -> Result<ApparatusId, ProductionMapError> {
    ApparatusId::new(value).map_err(|_| ProductionMapError::StoreFailed)
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
