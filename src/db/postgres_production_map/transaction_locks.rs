use sqlx::{Postgres, Transaction};

use crate::core::apparatus_standard::ApparatusId;
use crate::core::production_map::ProductionMapError;

/// Serialize production-map mutations across application processes. The
/// transaction-scoped locks are deliberately namespaced so an order lock and
/// an apparatus lock cannot collide with each other or with an unrelated
/// PostgreSQL advisory-lock caller.
pub(super) async fn lock_order_and_apparatuses_tx(
    tx: &mut Transaction<'_, Postgres>,
    order_id: &str,
    apparatuses: &[&str],
) -> Result<(), ProductionMapError> {
    lock_orders_and_apparatuses_tx(tx, &[order_id], apparatuses).await
}

pub(super) async fn lock_orders_and_apparatuses_tx(
    tx: &mut Transaction<'_, Postgres>,
    order_ids: &[&str],
    apparatuses: &[&str],
) -> Result<(), ProductionMapError> {
    let mut order_ids = order_ids
        .iter()
        .map(|order_id| order_id.trim().to_string())
        .collect::<Vec<_>>();
    if order_ids.iter().any(|order_id| order_id.is_empty()) {
        return Err(ProductionMapError::MissingId);
    }
    order_ids.sort_unstable();
    order_ids.dedup();
    for order_id in order_ids {
        lock_order_tx(tx, &order_id).await?;
    }

    let mut canonical_apparatuses = apparatuses
        .iter()
        .map(|apparatus| {
            ApparatusId::new(apparatus.trim().to_string())
                .map(|id| id.to_string())
                .map_err(|_| ProductionMapError::ScheduleInputInvalid)
        })
        .collect::<Result<Vec<_>, _>>()?;
    canonical_apparatuses.sort_unstable();
    canonical_apparatuses.dedup();
    for apparatus in canonical_apparatuses {
        lock_named_tx(tx, "apparatus", &apparatus).await?;
    }
    Ok(())
}

pub(super) async fn lock_apparatus_tx(
    tx: &mut Transaction<'_, Postgres>,
    apparatus: &str,
) -> Result<ApparatusId, ProductionMapError> {
    let apparatus_id = ApparatusId::new(apparatus.trim().to_string())
        .map_err(|_| ProductionMapError::ScheduleInputInvalid)?;
    lock_named_tx(tx, "apparatus", apparatus_id.as_str()).await?;
    Ok(apparatus_id)
}

pub(super) async fn lock_order_tx(
    tx: &mut Transaction<'_, Postgres>,
    order_id: &str,
) -> Result<(), ProductionMapError> {
    let order_id = order_id.trim();
    if order_id.is_empty() {
        return Err(ProductionMapError::MissingId);
    }
    lock_named_tx(tx, "order", order_id).await
}

pub(super) async fn lock_transfer_idempotency_tx(
    tx: &mut Transaction<'_, Postgres>,
    idempotency_key: &str,
) -> Result<(), ProductionMapError> {
    let idempotency_key = idempotency_key.trim();
    if idempotency_key.is_empty() {
        return Err(ProductionMapError::ApparatusTransferIdempotencyRequired);
    }
    lock_named_tx(tx, "apparatus-transfer-idempotency", idempotency_key).await
}

pub(super) async fn lock_schedule_reservation_tx(
    tx: &mut Transaction<'_, Postgres>,
    reservation_id: &str,
) -> Result<(), ProductionMapError> {
    let reservation_id = reservation_id.trim();
    if reservation_id.is_empty() {
        return Err(ProductionMapError::MissingId);
    }
    lock_named_tx(tx, "apparatus-schedule-reservation", reservation_id).await
}

pub(super) async fn lock_schedule_idempotency_tx(
    tx: &mut Transaction<'_, Postgres>,
    idempotency_key: &str,
) -> Result<(), ProductionMapError> {
    let idempotency_key = idempotency_key.trim();
    if idempotency_key.is_empty() {
        return Err(ProductionMapError::ScheduleInputInvalid);
    }
    lock_named_tx(tx, "apparatus-schedule-idempotency", idempotency_key).await
}

async fn lock_named_tx(
    tx: &mut Transaction<'_, Postgres>,
    scope: &str,
    value: &str,
) -> Result<(), ProductionMapError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 0))")
        .bind(advisory_lock_key(scope, value))
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

fn advisory_lock_key(scope: &str, value: &str) -> String {
    format!("mini-rs-erp:production-map:{}:{}", scope.trim(), value.trim())
}

#[cfg(test)]
mod tests {
    use super::advisory_lock_key;

    #[test]
    fn advisory_lock_keys_are_namespaced_by_resource_kind() {
        assert_ne!(
            advisory_lock_key("order", "zakaz-1"),
            advisory_lock_key("apparatus", "zakaz-1")
        );
        assert_eq!(
            advisory_lock_key("order", " zakaz-1 "),
            advisory_lock_key("order", "zakaz-1")
        );
    }
}
