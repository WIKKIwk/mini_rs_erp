use crate::core::auth::models::Principal;
use crate::core::preparation::*;
use crate::core::werka::models::SupplierItem;
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::collections::BTreeMap;

mod warehouses;
mod materials;
mod material_stock;
mod formula_orders;
mod formula_catalog;
pub(crate) mod formula_live;
mod formula_sets;
mod saved_formulas;
mod formula_shares;
mod formula_variants;
mod formula_revisions;
mod order_formula_contents;
mod formula_variant_reuse;
mod formula_variant_orders;
mod formula_print_snapshot;
mod production_journal;
pub(crate) use production_journal::MAX_JOURNAL_WINDOW;
mod production_notes;
mod output_notes;
pub(crate) mod history;
mod history_documents;
pub(crate) mod orders;
mod order_customers;

impl From<sqlx::Error> for PreparationError {
    fn from(error: sqlx::Error) -> Self {
        tracing::error!(%error, "preparation storage operation failed");
        PreparationError::StoreFailed
    }
}

// Reuse the shared stock and physical-location authority. A lot can be
// accounted to this warehouse but physically at an apparatus or in transit.
macro_rules! physically_in_warehouse {
    () => { "btrim(COALESCE(s.payload_json->>'inventory_transfer_id', '')) = ''
    AND NOT EXISTS (
        SELECT 1 FROM mini_inventory_placements p
        JOIN mini_inventory_locations l ON l.id = p.physical_location_id
        LEFT JOIN mini_warehouses w ON w.id = l.warehouse_id
        WHERE p.asset_kind = 'raw_material' AND lower(p.asset_ref) = lower(s.id)
          AND (l.kind <> 'warehouse' OR w.name IS NULL OR lower(w.name) <> lower(s.warehouse)))" };
}

macro_rules! free_in_warehouse {
    () => { concat!("s.reserved_order_id = ''
    AND NOT EXISTS (SELECT 1 FROM mini_raw_material_assignments a WHERE lower(a.barcode) = lower(s.barcode))
    AND ", physically_in_warehouse!()) };
}

const AVAILABLE_STOCK: &str = concat!("s.status = 'available' AND ", free_in_warehouse!());
const PADDON_STOCK: &str = concat!("s.status = 'on_paddon' AND ", free_in_warehouse!());

pub mod paddons;

const PREPARATION_ITEM_GROUP: &str = "Homashyo";
const PREPARATION_MATERIAL_CHILD_GROUP: &str = "seriyo";

pub(super) const SERIYO_GROUPS_SQL: &str = "seriyo_groups AS (
    SELECT name FROM mini_item_groups WHERE lower(btrim(name)) = 'seriyo' AND is_group
    UNION
    SELECT child.name FROM mini_item_groups child JOIN seriyo_groups parent
      ON lower(btrim(child.parent_item_group)) = lower(btrim(parent.name))
    WHERE child.is_group
)";

pub(super) const RULON_GROUPS_SQL: &str = "rulon_groups AS (
    SELECT name FROM mini_item_groups
    WHERE lower(btrim(name)) IN ('rulon', 'rulon materiallari') AND is_group
    UNION
    SELECT child.name FROM mini_item_groups child JOIN rulon_groups parent
      ON lower(btrim(child.parent_item_group)) = lower(btrim(parent.name))
    WHERE child.is_group
)";

// Prepared output and cataloged roll receipts both have an existing roll QR.
pub(super) const READY_ROLL_RECEIPT_SQL: &str = "
    receipt.status = 'submitted'
    AND btrim(receipt.barcode) <> ''
    AND btrim(COALESCE(receipt.payload_json->>'actor_ref', '')) <> ''
    AND NOT EXISTS (
        SELECT 1 FROM seriyo_groups
        WHERE lower(btrim(name)) = lower(btrim(item.item_group))
    )
    AND (
        receipt.payload_json->>'actor_role' = 'tayyorlov_masteri'
        OR EXISTS (
            SELECT 1 FROM rulon_groups
            WHERE lower(btrim(name)) = lower(btrim(item.item_group))
        )
    )";

// Share the receipt proof between the displayed balance and actual debits.
// The transfer evidence also covers lots received before the receipt was linked.
macro_rules! preparation_lot_for_master {
    ($master:literal, $is_seriyo:literal) => {
        concat!(
            "(r.owner_ref = ", $master, " OR EXISTS (
                SELECT 1 FROM mini_inventory_transfer_lines tl
                JOIN mini_inventory_transfers t ON t.id = tl.transfer_id
                JOIN mini_warehouses w ON w.id = t.destination_warehouse_id
                WHERE tl.asset_kind = 'raw_material' AND tl.asset_ref = s.id
                  AND t.status = 'received' AND lower(w.name) = lower(s.warehouse)
                  AND (s.payload_json->>'source' = 'raw_material_paddon'
                      OR (", $is_seriyo, " AND t.received_by_role = 'tayyorlov_masteri'
                          AND t.received_by_ref = ", $master, "))
            ))"
        )
    }
}

mod transfer_receipt_scope;
pub(super) mod transfer_receipts;

#[derive(Clone)]
pub struct PostgresPreparationStore {
    pool: PgPool,
}

include!("postgres_preparation_parts/stock_snapshot.rs");

include!("postgres_preparation_parts/create_material.rs");

include!("postgres_preparation_parts/consume.rs");

include!("postgres_preparation_parts/list_responsibilities.rs");

include!("postgres_preparation_parts/material_name_in_warehouse.rs");
