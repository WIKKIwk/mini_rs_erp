use async_trait::async_trait;
use sqlx::{PgPool, Postgres, Transaction};

use crate::core::admin::models::AdminWarehouse;
use crate::core::auth::models::PrincipalRole;
use crate::core::warehouses::{
    WarehouseAssignment, WarehouseAssignmentIdentity, WarehouseDeleteResult, WarehouseError,
    WarehouseStockItem, WarehouseStorePort, WarehouseSummary,
};

#[derive(Clone)]
pub struct PostgresWarehouseStore {
    pool: PgPool,
}

impl PostgresWarehouseStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

include!("postgres_warehouse_impl_parts/part_01.rs");
include!("postgres_warehouse_impl_parts/part_02.rs");

include!("postgres_warehouse_trait_impl.rs");

async fn warehouse_delete_snapshot_tx(
    tx: &mut Transaction<'_, Postgres>,
    warehouse_id: &str,
    warehouse: &str,
) -> Result<WarehouseDeleteSnapshotRow, WarehouseError> {
    sqlx::query_as::<_, WarehouseDeleteSnapshotRow>(
        r#"
        SELECT
            (
                (SELECT count(*)
                 FROM mini_raw_material_stock stock
                 LEFT JOIN mini_inventory_placements placement
                   ON placement.asset_kind = 'raw_material'
                  AND lower(placement.asset_ref) = lower(stock.id)
                 LEFT JOIN mini_inventory_locations physical_location
                   ON physical_location.id = placement.physical_location_id
                 LEFT JOIN mini_warehouses physical_warehouse
                   ON physical_warehouse.id = physical_location.warehouse_id
                 WHERE lower(stock.warehouse) = lower($1)
                   AND stock.status = 'available'
                   AND stock.qty > 0
                   AND (
                       placement.asset_ref IS NULL
                       OR (
                           physical_location.kind = 'warehouse'
                           AND lower(physical_warehouse.name) = lower(stock.warehouse)
                       )
                   ))
                +
                (SELECT count(DISTINCT (lower(item_code), lower(uom)))
                 FROM mini_finished_goods_stock stock
                 LEFT JOIN mini_inventory_placements placement
                   ON placement.asset_kind = 'finished_goods'
                  AND lower(placement.asset_ref) = lower(stock.id)
                 LEFT JOIN mini_inventory_locations physical_location
                   ON physical_location.id = placement.physical_location_id
                 LEFT JOIN mini_warehouses physical_warehouse
                   ON physical_warehouse.id = physical_location.warehouse_id
                 WHERE lower(stock.warehouse) = lower($1)
                   AND stock.status = 'available'
                   AND stock.qty > 0
                   AND (
                       placement.asset_ref IS NULL
                       OR (
                           physical_location.kind = 'warehouse'
                           AND lower(physical_warehouse.name) = lower(stock.warehouse)
                       )
                   ))
                +
                (SELECT COALESCE(sum(stock.quantity), 0)
                 FROM mini_qolip_locations stock
                 LEFT JOIN mini_inventory_placements placement
                   ON placement.asset_kind = 'qolip'
                  AND lower(placement.asset_ref) = lower(stock.id)
                 LEFT JOIN mini_inventory_locations physical_location
                   ON physical_location.id = placement.physical_location_id
                 LEFT JOIN mini_warehouses physical_warehouse
                   ON physical_warehouse.id = physical_location.warehouse_id
                 WHERE lower(stock.warehouse) = lower($1)
                   AND (
                       placement.asset_ref IS NULL
                       OR (
                           physical_location.kind = 'warehouse'
                           AND lower(physical_warehouse.name) = lower(stock.warehouse)
                       )
                   ))
            )::bigint AS product_count,
            (
                (SELECT count(*)
                 FROM mini_raw_material_assignments assignment
                 JOIN mini_raw_material_stock stock
                   ON lower(stock.barcode) = lower(assignment.barcode)
                 WHERE lower(stock.warehouse) = lower($1))
                +
                (SELECT COALESCE(sum(GREATEST(quantity, 0)), 0)
                 FROM mini_qolip_checkouts
                 WHERE lower(status) = 'open' AND lower(warehouse) = lower($1))
                +
                (SELECT count(*)
                 FROM mini_inventory_transfers
                 WHERE status IN ('requested', 'approved', 'in_transit')
                   AND (
                       source_warehouse_id = $2
                       OR destination_warehouse_id = $2
                       OR lower(source_warehouse) = lower($1)
                       OR lower(destination_warehouse) = lower($1)
                   ))
            )::bigint AS reserved_count,
            (SELECT count(*)
             FROM mini_warehouse_assignments
             WHERE assignment_kind = 'warehouse'
               AND lower(warehouse_name) = lower($1))::bigint AS assignment_count
        "#,
    )
    .bind(warehouse)
    .bind(warehouse_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| WarehouseError::StoreFailed)
}

fn count_as_usize(value: i64) -> usize {
    usize::try_from(value.max(0)).unwrap_or(usize::MAX)
}

#[derive(sqlx::FromRow)]
struct WarehouseRow {
    name: String,
    company: String,
    is_group: bool,
    parent_warehouse: String,
}

#[derive(sqlx::FromRow)]
struct WarehouseAssignmentRow {
    assignment_kind: String,
    warehouse: String,
    warehouse_name: Option<String>,
    apparatus_id: Option<String>,
    principal_role: String,
    principal_ref: String,
    display_name: String,
}

#[derive(sqlx::FromRow)]
struct WarehouseSummaryRow {
    warehouse: String,
    product_count: i64,
    reserved_count: i64,
    assignment_count: i64,
    assigned_display_names: String,
}

#[derive(sqlx::FromRow)]
struct WarehouseDeleteSnapshotRow {
    product_count: i64,
    reserved_count: i64,
    assignment_count: i64,
}

#[derive(sqlx::FromRow)]
struct WarehouseStockItemRow {
    code: String,
    name: String,
    uom: String,
    warehouse: String,
    item_group: String,
    on_hand_qty: f64,
    package_count: i64,
}

fn warehouse_id(name: &str) -> String {
    format!("warehouse:{}", name.trim().to_lowercase())
}

fn role_as_str(role: &PrincipalRole) -> &'static str {
    match role {
        PrincipalRole::Supplier => "supplier",
        PrincipalRole::Werka => "werka",
        PrincipalRole::Customer => "customer",
        PrincipalRole::Aparatchi => "aparatchi",
        PrincipalRole::Qolipchi => "qolipchi",
        PrincipalRole::Boyoqchi => "boyoqchi",
        PrincipalRole::MaterialTaminotchi => "material_taminotchi",
        PrincipalRole::Admin => "admin",
    }
}

fn role_from_str(raw: &str) -> Result<PrincipalRole, WarehouseError> {
    match raw.trim().to_lowercase().as_str() {
        "supplier" => Ok(PrincipalRole::Supplier),
        "werka" => Ok(PrincipalRole::Werka),
        "customer" => Ok(PrincipalRole::Customer),
        "aparatchi" => Ok(PrincipalRole::Aparatchi),
        "qolipchi" => Ok(PrincipalRole::Qolipchi),
        "boyoqchi" => Ok(PrincipalRole::Boyoqchi),
        "material_taminotchi" => Ok(PrincipalRole::MaterialTaminotchi),
        "admin" => Ok(PrincipalRole::Admin),
        _ => Err(WarehouseError::StoreFailed),
    }
}

fn row_to_assignment(row: WarehouseAssignmentRow) -> Result<WarehouseAssignment, WarehouseError> {
    Ok(WarehouseAssignment {
        assignment_kind: row.assignment_kind,
        warehouse: row.warehouse,
        warehouse_name: row.warehouse_name,
        apparatus_id: row.apparatus_id,
        principal_role: role_from_str(&row.principal_role)?,
        principal_ref: row.principal_ref,
        display_name: row.display_name,
    })
}

fn row_to_summary(row: WarehouseSummaryRow) -> WarehouseSummary {
    WarehouseSummary {
        warehouse: row.warehouse,
        product_count: row.product_count.max(0) as usize,
        reserved_count: row.reserved_count.max(0) as usize,
        assignment_count: row.assignment_count.max(0) as usize,
        assigned_display_names: row
            .assigned_display_names
            .lines()
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToString::to_string)
            .collect(),
    }
}

fn row_to_stock_item(row: WarehouseStockItemRow) -> WarehouseStockItem {
    WarehouseStockItem {
        code: row.code,
        name: row.name,
        uom: row.uom,
        warehouse: row.warehouse,
        item_group: row.item_group,
        on_hand_qty: row.on_hand_qty.max(0.0),
        package_count: row.package_count.max(0) as usize,
    }
}
