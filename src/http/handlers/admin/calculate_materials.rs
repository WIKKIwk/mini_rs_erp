use super::*;
use crate::core::admin::models::AdminItemGroup;
use crate::core::admin::ports::AdminPortError;
use crate::core::calculate_materials::{
    CalculateMaterial, CalculateMaterialError, CalculateMaterialUpsert, ensure_unique_name,
    normalize_material,
};
use crate::core::werka::models::SupplierItem;
use crate::http::handlers::material_catalog::ROLL_MATERIAL_ITEM_GROUP;

const RAW_MATERIAL_ITEM_GROUP: &str = "Homashyo";

pub async fn calculate_materials(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    match method {
        Method::GET => {
            let materials = state
                .calculate_materials
                .list()
                .await
                .map_err(calculate_material_store_error)?;
            Ok(json_response(serde_json::json!({
                "ok": true,
                "materials": materials,
            })))
        }
        Method::PUT => {
            require_capability(&state, &principal, Capability::AdminAccess).await?;
            let input: CalculateMaterialUpsert = parse_json(&body)?;
            let material = normalize_material(input).map_err(calculate_material_store_error)?;
            let current = state
                .calculate_materials
                .list()
                .await
                .map_err(calculate_material_store_error)?;
            ensure_unique_name(&current, &material).map_err(calculate_material_store_error)?;
            sync_catalog_item(&state, &material, &current)
                .await
                .map_err(calculate_material_catalog_error)?;
            let material = state
                .calculate_materials
                .upsert(CalculateMaterialUpsert {
                    id: material.id,
                    name: material.name,
                    active: material.active,
                    density_g_cm3: material.density_g_cm3,
                    variants: material.variants,
                })
                .await
                .map_err(calculate_material_store_error)?;
            Ok(json_response(serde_json::json!({
                "ok": true,
                "material": material,
            })))
        }
        _ => Err(method_not_allowed()),
    }
}

async fn sync_catalog_item(
    state: &AppState,
    material: &CalculateMaterial,
    current_materials: &[CalculateMaterial],
) -> Result<(), AdminPortError> {
    let rulon_group = ensure_roll_material_group(state).await?;
    let previous_name = current_materials
        .iter()
        .find(|current| current.id == material.id)
        .map(|current| current.name.trim())
        .filter(|name| !name.is_empty());
    let mut candidate_codes = vec![material.name.clone()];
    if let Some(previous_name) = previous_name
        && !previous_name.eq_ignore_ascii_case(&material.name)
    {
        candidate_codes.push(previous_name.to_string());
    }
    let mut existing = state.admin.items_by_codes(&candidate_codes).await?;
    if existing.is_empty() {
        for candidate_name in std::iter::once(material.name.as_str()).chain(previous_name) {
            let matching = state
                .admin
                .items_page_by_group("", candidate_name, 100, 0)
                .await?
                .into_iter()
                .find(|item| item.name.trim().eq_ignore_ascii_case(candidate_name));
            if let Some(item) = matching {
                existing.push(item);
                break;
            }
        }
    }
    let item_index = existing
        .iter()
        .position(|item| item.code.trim().eq_ignore_ascii_case(&material.name))
        .or((!existing.is_empty()).then_some(0));
    let item = item_index.map(|index| &mut existing[index]);

    if let Some(item) = item {
        sync_existing_catalog_item(state, item, material, &rulon_group).await
    } else {
        match state
            .admin
            .create_item(
                &material.name,
                &material.name,
                "Kg",
                &rulon_group,
                "",
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(AdminPortError::InvalidInput(error)) if error == "item code already exists" => {
                let mut items = state
                    .admin
                    .items_by_codes(std::slice::from_ref(&material.name))
                    .await?;
                let item = items.first_mut().ok_or(AdminPortError::LookupFailed)?;
                sync_existing_catalog_item(state, item, material, &rulon_group).await
            }
            Err(error) => Err(error),
        }
    }
}

async fn sync_existing_catalog_item(
    state: &AppState,
    item: &mut SupplierItem,
    material: &CalculateMaterial,
    rulon_group: &str,
) -> Result<(), AdminPortError> {
    if item.name.trim() != material.name.trim() {
        let detail = state
            .admin
            .update_item(&item.code, &item.code, &material.name)
            .await?;
        item.code = detail.code;
        item.name = detail.name;
        item.item_group = detail.item_group;
    }
    if !item.item_group.trim().eq_ignore_ascii_case(rulon_group) {
        let moved = state
            .admin
            .move_items_to_group(vec![item.code.clone()], rulon_group)
            .await?;
        if moved.updated_count != 1 {
            return Err(AdminPortError::NotFound);
        }
    }
    Ok(())
}

async fn ensure_roll_material_group(state: &AppState) -> Result<String, AdminPortError> {
    let mut groups = state.admin.item_group_tree().await?;
    let raw_group = find_group(&groups, &["homashyo", "xomashyo"])
        .map(group_name)
        .map(str::to_string)
        .unwrap_or_else(|| RAW_MATERIAL_ITEM_GROUP.to_string());
    if find_group(&groups, &["homashyo", "xomashyo"]).is_none() {
        let created = state
            .admin
            .create_item_group(&raw_group, "All Item Groups", true)
            .await?;
        groups.push(created);
    }

    let roll_group = find_group(&groups, &["rulon", "rulon materiallari"]);
    match roll_group {
        Some(group) => {
            let name = group_name(group).to_string();
            if !group
                .parent_item_group
                .trim()
                .eq_ignore_ascii_case(&raw_group)
            {
                state
                    .admin
                    .move_item_group_parent(&name, &raw_group)
                    .await?;
            }
            Ok(name)
        }
        None => state
            .admin
            .create_item_group(ROLL_MATERIAL_ITEM_GROUP, &raw_group, true)
            .await
            .map(|group| group_name(&group).to_string()),
    }
}

fn find_group<'a>(groups: &'a [AdminItemGroup], names: &[&str]) -> Option<&'a AdminItemGroup> {
    groups.iter().find(|group| {
        names
            .iter()
            .any(|name| group_name(group).eq_ignore_ascii_case(name))
    })
}

fn group_name(group: &AdminItemGroup) -> &str {
    let item_group_name = group.item_group_name.trim();
    if item_group_name.is_empty() {
        group.name.trim()
    } else {
        item_group_name
    }
}

fn calculate_material_store_error(error: CalculateMaterialError) -> AdminError {
    match error {
        CalculateMaterialError::InvalidInput(message) => bad_request(message),
        CalculateMaterialError::StoreFailed => server_error("calculate materials store failed"),
    }
}

fn calculate_material_catalog_error(_error: AdminPortError) -> AdminError {
    server_error("calculate material catalog sync failed")
}
