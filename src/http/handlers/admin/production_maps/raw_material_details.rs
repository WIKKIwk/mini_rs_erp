use super::*;
use crate::core::admin::models::AdminItemGroup;
use crate::core::gscale::models::RawMaterialStockEntry;
use crate::core::production_map::{ProductionMapDefinition, pechat};
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
    if !stock.status.trim().eq_ignore_ascii_case("available")
        || !stock.reserved_order_id.trim().is_empty()
    {
        return Err(production_map_error(
            ProductionMapError::RawMaterialStockUnavailable,
        ));
    }
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
    let map = state
        .production_maps
        .raw_map(&input.order_id)
        .await
        .map_err(production_map_error)?
        .ok_or_else(|| production_map_error(ProductionMapError::MapNotFound))?;
    let all_apparatus_options = state
        .production_maps
        .raw_material_assignment_apparatus_options(&input.order_id, &item_group_path)
        .await
        .map_err(production_map_error)?;
    let assigned_apparatus = if principal.role == PrincipalRole::MaterialTaminotchi {
        state.admin.principal_assigned_apparatus(principal).await
    } else {
        Vec::new()
    };
    let requested_apparatus = input.apparatus.trim();
    if principal.role == PrincipalRole::MaterialTaminotchi
        && !requested_apparatus.is_empty()
        && !queue_state::apparatus_matches_assigned(requested_apparatus, &assigned_apparatus)
    {
        return Err(production_map_error(
            ProductionMapError::ApparatusNotAssigned,
        ));
    }
    let apparatus_options = all_apparatus_options
        .iter()
        .filter(|apparatus| {
            principal.role != PrincipalRole::MaterialTaminotchi
                || queue_state::apparatus_matches_assigned(apparatus, &assigned_apparatus)
        })
        .cloned()
        .collect::<Vec<_>>();
    let apparatus = if requested_apparatus.is_empty() {
        match apparatus_options.as_slice() {
            [] if !all_apparatus_options.is_empty() => {
                return Err(production_map_error(
                    ProductionMapError::ApparatusNotAssigned,
                ));
            }
            [] => {
                return Err(production_map_error(
                    ProductionMapError::RawMaterialGroupNotAllowed,
                ));
            }
            [apparatus] => apparatus.clone(),
            _ => {
                return Err(production_map_error(
                    ProductionMapError::RawMaterialGroupAmbiguous(apparatus_options),
                ));
            }
        }
    } else {
        apparatus_options
            .iter()
            .find(|apparatus| queue_state::apparatus_titles_match(apparatus, requested_apparatus))
            .cloned()
            .ok_or_else(|| production_map_error(ProductionMapError::RawMaterialGroupNotAllowed))?
    };
    validate_rulon_size_for_apparatus_map(&map, &apparatus, &stock, &item, &item_group_path)?;
    input.item_code = item_code;
    input.item_name = item.name.trim().to_string();
    input.item_group = item.item_group.trim().to_string();
    input.item_group_path = item_group_path;
    input.apparatus = apparatus;
    require_material_item_group_scope(state, principal, &input.item_group).await?;
    require_material_warehouse_scope(state, principal, &stock.warehouse).await?;
    Ok((input, stock.warehouse.trim().to_string()))
}

pub(super) async fn require_material_item_group_scope(
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

pub(super) async fn require_material_warehouse_scope(
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

pub(super) fn validate_rulon_size_for_apparatus_map(
    map: &ProductionMapDefinition,
    apparatus: &str,
    stock: &RawMaterialStockEntry,
    item: &SupplierItem,
    item_group_path: &[String],
) -> Result<(), AdminError> {
    let Some(maximum_leftover_width_mm) = roll_width_allowance_mm(apparatus) else {
        return Ok(());
    };
    if !is_rulon_group(item_group_path) {
        return Ok(());
    }
    let order_width = map
        .width_mm
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| production_map_error(ProductionMapError::RawMaterialRollSizeMissing))?;
    let roll_width = roll_width_mm(stock, item)
        .ok_or_else(|| production_map_error(ProductionMapError::RawMaterialRollSizeMissing))?;
    if roll_width + f64::EPSILON < order_width
        || roll_width > order_width + maximum_leftover_width_mm + f64::EPSILON
    {
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

pub(super) fn raw_material_rulon_match_metrics(
    map: &ProductionMapDefinition,
    apparatus: &str,
    stock: &RawMaterialStockEntry,
    item: &SupplierItem,
    item_group_path: &[String],
) -> Option<(f64, f64, f64)> {
    if !is_rulon_group(item_group_path) || roll_width_allowance_mm(apparatus).is_none() {
        return None;
    }
    let order_width = map
        .width_mm
        .filter(|value| value.is_finite() && *value > 0.0)?;
    let roll_width = roll_width_mm(stock, item)?;
    let leftover_width = roll_width - order_width;
    Some((
        order_width,
        roll_width,
        if leftover_width.abs() <= 0.001 {
            0.0
        } else {
            leftover_width
        },
    ))
}

pub(super) fn item_group_path(groups: &[AdminItemGroup], item_group: &str) -> Vec<String> {
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

fn roll_width_allowance_mm(apparatus: &str) -> Option<f64> {
    if pechat::is_pechat_apparatus(apparatus) {
        return Some(20.0);
    }
    if apparatus.trim().to_lowercase().contains("laminatsiya") {
        return Some(30.0);
    }
    None
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

pub(super) async fn resolve_raw_material_stock_item(
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
