use std::collections::{BTreeMap, BTreeSet};

use sqlx::{Postgres, Transaction};

use crate::core::apparatus_standard::ApparatusId;
use crate::core::production_map::{
    ProductionMapError, QueueActionActor, RawMaterialStockTransition,
    RawMaterialStockTransitionKind,
};
use crate::db::postgres_raw_material_events::{
    RawMaterialEventDraft, insert_raw_material_event_tx,
};

#[derive(Debug, Default)]
pub(super) struct RawMaterialStockTransitionOutcome {
    pub(super) warehouses: Vec<String>,
    pub(super) unused_unlinks: Vec<UnusedRawMaterialUnlink>,
}

#[derive(Debug, Clone)]
pub(super) struct UnusedRawMaterialUnlink {
    pub(super) barcode: String,
    pub(super) stock_status: String,
    pub(super) reserved_order_id: String,
    pub(super) warehouse: String,
}

pub(super) async fn apply_raw_material_stock_transitions_tx(
    tx: &mut Transaction<'_, Postgres>,
    transitions: &[RawMaterialStockTransition],
    actor: &QueueActionActor,
    apparatus: &str,
) -> Result<RawMaterialStockTransitionOutcome, ProductionMapError> {
    let apparatus_id = ApparatusId::new(apparatus.trim().to_string())
        .map_err(|_| ProductionMapError::RawMaterialInvalidInput)?;
    let mut warehouses = BTreeSet::new();
    let mut unused_unlinks = Vec::new();
    for transition in transitions {
        if transition.is_empty() {
            continue;
        }
        let barcodes = normalized_barcodes(&transition.barcodes);
        if barcodes.is_empty() || transition.order_id.trim().is_empty() {
            continue;
        }
        let before = raw_material_stock_rows_for_update_tx(tx, &barcodes).await?;
        let owners = raw_material_assignment_owners_tx(tx, &barcodes, &transition.order_id).await?;
        let rows = match transition.kind {
            RawMaterialStockTransitionKind::InUse => {
                let rows = mark_raw_material_stock_in_use_tx(
                    tx,
                    &barcodes,
                    &transition.order_id,
                    apparatus_id.as_str(),
                )
                .await
                .map_err(stock_transition_store_error(&transition.order_id))?;
                ensure_stock_transition_rows_affected(transition.kind, barcodes.len(), rows.len())?;
                rows
            }
            RawMaterialStockTransitionKind::Consumed => {
                let rows = mark_raw_material_stock_consumed_tx(
                    tx,
                    &barcodes,
                    &transition.order_id,
                    apparatus_id.as_str(),
                )
                .await
                .map_err(stock_transition_store_error(&transition.order_id))?;
                ensure_stock_transition_rows_affected(transition.kind, barcodes.len(), rows.len())?;
                rows
            }
            RawMaterialStockTransitionKind::Complete => {
                let settlement = settle_completion_raw_materials_tx(
                    tx,
                    &barcodes,
                    &transition.order_id,
                    apparatus_id.as_str(),
                    &before,
                )
                .await?;
                warehouses.extend(
                    settlement
                        .unused_unlinks
                        .iter()
                        .map(|unlink| unlink.warehouse.trim().to_string())
                        .filter(|warehouse| !warehouse.is_empty()),
                );
                unused_unlinks.extend(settlement.unused_unlinks);
                settlement.consumed_rows
            }
        };
        for row in &rows {
            let previous = before.get(&stock_key(&row.barcode));
            let owner = owners.get(&stock_key(&row.barcode));
            insert_raw_material_event_tx(
                tx,
                stock_transition_event_draft(
                    transition.kind,
                    row,
                    previous.map(|row| row.status.clone()),
                    &transition.order_id,
                    actor,
                    apparatus_id.as_str(),
                    owner,
                ),
            )
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        }
        warehouses.extend(
            rows.into_iter()
                .map(|row| row.warehouse.trim().to_string())
                .filter(|warehouse| !warehouse.is_empty()),
        );
    }
    Ok(RawMaterialStockTransitionOutcome {
        warehouses: warehouses.into_iter().collect(),
        unused_unlinks,
    })
}

fn stock_transition_store_error(
    order_id: &str,
) -> impl FnOnce(sqlx::Error) -> ProductionMapError + '_ {
    move |error| {
        tracing::error!(
            error = %error,
            order_id,
            "failed to update raw material stock inside queue action transaction"
        );
        ProductionMapError::StoreFailed
    }
}

struct CompletionMaterialSettlement {
    consumed_rows: Vec<RawMaterialStockTransitionRow>,
    unused_unlinks: Vec<UnusedRawMaterialUnlink>,
}

async fn settle_completion_raw_materials_tx(
    tx: &mut Transaction<'_, Postgres>,
    barcodes: &[String],
    order_id: &str,
    apparatus_id: &str,
    before: &BTreeMap<String, RawMaterialStockTransitionRow>,
) -> Result<CompletionMaterialSettlement, ProductionMapError> {
    if before.len() != barcodes.len() {
        return Err(ProductionMapError::RawMaterialStockUnavailable);
    }
    let order_id = order_id.trim();
    let mut consumed_barcodes = Vec::new();
    let mut unused_barcodes = Vec::new();
    for barcode in barcodes {
        let row = before
            .get(&stock_key(barcode))
            .ok_or(ProductionMapError::RawMaterialStockUnavailable)?;
        let status = row.status.trim().to_ascii_lowercase();
        let reservation = row.reserved_order_id.trim();
        if matches!(status.as_str(), "in_use" | "consumed") && reservation == order_id {
            consumed_barcodes.push(barcode.clone());
        } else if status == "available" && (reservation.is_empty() || reservation == order_id) {
            unused_barcodes.push(barcode.clone());
        } else {
            return Err(ProductionMapError::RawMaterialStockUnavailable);
        }
    }

    let consumed_rows = if consumed_barcodes.is_empty() {
        Vec::new()
    } else {
        let rows =
            mark_raw_material_stock_consumed_tx(tx, &consumed_barcodes, order_id, apparatus_id)
                .await
                .map_err(stock_transition_store_error(order_id))?;
        ensure_stock_transition_rows_affected(
            RawMaterialStockTransitionKind::Consumed,
            consumed_barcodes.len(),
            rows.len(),
        )?;
        rows
    };

    let unused_unlinks = if unused_barcodes.is_empty() {
        Vec::new()
    } else {
        unlink_unused_raw_material_assignments_tx(tx, &unused_barcodes, order_id, apparatus_id)
            .await?;
        unused_barcodes
            .iter()
            .filter_map(|barcode| before.get(&stock_key(barcode)))
            .map(|row| UnusedRawMaterialUnlink {
                barcode: row.barcode.trim().to_string(),
                stock_status: row.status.trim().to_string(),
                reserved_order_id: row.reserved_order_id.trim().to_string(),
                warehouse: row.warehouse.trim().to_string(),
            })
            .collect()
    };

    Ok(CompletionMaterialSettlement {
        consumed_rows,
        unused_unlinks,
    })
}

async fn unlink_unused_raw_material_assignments_tx(
    tx: &mut Transaction<'_, Postgres>,
    barcodes: &[String],
    order_id: &str,
    apparatus_id: &str,
) -> Result<(), ProductionMapError> {
    let removed = sqlx::query_scalar::<_, String>(
        "DELETE FROM mini_raw_material_assignments
         WHERE lower(barcode) = ANY($1)
           AND order_id = $2
           AND canonical_apparatus_id = $3
         RETURNING barcode",
    )
    .bind(barcodes)
    .bind(order_id.trim())
    .bind(apparatus_id.trim())
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if removed.len() != barcodes.len() {
        return Err(ProductionMapError::RawMaterialStockUnavailable);
    }
    sqlx::query(
        "UPDATE mini_raw_material_stock
         SET reserved_order_id = '',
             payload_json = payload_json - 'in_use_order_id',
             updated_at = now()
         WHERE lower(barcode) = ANY($1)
           AND status = 'available'
           AND reserved_order_id = $2",
    )
    .bind(barcodes)
    .bind(order_id.trim())
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

async fn mark_raw_material_stock_in_use_tx(
    tx: &mut Transaction<'_, Postgres>,
    barcodes: &[String],
    order_id: &str,
    apparatus_id: &str,
) -> Result<Vec<RawMaterialStockTransitionRow>, sqlx::Error> {
    sqlx::query_as::<_, RawMaterialStockTransitionRow>(
        "UPDATE mini_raw_material_stock AS stock
         SET status = 'in_use',
             reserved_order_id = $2,
             payload_json = jsonb_set(stock.payload_json, '{in_use_order_id}', to_jsonb($2::text), true),
             updated_at = now()
         WHERE lower(stock.barcode) = ANY($1)
           AND (stock.status = 'available' OR (stock.status = 'in_use' AND stock.reserved_order_id = $2))
           AND EXISTS (
               SELECT 1
               FROM mini_raw_material_assignments AS assignment
               WHERE lower(assignment.barcode) = lower(stock.barcode)
                 AND assignment.order_id = $2
                 AND assignment.canonical_apparatus_id = $3
           )
         RETURNING id, warehouse, item_code, item_name, barcode,
                   qty::float8 AS qty, uom,
                   status, reserved_order_id, source_receipt_id",
    )
    .bind(barcodes)
    .bind(order_id.trim())
    .bind(apparatus_id.trim())
    .fetch_all(&mut **tx)
    .await
}

async fn mark_raw_material_stock_consumed_tx(
    tx: &mut Transaction<'_, Postgres>,
    barcodes: &[String],
    order_id: &str,
    apparatus_id: &str,
) -> Result<Vec<RawMaterialStockTransitionRow>, sqlx::Error> {
    sqlx::query_as::<_, RawMaterialStockTransitionRow>(
        "UPDATE mini_raw_material_stock AS stock
         SET status = 'consumed',
             payload_json = jsonb_set(stock.payload_json, '{consumed_order_id}', to_jsonb($2::text), true),
             updated_at = now()
         WHERE lower(stock.barcode) = ANY($1)
           AND stock.reserved_order_id = $2
           AND stock.status IN ('in_use', 'consumed')
           AND EXISTS (
               SELECT 1
               FROM mini_raw_material_assignments AS assignment
               WHERE lower(assignment.barcode) = lower(stock.barcode)
                 AND assignment.order_id = $2
                 AND assignment.canonical_apparatus_id = $3
           )
         RETURNING id, warehouse, item_code, item_name, barcode,
                   qty::float8 AS qty, uom,
                   status, reserved_order_id, source_receipt_id",
    )
    .bind(barcodes)
    .bind(order_id.trim())
    .bind(apparatus_id.trim())
    .fetch_all(&mut **tx)
    .await
}

#[derive(Clone, sqlx::FromRow)]
struct RawMaterialStockTransitionRow {
    id: String,
    warehouse: String,
    item_code: String,
    item_name: String,
    barcode: String,
    qty: f64,
    uom: String,
    status: String,
    reserved_order_id: String,
    source_receipt_id: String,
}

struct RawMaterialOwner {
    role: String,
    ref_: String,
    display_name: String,
}

async fn raw_material_stock_rows_for_update_tx(
    tx: &mut Transaction<'_, Postgres>,
    barcodes: &[String],
) -> Result<BTreeMap<String, RawMaterialStockTransitionRow>, ProductionMapError> {
    let rows = sqlx::query_as::<_, RawMaterialStockTransitionRow>(
        "SELECT id, warehouse, item_code, item_name, barcode,
                qty::float8 AS qty, uom,
                status, reserved_order_id, source_receipt_id
         FROM mini_raw_material_stock
         WHERE lower(barcode) = ANY($1)
         FOR UPDATE",
    )
    .bind(barcodes)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(rows
        .into_iter()
        .map(|row| (stock_key(&row.barcode), row))
        .collect())
}

async fn raw_material_assignment_owners_tx(
    tx: &mut Transaction<'_, Postgres>,
    barcodes: &[String],
    order_id: &str,
) -> Result<BTreeMap<String, RawMaterialOwner>, ProductionMapError> {
    let rows = sqlx::query_as::<_, RawMaterialAssignmentOwnerRow>(
        "SELECT barcode,
                COALESCE(payload_json->>'assigned_by_role', '') AS owner_role,
                COALESCE(payload_json->>'assigned_by_ref', '') AS owner_ref,
                COALESCE(payload_json->>'assigned_by_display_name', '') AS owner_display_name
         FROM mini_raw_material_assignments
         WHERE lower(barcode) = ANY($1)
           AND order_id = $2",
    )
    .bind(barcodes)
    .bind(order_id.trim())
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(rows
        .into_iter()
        .map(|row| {
            (
                stock_key(&row.barcode),
                RawMaterialOwner {
                    role: row.owner_role,
                    ref_: row.owner_ref,
                    display_name: row.owner_display_name,
                },
            )
        })
        .collect())
}

#[derive(sqlx::FromRow)]
struct RawMaterialAssignmentOwnerRow {
    barcode: String,
    owner_role: String,
    owner_ref: String,
    owner_display_name: String,
}

fn stock_transition_event_draft(
    kind: RawMaterialStockTransitionKind,
    row: &RawMaterialStockTransitionRow,
    previous_status: Option<String>,
    order_id: &str,
    actor: &QueueActionActor,
    apparatus: &str,
    owner: Option<&RawMaterialOwner>,
) -> RawMaterialEventDraft {
    let (event_type, status_after, qty_delta) = match kind {
        RawMaterialStockTransitionKind::InUse => ("usage_started", "in_use", 0.0),
        RawMaterialStockTransitionKind::Consumed | RawMaterialStockTransitionKind::Complete => {
            ("consumption_posted", "consumed", -row.qty)
        }
    };
    RawMaterialEventDraft {
        idempotency_key: format!(
            "{}:{}:{}:{}",
            event_type,
            row.barcode.trim().to_ascii_uppercase(),
            order_id.trim(),
            apparatus.trim()
        ),
        event_type: event_type.to_string(),
        warehouse: row.warehouse.trim().to_string(),
        barcode: row.barcode.trim().to_string(),
        item_code: row.item_code.trim().to_string(),
        item_name: row.item_name.trim().to_string(),
        qty_delta,
        uom: row.uom.trim().to_string(),
        stock_status_before: previous_status,
        stock_status_after: Some(status_after.to_string()),
        order_id: Some(order_id.trim().to_string()),
        apparatus: Some(apparatus.trim().to_string()),
        actor_role: actor.role.trim().to_string(),
        actor_ref: actor.ref_.trim().to_string(),
        actor_display_name: actor.display_name.trim().to_string(),
        owner_role: owner
            .filter(|owner| owner.role.trim() == "material_taminotchi")
            .map(|owner| owner.role.trim().to_string())
            .unwrap_or_default(),
        owner_ref: owner
            .filter(|owner| owner.role.trim() == "material_taminotchi")
            .map(|owner| owner.ref_.trim().to_string())
            .unwrap_or_default(),
        owner_display_name: owner
            .filter(|owner| owner.role.trim() == "material_taminotchi")
            .map(|owner| owner.display_name.trim().to_string())
            .unwrap_or_default(),
        source_type: "consumption".to_string(),
        source_id: order_id.trim().to_string(),
        source_line_ref: Some(row.barcode.trim().to_string()),
        correlation_id: None,
        payload_json: serde_json::json!({
            "stock_id": row.id.trim(),
            "barcode": row.barcode.trim(),
            "order_id": order_id.trim(),
            "apparatus_id": apparatus.trim(),
            "reserved_order_id": row.reserved_order_id.trim(),
            "source_receipt_id": row.source_receipt_id.trim(),
        }),
    }
}

fn stock_key(barcode: &str) -> String {
    barcode.trim().to_ascii_lowercase()
}

fn normalized_barcodes(barcodes: &[String]) -> Vec<String> {
    barcodes
        .iter()
        .map(|barcode| barcode.trim().to_ascii_lowercase())
        .filter(|barcode| !barcode.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn ensure_stock_transition_rows_affected(
    kind: RawMaterialStockTransitionKind,
    requested_rows: usize,
    affected_rows: usize,
) -> Result<(), ProductionMapError> {
    if matches!(
        kind,
        RawMaterialStockTransitionKind::InUse | RawMaterialStockTransitionKind::Consumed
    ) && requested_rows != affected_rows
    {
        return Err(ProductionMapError::RawMaterialStockUnavailable);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{
        RawMaterialStockTransitionRow, apply_raw_material_stock_transitions_tx,
        ensure_stock_transition_rows_affected, stock_transition_event_draft,
    };
    use crate::core::production_map::{
        ProductionMapError, QueueActionActor, RawMaterialStockTransition,
        RawMaterialStockTransitionKind,
    };
    use crate::core::apparatus_standard::{
        ApparatusId, ProcessTechnology,
        service::CanonicalApparatusService,
        test_support::{TestApparatusSpec, canonical_draft},
    };
    use crate::db::postgres::{apply_foundation_migration, postgres_test_database_options};
    use crate::db::postgres_canonical_apparatus::PostgresCanonicalApparatusRepository;

    #[test]
    fn consumed_transition_rejects_a_silent_zero_row_update() {
        assert_eq!(
            ensure_stock_transition_rows_affected(RawMaterialStockTransitionKind::Consumed, 1, 0,),
            Err(ProductionMapError::RawMaterialStockUnavailable)
        );
        assert_eq!(
            ensure_stock_transition_rows_affected(RawMaterialStockTransitionKind::Consumed, 2, 2,),
            Ok(())
        );
        assert_eq!(
            ensure_stock_transition_rows_affected(RawMaterialStockTransitionKind::InUse, 2, 1,),
            Err(ProductionMapError::RawMaterialStockUnavailable)
        );
    }

    #[test]
    fn consumption_event_identity_is_scoped_to_canonical_apparatus() {
        let row = RawMaterialStockTransitionRow {
            id: "raw:001".to_string(),
            warehouse: "Raw warehouse".to_string(),
            item_code: "FILM-001".to_string(),
            item_name: "Film".to_string(),
            barcode: "RM-001".to_string(),
            qty: 4.5,
            uom: "kg".to_string(),
            status: "consumed".to_string(),
            reserved_order_id: "ORDER-001".to_string(),
            source_receipt_id: "receipt-001".to_string(),
        };
        let actor = QueueActionActor {
            role: "admin".to_string(),
            ref_: "admin-001".to_string(),
            display_name: "Admin".to_string(),
        };
        let first = stock_transition_event_draft(
            RawMaterialStockTransitionKind::Consumed,
            &row,
            Some("in_use".to_string()),
            "ORDER-001",
            &actor,
            "apparatus:catalog:first",
            None,
        );
        let second = stock_transition_event_draft(
            RawMaterialStockTransitionKind::Consumed,
            &row,
            Some("in_use".to_string()),
            "ORDER-001",
            &actor,
            "apparatus:catalog:second",
            None,
        );

        assert_ne!(first.idempotency_key, second.idempotency_key);
        assert_eq!(first.qty_delta, -4.5);
        assert_eq!(
            first.payload_json["apparatus_id"],
            serde_json::json!("apparatus:catalog:first")
        );
    }

    #[tokio::test]
    async fn completion_consumes_in_use_and_unlinks_available_material_atomically() {
        let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
        let db_name = "mini_rs_erp_test_complete_material_settlement";
        let admin_pool = sqlx::PgPool::connect(&admin_url).await.expect("admin db");
        sqlx::query(&format!(
            r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
        ))
        .execute(&admin_pool)
        .await
        .expect("drop test db");
        sqlx::query(&format!(r#"CREATE DATABASE "{db_name}""#))
            .execute(&admin_pool)
            .await
            .expect("create test db");
        admin_pool.close().await;

        let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&admin_url, db_name))
            .await
            .expect("test db");
        apply_foundation_migration(&pool)
            .await
            .expect("apply migrations");
        sqlx::query(
            "INSERT INTO mini_production_maps (id, product_code, title, map_json)
             VALUES ('ORDER-COMPLETE', 'COMPLETE', 'Completion material test', '{}'::jsonb)",
        )
        .execute(&pool)
        .await
        .expect("seed production map");
        CanonicalApparatusService::new(Arc::new(PostgresCanonicalApparatusRepository::new(
            pool.clone(),
        )))
        .seed_for_test(
            ApparatusId::new("apparatus:default:bosma_7".to_string())
                .expect("canonical apparatus id"),
            canonical_draft(&TestApparatusSpec::print(
                "apparatus:default:bosma_7",
                "Bosma 7",
                ProcessTechnology::Rotogravure,
                Some(7),
            )),
        )
        .await
        .expect("seed canonical apparatus");
        sqlx::query(
            "INSERT INTO mini_raw_material_stock (
                 id, warehouse, item_code, item_name, barcode, qty, uom,
                 status, reserved_order_id, source_receipt_id, payload_json
             ) VALUES
                 ('raw:complete-used', 'Warehouse A', 'BOPP', 'Used roll',
                  'COMPLETE-USED', 11, 'kg', 'in_use', 'ORDER-COMPLETE',
                  'receipt-used', '{\"physical_marker\":\"used\"}'::jsonb),
                 ('raw:complete-unused', 'Warehouse B', 'BOPP', 'Unused roll',
                  'COMPLETE-UNUSED', 23, 'kg', 'available', 'ORDER-COMPLETE',
                  'receipt-unused',
                  '{\"physical_marker\":\"keep\",\"in_use_order_id\":\"ORDER-COMPLETE\"}'::jsonb)",
        )
        .execute(&pool)
        .await
        .expect("seed raw material stock");
        sqlx::query(
            "INSERT INTO mini_factory_locations (id, name)
             VALUES ('state:complete-unused', 'Completion unused state')",
        )
        .execute(&pool)
        .await
        .expect("seed unused stock state");
        sqlx::query(
            "INSERT INTO mini_inventory_placements (
                 asset_kind, asset_ref, physical_location_id
             ) VALUES (
                 'raw_material', 'raw:complete-unused',
                 'inventory_location:state:state:complete-unused'
             )",
        )
        .execute(&pool)
        .await
        .expect("place unused stock in state");
        for barcode in ["COMPLETE-USED", "COMPLETE-UNUSED"] {
            sqlx::query(
                "INSERT INTO mini_raw_material_assignments (
                     barcode, order_id, apparatus, canonical_apparatus_id,
                     item_code, item_group, payload_json
                 ) VALUES ($1, 'ORDER-COMPLETE', 'apparatus:default:bosma_7',
                           'apparatus:default:bosma_7', 'BOPP', 'rulon',
                           jsonb_build_object(
                               'barcode', $1::text,
                               'order_id', 'ORDER-COMPLETE',
                               'apparatus', 'apparatus:default:bosma_7',
                               'apparatus_id', 'apparatus:default:bosma_7',
                               'item_code', 'BOPP',
                               'item_group', 'rulon'
                           ))",
            )
            .bind(barcode)
            .execute(&pool)
            .await
            .expect("seed raw material assignment");
        }

        let mut tx = pool.begin().await.expect("begin settlement");
        let outcome = apply_raw_material_stock_transitions_tx(
            &mut tx,
            &[RawMaterialStockTransition::new(
                RawMaterialStockTransitionKind::Complete,
                vec!["COMPLETE-USED".to_string(), "COMPLETE-UNUSED".to_string()],
                "ORDER-COMPLETE",
            )],
            &QueueActionActor {
                role: "aparatchi".to_string(),
                ref_: "WORKER-1".to_string(),
                display_name: "Worker".to_string(),
            },
            "apparatus:default:bosma_7",
        )
        .await
        .expect("settle completion materials");
        tx.commit().await.expect("commit settlement");

        assert_eq!(outcome.unused_unlinks.len(), 1);
        assert_eq!(outcome.unused_unlinks[0].barcode, "COMPLETE-UNUSED");
        let rows: Vec<(String, String, String, String, String, String)> = sqlx::query_as(
            "SELECT barcode, status, reserved_order_id, warehouse,
                    COALESCE(payload_json->>'physical_marker', ''),
                    COALESCE(payload_json->>'in_use_order_id', '')
             FROM mini_raw_material_stock
             WHERE barcode IN ('COMPLETE-USED', 'COMPLETE-UNUSED')
             ORDER BY barcode",
        )
        .fetch_all(&pool)
        .await
        .expect("settled stock rows");
        assert_eq!(
            rows,
            vec![
                (
                    "COMPLETE-UNUSED".to_string(),
                    "available".to_string(),
                    "".to_string(),
                    "Warehouse B".to_string(),
                    "keep".to_string(),
                    "".to_string(),
                ),
                (
                    "COMPLETE-USED".to_string(),
                    "consumed".to_string(),
                    "ORDER-COMPLETE".to_string(),
                    "Warehouse A".to_string(),
                    "used".to_string(),
                    "".to_string(),
                ),
            ]
        );
        let assignments: Vec<String> = sqlx::query_scalar(
            "SELECT barcode FROM mini_raw_material_assignments ORDER BY barcode",
        )
        .fetch_all(&pool)
        .await
        .expect("remaining assignments");
        assert_eq!(assignments, vec!["COMPLETE-USED".to_string()]);
        let consumed_events: Vec<String> = sqlx::query_scalar(
            "SELECT barcode FROM mini_raw_material_events
             WHERE event_type = 'consumption_posted'
             ORDER BY barcode",
        )
        .fetch_all(&pool)
        .await
        .expect("consumption audit");
        assert_eq!(consumed_events, vec!["COMPLETE-USED".to_string()]);
        let unused_location: String = sqlx::query_scalar(
            "SELECT physical_location_id
             FROM mini_inventory_placements
             WHERE asset_kind = 'raw_material'
               AND asset_ref = 'raw:complete-unused'",
        )
        .fetch_one(&pool)
        .await
        .expect("unused stock remains in its physical state");
        assert_eq!(
            unused_location,
            "inventory_location:state:state:complete-unused"
        );

        pool.close().await;
        let admin_pool = sqlx::PgPool::connect(&admin_url)
            .await
            .expect("admin cleanup");
        sqlx::query(&format!(
            r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
        ))
        .execute(&admin_pool)
        .await
        .expect("cleanup test db");
        admin_pool.close().await;
    }
}
