use super::*;

#[derive(sqlx::FromRow)]
pub(super) struct Destination {
    id: String,
    name: String,
}

pub(super) fn destination_id(value: &str) -> Result<&str, WasteBinError> {
    if value.trim().is_empty() {
        return Err(WasteBinError::WarehouseRequired);
    }
    identity(value)
}

pub(super) async fn assigned_warehouse(
    db: &mut PgConnection,
    id: &str,
    actor_ref: &str,
) -> Result<Destination, WasteBinError> {
    // Hold the matching assignment and the warehouse until the credit
    // commits. Unassignment, deletion and a receipt cannot race.
    sqlx::query_as(
        "SELECT w.id, w.name
         FROM mini_warehouses w
         JOIN mini_warehouse_assignments a ON lower(a.warehouse_name) = lower(w.name)
         WHERE w.id = $1 AND w.is_waste AND NOT w.is_group
           AND a.assignment_kind = 'warehouse' AND a.principal_role = 'chiqindi_qabulchi'
           AND a.principal_ref = $2
         LIMIT 1 FOR SHARE OF w, a",
    )
    .bind(id)
    .bind(actor_ref)
    .fetch_optional(db)
    .await?
    .ok_or(WasteBinError::WarehouseNotAssigned)
}

pub(super) async fn stock_receipt(
    db: &mut PgConnection,
    cycle_id: &str,
    destination: &Destination,
    actor: &QueueActionActor,
) -> Result<(), WasteBinError> {
    sqlx::query(
        "INSERT INTO mini_waste_warehouse_receipts
         (cycle_id, warehouse_id, warehouse_name, stocked_by_ref, stocked_by_display_name)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(cycle_id)
    .bind(&destination.id)
    .bind(&destination.name)
    .bind(&actor.ref_)
    .bind(&actor.display_name)
    .execute(db)
    .await?;
    Ok(())
}

impl PostgresWasteBinStore {
    pub async fn warehouses(&self, receiver_ref: Option<&str>) -> Result<Value, WasteBinError> {
        let warehouses: Vec<Value> = sqlx::query_scalar(
            "SELECT jsonb_build_object(
                'id', w.id,
                'warehouse', w.name,
                'on_hand_kg', COALESCE(stock.on_hand_kg, 0),
                'receipt_count', COALESCE(stock.receipt_count, 0)
             )
             FROM mini_warehouses w
             LEFT JOIN mini_waste_warehouse_balances stock ON stock.warehouse_id = w.id
             WHERE w.is_waste AND NOT w.is_group AND ($1::text IS NULL OR EXISTS (
                 SELECT 1 FROM mini_warehouse_assignments a
                 WHERE a.assignment_kind = 'warehouse' AND lower(a.warehouse_name) = lower(w.name)
                   AND a.principal_role = 'chiqindi_qabulchi' AND a.principal_ref = $1
             ))
             ORDER BY lower(w.name), w.id",
        )
        .bind(receiver_ref)
        .fetch_all(&self.pool)
        .await?;
        Ok(json!({"warehouses": warehouses}))
    }

    pub async fn record_legacy_destination(
        &self,
        raw_code: &str,
        cycle_id: &str,
        warehouse_id: &str,
        actor: &QueueActionActor,
    ) -> Result<Value, WasteBinError> {
        receiver(actor)?;
        let code = code(raw_code)?;
        let warehouse_id = destination_id(warehouse_id)?;
        let mut tx = self.pool.begin().await?;
        lock_bins(&mut tx, std::slice::from_ref(&code)).await?;
        cycle_apparatus(&mut tx, &code, cycle_id).await?;
        let cycle = read::load_cycle(&mut tx, cycle_id).await?;
        let receipt = cycle.receipt()?.ok_or(WasteBinError::Conflict)?;
        if cycle.reset_at_unix.is_some() {
            return Err(WasteBinError::Conflict);
        }
        match receipt.warehouse_id.as_deref() {
            Some(existing) if existing == warehouse_id => (),
            Some(_) => return Err(WasteBinError::Conflict),
            None => {
                let destination = assigned_warehouse(&mut tx, warehouse_id, &actor.ref_).await?;
                stock_receipt(&mut tx, cycle_id, &destination, actor).await?;
            }
        }
        let result = read::bin_snapshot(&mut tx, &code, Some(cycle_id)).await?;
        tx.commit().await?;
        Ok(result)
    }
}
