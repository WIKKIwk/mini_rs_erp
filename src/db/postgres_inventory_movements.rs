use std::collections::BTreeMap;

use async_trait::async_trait;
use data_encoding::HEXLOWER;
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};

use crate::core::apparatus_standard::ApparatusId;
use crate::core::inventory_movements::{
    InventoryActor, InventoryAsset, InventoryAssetKind, InventoryAssetQuery, InventoryLocation,
    InventoryLocationApparatus, InventoryLocationKind, InventoryLocationRef,
    InventoryMovementError, InventoryMovementStorePort, InventoryRelocationBatchCreate,
    InventoryRelocationCreate, InventoryReturnBatchCreate, InventoryTransfer,
    InventoryTransferAction, InventoryTransferActionKind, InventoryTransferCreate,
    InventoryTransferLine, InventoryTransferQuery, InventoryTransferStatus,
    RawMaterialStatePlacement, inventory_role_code,
};
use crate::core::quantity::erp_quantity_from_units;
use crate::db::postgres_raw_material_events::{
    RawMaterialEventDraft, insert_raw_material_event_tx,
};

include!("postgres_inventory_movements_parts/part_01.rs");
include!("postgres_inventory_movements_parts/part_02.rs");
include!("postgres_inventory_movements_parts/part_03.rs");
include!("postgres_inventory_movements_parts/part_04.rs");
