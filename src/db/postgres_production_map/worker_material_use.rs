//! Direct worker assignment keeps permission, placement, assignment and use in one commit.
use super::material_helpers::save_raw_material_assignment_tx;
use super::material_link_requests::LinkCandidate;
use super::*;
use crate::core::production_map::queue_state;
use sqlx::{Postgres, Transaction};

const CANDIDATES: &str = r#"
SELECT stock.id AS stock_id, stock.barcode, stock.item_name, stock.qty::float8 AS qty,
       stock.uom, location.id AS location_id, location.name AS location_name,
       placement.version AS placement_version, placement.updated_by_ref AS mover_ref,
       placement.updated_by_name AS mover_display_name
FROM mini_raw_material_stock stock
JOIN mini_inventory_placements placement
  ON placement.asset_kind='raw_material' AND placement.asset_ref=stock.id
JOIN mini_inventory_locations location ON location.id=placement.physical_location_id
JOIN mini_factory_location_apparatus_links link ON link.location_id=location.factory_location_id
WHERE link.apparatus_id=$1 AND location.kind='state' AND location.active
  AND stock.status='available' AND stock.qty>0 AND stock.reserved_order_id=''
  AND COALESCE(stock.payload_json->>'inventory_transfer_id','')=''
  AND NOT EXISTS (SELECT 1 FROM mini_raw_material_assignments a
                  WHERE lower(a.barcode)=lower(stock.barcode))
AND NOT EXISTS (SELECT 1 FROM mini_queue_states q WHERE q.canonical_apparatus_id=$1 AND q.order_id=$2
                  AND q.state NOT IN ('pending','print_preflight','in_progress','paused'))
ORDER BY stock.id
"#;

pub(crate) struct WorkerMaterialUseResult {
    pub assignment: RawMaterialAssignment,
    pub in_use: bool,
}

impl PostgresProductionMapStore {
    pub(crate) async fn worker_material_validation_snapshot(
        &self,
        barcode: &str,
    ) -> Result<serde_json::Value, ProductionMapError> {
        validation_snapshot(&self.pool, barcode).await
    }
    pub(crate) async fn worker_material_candidates(
        &self,
        apparatus: &str,
        order_id: &str,
    ) -> Result<Vec<LinkCandidate>, ProductionMapError> {
        sqlx::query_as(CANDIDATES)
            .bind(apparatus)
            .bind(order_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)
    }

    pub(crate) async fn use_worker_material(
        &self,
        assignment: RawMaterialAssignment,
        expected_revision: u64,
        expected_map: &ProductionMapDefinition,
        placement_version: i64,
        expected_stock: &serde_json::Value,
        actor: &QueueActionActor,
    ) -> Result<WorkerMaterialUseResult, ProductionMapError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        lock_order_and_apparatuses_tx(
            &mut tx,
            &assignment.order_id,
            &[assignment.apparatus_id.as_str()],
        )
        .await?;
        // Canonical updates also write this row. Hold it until assignment/use commits.
        let current: Option<(i64, serde_json::Value, String)> = sqlx::query_as(
            "SELECT source_revision,policies_json,lifecycle_state FROM mini_apparatus WHERE id=$1 FOR SHARE")
            .bind(assignment.apparatus_id.as_str()).fetch_optional(&mut *tx).await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        if !current.is_some_and(|(revision, policies, lifecycle)| {
            revision == expected_revision as i64
                && lifecycle == "active"
                && policies["worker_material_assignment_allowed"] == true
        }) {
            return Err(ProductionMapError::ApparatusNotAssigned);
        }
        let map: Option<(String, serde_json::Value)> = sqlx::query_as(
            "SELECT lifecycle_status,map_json FROM mini_production_maps WHERE id=$1 FOR UPDATE",
        )
        .bind(&assignment.order_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        let Some((lifecycle, map)) = map else {
            return Err(ProductionMapError::MapNotFound);
        };
        let current_map: ProductionMapDefinition =
            serde_json::from_value(map).map_err(|_| ProductionMapError::StoreFailed)?;
        if !matches!(lifecycle.as_str(), "released" | "in_progress") || &current_map != expected_map
        {
            return Err(ProductionMapError::RawMaterialOrderNotActive);
        }
        let control: Option<String> = sqlx::query_scalar(
            "SELECT state FROM mini_order_control_states WHERE order_id=$1 FOR UPDATE",
        )
        .bind(&assignment.order_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        match control.as_deref().unwrap_or("active") {
            "active" => {}
            "freeze_requested" => return Err(ProductionMapError::OrderFreezeRequested),
            _ => return Err(ProductionMapError::OrderFrozen),
        }
        let queue: Option<String> = sqlx::query_scalar(
            "SELECT state FROM mini_queue_states WHERE order_id=$1 AND canonical_apparatus_id=$2 FOR UPDATE")
            .bind(&assignment.order_id).bind(assignment.apparatus_id.as_str())
            .fetch_optional(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        let queue =
            queue_state::ApparatusQueueOrderState::parse(queue.as_deref().unwrap_or("pending"))
                .ok_or(ProductionMapError::RawMaterialOrderNotActive)?;
        if !matches!(
            queue,
            queue_state::ApparatusQueueOrderState::Pending
                | queue_state::ApparatusQueueOrderState::PrintPreflight
        ) && !queue.allows_material_intake()
        {
            return Err(ProductionMapError::RawMaterialOrderNotActive);
        }
        let stock: Option<(String, String)> = sqlx::query_as(
            "SELECT id,status FROM mini_raw_material_stock WHERE lower(barcode)=lower($1) FOR UPDATE")
            .bind(&assignment.barcode).fetch_optional(&mut *tx).await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let Some((stock_id, status)) = stock else {
            return Err(ProductionMapError::RawMaterialStockUnavailable);
        };
        let existing: Option<serde_json::Value> = sqlx::query_scalar(
            "SELECT payload_json FROM mini_raw_material_assignments WHERE lower(barcode)=lower($1) FOR UPDATE")
            .bind(&assignment.barcode).fetch_optional(&mut *tx).await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        if let Some(existing) = existing {
            let existing: RawMaterialAssignment =
                serde_json::from_value(existing).map_err(|_| ProductionMapError::StoreFailed)?;
            // A transport retry from this worker returns the committed result without another use event.
            if existing.order_id == assignment.order_id
                && existing.apparatus_id == assignment.apparatus_id
                && existing.assigned_by_role == actor.role
                && existing.assigned_by_ref == actor.ref_
            {
                tx.commit()
                    .await
                    .map_err(|_| ProductionMapError::StoreFailed)?;
                return Ok(WorkerMaterialUseResult {
                    assignment: existing,
                    in_use: status == "in_use",
                });
            }
            return Err(ProductionMapError::RawMaterialAlreadyAssigned);
        }
        sqlx::query("SELECT item.code FROM mini_items item JOIN mini_raw_material_stock stock ON lower(stock.item_code)=lower(item.code) WHERE stock.id=$1 FOR SHARE OF item")
            .bind(&stock_id).fetch_all(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        let snapshot: serde_json::Value =
            validation_snapshot(&mut *tx, &assignment.barcode).await?;
        if &snapshot != expected_stock {
            return Err(ProductionMapError::RawMaterialStockUnavailable);
        }
        sqlx::query("SELECT asset_ref FROM mini_inventory_placements WHERE asset_kind='raw_material' AND asset_ref=$1 FOR UPDATE")
            .bind(&stock_id).fetch_all(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        // Keep the State active and linked until this placement has been consumed.
        let candidates: Vec<LinkCandidate> =
            sqlx::query_as(&format!("{CANDIDATES} FOR SHARE OF location, link"))
                .bind(assignment.apparatus_id.as_str())
                .bind(&assignment.order_id)
                .fetch_all(&mut *tx)
                .await
                .map_err(|_| ProductionMapError::StoreFailed)?;
        if !candidates
            .iter()
            .any(|c| c.stock_id == stock_id && c.placement_version == placement_version)
        {
            return Err(ProductionMapError::RawMaterialStockUnavailable);
        }
        save_raw_material_assignment_tx(&mut tx, &assignment).await?;
        let in_use = queue.allows_material_intake();
        if in_use {
            receive_assigned_material_tx(&mut tx, &assignment, actor).await?;
        }
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        Ok(WorkerMaterialUseResult { assignment, in_use })
    }
}

async fn validation_snapshot<'a, E>(
    executor: E,
    barcode: &str,
) -> Result<serde_json::Value, ProductionMapError>
where
    E: sqlx::Executor<'a, Database = Postgres>,
{
    sqlx::query_scalar("SELECT jsonb_build_object('stock',to_jsonb(stock),'item',to_jsonb(item))
        FROM mini_raw_material_stock stock JOIN mini_items item ON lower(item.code)=lower(stock.item_code)
        WHERE lower(stock.barcode)=lower($1)")
        .bind(barcode).fetch_optional(executor).await.map_err(|_| ProductionMapError::StoreFailed)?
        .ok_or(ProductionMapError::RawMaterialStockUnavailable)
}

pub(super) async fn receive_assigned_material_tx(
    tx: &mut Transaction<'_, Postgres>,
    assignment: &RawMaterialAssignment,
    actor: &QueueActionActor,
) -> Result<Vec<String>, ProductionMapError> {
    let outcome = apply_raw_material_stock_transitions_tx(
        tx,
        &[RawMaterialStockTransition::new(
            RawMaterialStockTransitionKind::InUse,
            vec![assignment.barcode.clone()],
            &assignment.order_id,
        )],
        actor,
        &assignment.apparatus,
    )
    .await?;
    // Additional intake is a resource of the current execution as well as
    // a stock transition. Keep both records in this same transaction.
    if let Some(mut session) = stage_execution::sessions_tx(tx, &assignment.order_id)
        .await?
        .into_iter()
        .filter(|session| session.apparatus == assignment.apparatus_id.as_str())
        .max_by_key(|session| (session.started_at_unix, session.session_id.clone()))
        .filter(|session| {
            matches!(
                session.status.as_str(),
                "active" | "paused" | "roll_detached"
            )
        })
    {
        sqlx::query(
            "SELECT session_id FROM mini_order_run_sessions WHERE session_id=$1 FOR UPDATE",
        )
        .bind(&session.session_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        let (stock_id, item_name, qty, uom) = sqlx::query_as::<_, (String, String, f64, String)>(
                    "SELECT id,item_name,qty::float8,uom FROM mini_raw_material_stock WHERE lower(barcode)=lower($1)",
                )
                .bind(&assignment.barcode)
                .fetch_one(&mut **tx)
                .await
                .map_err(|_| ProductionMapError::StoreFailed)?;
        let mut resources = ProductionQrSessionResources::for_session(&session);
        if !resources
            .raw_materials
            .iter()
            .any(|material| material.barcode.eq_ignore_ascii_case(&assignment.barcode))
        {
            resources
                .raw_materials
                .push(crate::core::production_map::ProductionQrRawMaterial {
                    barcode: assignment.barcode.clone(),
                    item_code: assignment.item_code.clone(),
                    item_name,
                    stock_id: Some(stock_id),
                    source_qty: Some(qty),
                    uom: Some(uom),
                });
        }
        resources.raw_materials_available = true;
        resources.write_to_session(&mut session);
        sqlx::query(
                    "UPDATE mini_order_run_sessions SET payload_json=$2,updated_at=now() WHERE session_id=$1",
                )
                    .bind(&session.session_id)
                    .bind(&session.payload_json)
                    .execute(&mut **tx)
                    .await
                    .map_err(|_| ProductionMapError::StoreFailed)?;
    }
    Ok(outcome.warehouses)
}
