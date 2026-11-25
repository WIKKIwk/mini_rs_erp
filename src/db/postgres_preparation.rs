use crate::core::auth::models::Principal;
use crate::core::preparation::*;
use crate::core::werka::models::SupplierItem;
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Row, Transaction};

mod warehouses;
mod materials;
mod material_stock;
mod formula_orders;
mod formula_catalog;
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

#[derive(Clone)]
pub struct PostgresPreparationStore {
    pool: PgPool,
}

include!("postgres_preparation_parts/stock_snapshot.rs");

include!("postgres_preparation_parts/create_material.rs");

include!("postgres_preparation_parts/consume.rs");

include!("postgres_preparation_parts/list_responsibilities.rs");

include!("postgres_preparation_parts/material_name_in_warehouse.rs");
