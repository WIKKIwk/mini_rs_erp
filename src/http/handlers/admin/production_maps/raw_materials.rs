use super::raw_material_details::{
    apparatus_id_matches_text, apparatus_id_matches_text_value, assigned_apparatus_contains,
    fill_raw_material_assignment_input, item_group_path, lookup_raw_material_detail,
    raw_material_rulon_match_metrics, require_material_item_group_scope,
    require_material_warehouse_scope, resolve_raw_material_stock_item,
    roll_width_allowance_mm,
    validate_rulon_size_for_apparatus_map,
};
use super::*;
use crate::core::apparatus_standard::{MaterialExecutionPolicy, ToolingExecutionPolicy};
use crate::core::inventory_movements::RawMaterialStatePlacement;
use crate::core::gscale::models::RawMaterialStockEntry;
use crate::core::werka::models::SupplierItem;
use crate::db::postgres_raw_material_events::{
    RawMaterialEventDraft, RawMaterialEventQuery, RawMaterialEventScope,
};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

include!("raw_materials_parts/part_01.rs");
include!("raw_materials_parts/part_02.rs");
include!("raw_materials_parts/part_03.rs");
include!("raw_materials_parts/part_04.rs");
