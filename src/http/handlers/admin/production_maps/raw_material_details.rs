use super::*;
use crate::core::admin::models::AdminItemGroup;
use crate::core::gscale::models::RawMaterialStockEntry;
use crate::core::production_map::{ProductionMapDefinition, chain, pechat};
use crate::core::werka::models::SupplierItem;

#[derive(serde::Serialize)]
pub(super) struct RawMaterialLookupResponse {
    barcode: String,
    warehouse: String,
    item_code: String,
    item_name: String,
    item_group: String,
    qty: f64,
    uom: String,
    status: String,
    reserved_order_id: String,
    source_receipt_id: String,
}

pub(super) async fn fill_raw_material_assignment_input(
    state: &AppState,
    principal: &Principal,
    mut input: RawMaterialAssignmentInput,
) -> Result<(RawMaterialAssignmentInput, String), AdminError> {
    let barcode = input.barcode.trim();
    if barcode.is_empty() {
        return Err(production_map_error(
            ProductionMapError::RawMaterialInvalidInput,
        ));
    }
    if input.order_id.trim().is_empty() {
        return Err(production_map_error(
            ProductionMapError::RawMaterialInvalidInput,
        ));
    }
    let (stock, item) = resolve_raw_material_stock_item(state, barcode).await?;
    let item_code = stock.item_code.trim().to_string();
    if item_code.is_empty() {
        return Err(production_map_error(
            ProductionMapError::RawMaterialInvalidInput,
        ));
    }
    let groups = state
        .admin
        .item_group_tree()
        .await
        .map_err(|_| server_error("item group tree fetch failed"))?;
    let item_group_path = item_group_path(&groups, &item.item_group);
    validate_rulon_size_for_pechat_order(state, &input.order_id, &stock, &item, &item_group_path)
        .await?;
    input.item_code = item_code;
    input.item_name = item.name.trim().to_string();
    input.item_group = item.item_group.trim().to_string();
    input.item_group_path = item_group_path;
    require_material_item_group_scope(state, principal, &input.item_group).await?;
    require_material_warehouse_scope(state, principal, &stock.warehouse).await?;
    Ok((input, stock.warehouse.trim().to_string()))
}

async fn require_material_item_group_scope(
    state: &AppState,
    principal: &Principal,
    item_group: &str,
) -> Result<(), AdminError> {
    if principal.role != PrincipalRole::MaterialTaminotchi {
        return Ok(());
    }
    let item_group = item_group.trim();
    let assigned_groups = state
        .admin
        .principal_assigned_item_group_scope(principal)
        .await
        .map_err(|_| server_error("item group scope fetch failed"))?;
    if !item_group.is_empty()
        && assigned_groups
            .iter()
            .any(|group| group.trim().eq_ignore_ascii_case(item_group))
    {
        return Ok(());
    }
    Err(bad_request(
        "item group is not assigned to material taminotchi",
    ))
}

async fn require_material_warehouse_scope(
    state: &AppState,
    principal: &Principal,
    warehouse: &str,
) -> Result<(), AdminError> {
    if principal.role != PrincipalRole::MaterialTaminotchi {
        return Ok(());
    }
    let warehouse = warehouse.trim();
    if warehouse.is_empty() {
        return Err(bad_request(
            "warehouse is not assigned to material taminotchi",
        ));
    }
    let assigned = state
        .warehouses
        .assigned_warehouse_names(principal)
        .await
        .map_err(warehouse_error)?;
    if assigned
        .iter()
        .any(|assigned| assigned.trim().eq_ignore_ascii_case(warehouse))
    {
        return Ok(());
    }
    Err(bad_request(
        "warehouse is not assigned to material taminotchi",
    ))
}

async fn validate_rulon_size_for_pechat_order(
    state: &AppState,
    order_id: &str,
    stock: &RawMaterialStockEntry,
    item: &SupplierItem,
    item_group_path: &[String],
) -> Result<(), AdminError> {
    if !is_rulon_group(item_group_path) {
        return Ok(());
    }
    let map = state
        .production_maps
        .raw_map(order_id)
        .await
        .map_err(production_map_error)?
        .ok_or_else(|| production_map_error(ProductionMapError::MapNotFound))?;
    if !map_has_pechat_stage(&map) {
        return Ok(());
    }
    let order_width = map
        .width_mm
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| production_map_error(ProductionMapError::RawMaterialRollSizeMissing))?;
    let roll_width = roll_width_mm(stock, item)
        .ok_or_else(|| production_map_error(ProductionMapError::RawMaterialRollSizeMissing))?;
    if roll_width + f64::EPSILON < order_width || roll_width > order_width + 20.0 + f64::EPSILON {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AdminErrorResponse::roll_size_mismatch(
                order_width,
                roll_width,
            )),
        ));
    }
    Ok(())
}

fn item_group_path(groups: &[AdminItemGroup], item_group: &str) -> Vec<String> {
    let mut path = Vec::new();
    let mut current = item_group.trim().to_string();
    let mut seen = std::collections::BTreeSet::new();
    while !current.is_empty() && seen.insert(current.to_lowercase()) {
        path.push(current.clone());
        let Some(group) = groups
            .iter()
            .find(|group| group.item_group_name.trim().eq_ignore_ascii_case(&current))
        else {
            break;
        };
        current = group.parent_item_group.trim().to_string();
    }
    path
}

fn is_rulon_group(item_group_path: &[String]) -> bool {
    item_group_path
        .iter()
        .any(|group| group.trim().eq_ignore_ascii_case("Rulon"))
}

fn map_has_pechat_stage(map: &ProductionMapDefinition) -> bool {
    chain::linear_work_stages(map)
        .iter()
        .any(|stage| pechat::pechat_color_count(&stage.station_title).is_some())
}

fn roll_width_mm(stock: &RawMaterialStockEntry, item: &SupplierItem) -> Option<f64> {
    [
        stock.item_code.as_str(),
        stock.item_name.as_str(),
        item.code.as_str(),
        item.name.as_str(),
    ]
    .into_iter()
    .find_map(roll_width_from_text)
}

fn roll_width_from_text(value: &str) -> Option<f64> {
    let bytes = value.as_bytes();
    for slash_index in bytes.iter().position(|byte| *byte == b'/')?..bytes.len() {
        if bytes[slash_index] != b'/' {
            continue;
        }
        let mut end = slash_index;
        while end > 0 && bytes[end - 1].is_ascii_whitespace() {
            end -= 1;
        }
        let mut start = end;
        while start > 0 && bytes[start - 1].is_ascii_digit() {
            start -= 1;
        }
        if start == end {
            continue;
        }
        if let Ok(width) = value[start..end].parse::<f64>() {
            return Some(width);
        }
    }
    None
}

pub(super) async fn lookup_raw_material_detail(
    state: &AppState,
    principal: &Principal,
    barcode: &str,
) -> Result<RawMaterialLookupResponse, AdminError> {
    let (stock, item) = resolve_raw_material_stock_item(state, barcode).await?;
    require_material_item_group_scope(state, principal, &item.item_group).await?;
    Ok(RawMaterialLookupResponse {
        barcode: stock.barcode.trim().to_string(),
        warehouse: stock.warehouse.trim().to_string(),
        item_code: stock.item_code.trim().to_string(),
        item_name: item.name.trim().to_string(),
        item_group: item.item_group.trim().to_string(),
        qty: stock.qty,
        uom: stock.uom.trim().to_string(),
        status: stock.status.trim().to_string(),
        reserved_order_id: stock.reserved_order_id.trim().to_string(),
        source_receipt_id: stock.source_receipt_id.trim().to_string(),
    })
}

async fn resolve_raw_material_stock_item(
    state: &AppState,
    barcode: &str,
) -> Result<(RawMaterialStockEntry, SupplierItem), AdminError> {
    let barcode = barcode.trim();
    if barcode.is_empty() {
        return Err(production_map_error(
            ProductionMapError::RawMaterialInvalidInput,
        ));
    }
    let stock = state
        .gscale
        .raw_material_stock_by_barcode(barcode)
        .await
        .map_err(|_| production_map_error(ProductionMapError::RawMaterialInvalidInput))?
        .ok_or_else(|| production_map_error(ProductionMapError::RawMaterialInvalidInput))?;
    let item_code = stock.item_code.trim().to_string();
    if item_code.is_empty() {
        return Err(production_map_error(
            ProductionMapError::RawMaterialInvalidInput,
        ));
    }
    let items = state
        .admin
        .items_by_codes(std::slice::from_ref(&item_code))
        .await
        .map_err(|_| production_map_error(ProductionMapError::RawMaterialInvalidInput))?;
    let Some(item) = items
        .into_iter()
        .find(|item| item.code.trim().eq_ignore_ascii_case(&item_code))
    else {
        return Err(production_map_error(
            ProductionMapError::RawMaterialInvalidInput,
        ));
    };
    Ok((stock, item))
}
