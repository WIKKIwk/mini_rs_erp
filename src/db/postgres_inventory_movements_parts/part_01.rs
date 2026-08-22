
#[derive(Clone)]
pub struct PostgresInventoryMovementStore {
    pool: PgPool,
}

impl PostgresInventoryMovementStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

include!("../postgres_inventory_movements_impl_parts/part_01.rs");
include!("../postgres_inventory_movements_impl_parts/part_02.rs");
include!("../postgres_inventory_movements_impl_parts/part_03.rs");

include!("../postgres_inventory_movements_trait_impl.rs");

const ASSET_LIST_SQL: &str = r#"
WITH assets AS (
    SELECT
        'raw_material'::text AS asset_kind,
        stock.id AS asset_ref,
        stock.warehouse,
        stock.item_code,
        COALESCE(NULLIF(btrim(stock.item_name), ''), stock.item_code) AS item_name,
        stock.barcode AS identifier,
        (stock.qty * 1000000)::bigint AS qty_units,
        stock.uom,
        CASE
            WHEN btrim(COALESCE(stock.payload_json->>'inventory_transfer_id', '')) <> ''
                THEN COALESCE(transfer.status, 'transfer_reserved')
            ELSE stock.status
        END AS status,
        COALESCE(stock.payload_json->>'inventory_transfer_id', '') AS transfer_id
    FROM mini_raw_material_stock stock
    LEFT JOIN mini_inventory_transfers transfer
      ON transfer.id = stock.payload_json->>'inventory_transfer_id'
    WHERE stock.qty > 0 AND stock.status <> 'consumed'

    UNION ALL

    SELECT
        'finished_goods'::text,
        stock.id,
        stock.warehouse,
        stock.item_code,
        COALESCE(NULLIF(btrim(stock.item_name), ''), stock.item_code),
        stock.id,
        (stock.qty * 1000000)::bigint,
        stock.uom,
        CASE
            WHEN btrim(COALESCE(stock.payload_json->>'inventory_transfer_id', '')) <> ''
                THEN COALESCE(transfer.status, 'transfer_reserved')
            ELSE stock.status
        END,
        COALESCE(stock.payload_json->>'inventory_transfer_id', '')
    FROM mini_finished_goods_stock stock
    LEFT JOIN mini_inventory_transfers transfer
      ON transfer.id = stock.payload_json->>'inventory_transfer_id'
    WHERE stock.qty > 0 AND stock.status <> 'dispatched'

    UNION ALL

    SELECT
        'qolip'::text,
        stock.id,
        stock.warehouse,
        stock.item_code,
        stock.item_name,
        stock.qolip_code,
        stock.quantity::bigint * 1000000,
        'dona'::text,
        CASE
            WHEN btrim(stock.inventory_transfer_id) = '' THEN 'available'
            ELSE COALESCE(transfer.status, 'transfer_reserved')
        END,
        stock.inventory_transfer_id
    FROM mini_qolip_locations stock
    LEFT JOIN mini_inventory_transfers transfer
      ON transfer.id = stock.inventory_transfer_id
    WHERE stock.quantity > 0
)
SELECT
    assets.asset_kind,
    assets.asset_ref,
    warehouse.id AS custody_warehouse_id,
    assets.warehouse AS custody_warehouse,
    assets.item_code,
    assets.item_name,
    assets.identifier,
    assets.qty_units,
    assets.uom,
    assets.status,
    location.id AS physical_location_id,
    location.kind AS physical_location_kind,
    location.name AS physical_location_name,
    assets.transfer_id,
    COALESCE(placement.version, 1)::bigint AS placement_version
FROM assets
JOIN mini_warehouses warehouse
  ON lower(warehouse.name) = lower(assets.warehouse)
JOIN mini_inventory_locations warehouse_location
  ON warehouse_location.warehouse_id = warehouse.id
LEFT JOIN mini_inventory_placements placement
  ON placement.asset_kind = assets.asset_kind
 AND lower(placement.asset_ref) = lower(assets.asset_ref)
JOIN mini_inventory_locations location
  ON location.id = COALESCE(placement.physical_location_id, warehouse_location.id)
WHERE ($1 = true OR lower(assets.warehouse) = ANY($2))
  AND (
        $3 = ''
        OR (
            location.kind = 'warehouse'
            AND location.warehouse_id = $3
        )
  )
  AND ($4 = '' OR assets.asset_kind = $4)
  AND (
        $5 = ''
        OR lower(assets.item_code) LIKE $6
        OR lower(assets.item_name) LIKE $6
        OR lower(assets.identifier) LIKE $6
        OR lower(assets.asset_ref) LIKE $6
  )
  AND (
        $9 = false
        OR (
            location.kind = 'state'
            AND placement.updated_by_ref = $10
        )
  )
ORDER BY lower(assets.item_name), lower(assets.identifier), assets.asset_ref
LIMIT $7 OFFSET $8
"#;

#[derive(sqlx::FromRow)]
struct InventoryLocationRow {
    id: String,
    kind: String,
    name: String,
    warehouse_id: String,
    factory_location_id: String,
    active: bool,
    apparatus_json: Value,
}

#[derive(sqlx::FromRow)]
struct RawMaterialStatePlacementRow {
    barcode: String,
    location_id: String,
    location_name: String,
    apparatus_json: Value,
}

#[derive(sqlx::FromRow)]
struct InventoryAssetRow {
    asset_kind: String,
    asset_ref: String,
    custody_warehouse_id: String,
    custody_warehouse: String,
    item_code: String,
    item_name: String,
    identifier: String,
    qty_units: i64,
    uom: String,
    status: String,
    physical_location_id: String,
    physical_location_kind: String,
    physical_location_name: String,
    transfer_id: String,
    placement_version: i64,
}

#[derive(sqlx::FromRow)]
struct AssetLockRow {
    asset_kind: String,
    asset_ref: String,
    warehouse_id: String,
    warehouse: String,
    item_code: String,
    item_name: String,
    identifier: String,
    qty_units: i64,
    uom: String,
    status: String,
    transfer_id: String,
    physical_location_id: String,
}

#[derive(sqlx::FromRow)]
struct WarehouseLookupRow {
    id: String,
    name: String,
    is_group: bool,
    parent_warehouse: String,
    assignment_count: i64,
}

#[derive(sqlx::FromRow, Clone)]
struct InventoryTransferRow {
    id: String,
    source_warehouse_id: String,
    source_warehouse: String,
    destination_warehouse_id: String,
    destination_warehouse: String,
    status: String,
    note: String,
    requested_by_name: String,
    approved_by_name: String,
    dispatched_by_name: String,
    received_by_name: String,
    rejected_by_name: String,
    cancelled_by_name: String,
    created_at_unix: i64,
    approved_at_unix: Option<i64>,
    dispatched_at_unix: Option<i64>,
    received_at_unix: Option<i64>,
    rejected_at_unix: Option<i64>,
    cancelled_at_unix: Option<i64>,
}

#[derive(sqlx::FromRow, Clone)]
struct InventoryTransferLineRow {
    transfer_id: String,
    asset_kind: String,
    asset_ref: String,
    item_code: String,
    item_name: String,
    identifier: String,
    qty_units: i64,
    uom: String,
    source_physical_location_id: String,
}

#[derive(sqlx::FromRow)]
struct MovementIdentityRow {
    event_type: String,
    asset_kind: String,
    asset_ref: String,
    to_location_id: String,
}

#[derive(sqlx::FromRow)]
struct TransferActionIdentityRow {
    transfer_id: String,
    action: String,
}

fn location_from_row(
    row: InventoryLocationRow,
) -> Result<InventoryLocation, InventoryMovementError> {
    let apparatus = serde_json::from_value::<Vec<InventoryLocationApparatus>>(row.apparatus_json)
        .map_err(|_| InventoryMovementError::StoreFailed)?;
    Ok(InventoryLocation {
        id: row.id,
        kind: InventoryLocationKind::parse(&row.kind)?,
        name: row.name,
        warehouse_id: row.warehouse_id,
        factory_location_id: row.factory_location_id,
        active: row.active,
        apparatus,
    })
}

fn asset_from_row(row: InventoryAssetRow) -> Result<InventoryAsset, InventoryMovementError> {
    Ok(InventoryAsset {
        kind: InventoryAssetKind::parse(&row.asset_kind)?,
        asset_ref: row.asset_ref,
        custody_warehouse_id: row.custody_warehouse_id,
        custody_warehouse: row.custody_warehouse,
        item_code: row.item_code,
        item_name: row.item_name,
        identifier: row.identifier,
        qty: erp_quantity_from_units(row.qty_units),
        uom: row.uom,
        status: row.status,
        physical_location: InventoryLocationRef {
            id: row.physical_location_id,
            kind: InventoryLocationKind::parse(&row.physical_location_kind)?,
            name: row.physical_location_name,
        },
        transfer_id: row.transfer_id,
        placement_version: row.placement_version,
    })
}

async fn fetch_asset(
    pool: &PgPool,
    kind: InventoryAssetKind,
    asset_ref: &str,
) -> Result<InventoryAsset, InventoryMovementError> {
    let rows = sqlx::query_as::<_, InventoryAssetRow>(ASSET_LIST_SQL)
        .bind(true)
        .bind(Vec::<String>::new())
        .bind("")
        .bind(kind.as_str())
        .bind(asset_ref.trim().to_ascii_lowercase())
        .bind(format!("%{}%", asset_ref.trim().to_ascii_lowercase()))
        .bind(50_i64)
        .bind(0_i64)
        .bind(false)
        .bind("")
        .fetch_all(pool)
        .await
        .map_err(store_error)?;
    rows.into_iter()
        .find(|row| row.asset_ref.eq_ignore_ascii_case(asset_ref))
        .ok_or(InventoryMovementError::AssetNotFound)
        .and_then(asset_from_row)
}

async fn lock_asset_tx(
    tx: &mut Transaction<'_, Postgres>,
    kind: InventoryAssetKind,
    asset_ref: &str,
) -> Result<AssetLockRow, InventoryMovementError> {
    let row = match kind {
        InventoryAssetKind::RawMaterial => {
            sqlx::query_as::<_, AssetLockRow>(
                r#"
                SELECT
                    'raw_material'::text AS asset_kind,
                    stock.id AS asset_ref,
                    warehouse.id AS warehouse_id,
                    stock.warehouse,
                    stock.item_code,
                    COALESCE(NULLIF(btrim(stock.item_name), ''), stock.item_code) AS item_name,
                    stock.barcode AS identifier,
                    (stock.qty * 1000000)::bigint AS qty_units,
                    stock.uom,
                    stock.status,
                    COALESCE(stock.payload_json->>'inventory_transfer_id', '') AS transfer_id,
                    COALESCE(
                        placement.physical_location_id,
                        warehouse_location.id
                    ) AS physical_location_id
                FROM mini_raw_material_stock stock
                JOIN mini_warehouses warehouse
                  ON lower(warehouse.name) = lower(stock.warehouse)
                JOIN mini_inventory_locations warehouse_location
                  ON warehouse_location.warehouse_id = warehouse.id
                LEFT JOIN mini_inventory_placements placement
                  ON placement.asset_kind = 'raw_material'
                 AND lower(placement.asset_ref) = lower(stock.id)
                WHERE lower(stock.id) = lower($1)
                FOR UPDATE OF stock
                "#,
            )
            .bind(asset_ref.trim())
            .fetch_optional(&mut **tx)
            .await
        }
        InventoryAssetKind::FinishedGoods => {
            sqlx::query_as::<_, AssetLockRow>(
                r#"
                SELECT
                    'finished_goods'::text AS asset_kind,
                    stock.id AS asset_ref,
                    warehouse.id AS warehouse_id,
                    stock.warehouse,
                    stock.item_code,
                    COALESCE(NULLIF(btrim(stock.item_name), ''), stock.item_code) AS item_name,
                    stock.id AS identifier,
                    (stock.qty * 1000000)::bigint AS qty_units,
                    stock.uom,
                    stock.status,
                    COALESCE(stock.payload_json->>'inventory_transfer_id', '') AS transfer_id,
                    COALESCE(
                        placement.physical_location_id,
                        warehouse_location.id
                    ) AS physical_location_id
                FROM mini_finished_goods_stock stock
                JOIN mini_warehouses warehouse
                  ON lower(warehouse.name) = lower(stock.warehouse)
                JOIN mini_inventory_locations warehouse_location
                  ON warehouse_location.warehouse_id = warehouse.id
                LEFT JOIN mini_inventory_placements placement
                  ON placement.asset_kind = 'finished_goods'
                 AND lower(placement.asset_ref) = lower(stock.id)
                WHERE lower(stock.id) = lower($1)
                FOR UPDATE OF stock
                "#,
            )
            .bind(asset_ref.trim())
            .fetch_optional(&mut **tx)
            .await
        }
        InventoryAssetKind::Qolip => {
            sqlx::query_as::<_, AssetLockRow>(
                r#"
                SELECT
                    'qolip'::text AS asset_kind,
                    stock.id AS asset_ref,
                    warehouse.id AS warehouse_id,
                    stock.warehouse,
                    stock.item_code,
                    stock.item_name,
                    stock.qolip_code AS identifier,
                    stock.quantity::bigint * 1000000 AS qty_units,
                    'dona'::text AS uom,
                    CASE
                        WHEN btrim(stock.inventory_transfer_id) = '' THEN 'available'
                        ELSE 'transfer_reserved'
                    END AS status,
                    stock.inventory_transfer_id AS transfer_id,
                    COALESCE(
                        placement.physical_location_id,
                        warehouse_location.id
                    ) AS physical_location_id
                FROM mini_qolip_locations stock
                JOIN mini_warehouses warehouse
                  ON lower(warehouse.name) = lower(stock.warehouse)
                JOIN mini_inventory_locations warehouse_location
                  ON warehouse_location.warehouse_id = warehouse.id
                LEFT JOIN mini_inventory_placements placement
                  ON placement.asset_kind = 'qolip'
                 AND lower(placement.asset_ref) = lower(stock.id)
                WHERE lower(stock.id) = lower($1)
                FOR UPDATE OF stock
                "#,
            )
            .bind(asset_ref.trim())
            .fetch_optional(&mut **tx)
            .await
        }
    }
    .map_err(store_error)?;
    row.ok_or(InventoryMovementError::AssetNotFound)
}

fn ensure_asset_available(asset: &AssetLockRow) -> Result<(), InventoryMovementError> {
    if !asset.transfer_id.trim().is_empty() || asset.status != "available" || asset.qty_units <= 0 {
        Err(InventoryMovementError::AssetUnavailable)
    } else {
        Ok(())
    }
}
