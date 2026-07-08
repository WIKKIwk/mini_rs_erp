use super::*;
use crate::core::admin::models::AdminItemGroup;

pub(super) async fn material_scoped_items(
    state: &AppState,
    principal: &Principal,
    query: &ItemQuery,
) -> Result<Vec<SupplierItem>, AdminError> {
    let search = query.q.as_deref().unwrap_or("");
    let requested_group = query.group.as_deref().unwrap_or("").trim();
    let limit = positive_int(query.limit.as_deref(), 50).min(200);
    let offset = optional_offset(query.offset.as_deref());
    let scoped_groups = state
        .admin
        .principal_assigned_item_group_scope(principal)
        .await
        .map_err(|_| server_error("item group scope fetch failed"))?;
    if scoped_groups.is_empty() {
        return Ok(Vec::new());
    }
    let groups = if requested_group.is_empty() {
        scoped_groups
    } else {
        let requested_scope = state
            .admin
            .item_group_scope(vec![requested_group.to_string()])
            .await
            .map_err(|_| server_error("item group scope fetch failed"))?;
        scoped_groups
            .into_iter()
            .filter(|group| {
                requested_scope
                    .iter()
                    .any(|requested| requested.trim().eq_ignore_ascii_case(group.trim()))
            })
            .collect()
    };
    if groups.is_empty() {
        return Ok(Vec::new());
    }

    let mut items = Vec::new();
    let mut seen_codes = std::collections::BTreeSet::new();
    for group in groups {
        let group_items = state
            .admin
            .items_page_by_group(&group, search, limit, 0)
            .await
            .map_err(|_| server_error("admin items failed"))?;
        for item in group_items {
            let key = item.code.trim().to_lowercase();
            if key.is_empty() || seen_codes.insert(key) {
                items.push(item);
            }
        }
    }
    Ok(items.into_iter().skip(offset).take(limit).collect())
}

pub(super) async fn scoped_item_group_tree(
    state: &AppState,
    principal: &Principal,
    groups: Vec<AdminItemGroup>,
) -> Result<Vec<AdminItemGroup>, AdminError> {
    let scoped_groups = state
        .admin
        .principal_assigned_item_group_scope(principal)
        .await
        .map_err(|_| server_error("item group scope fetch failed"))?;
    if scoped_groups.is_empty() {
        return Ok(Vec::new());
    }
    let by_name = groups
        .iter()
        .map(|group| (group.item_group_name.trim().to_lowercase(), group.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut include = std::collections::BTreeSet::new();
    for group in scoped_groups {
        let mut current = group.trim().to_lowercase();
        while !current.is_empty() && include.insert(current.clone()) {
            let Some(entry) = by_name.get(&current) else {
                break;
            };
            current = entry.parent_item_group.trim().to_lowercase();
        }
    }
    Ok(groups
        .into_iter()
        .filter(|group| include.contains(&group.item_group_name.trim().to_lowercase()))
        .collect())
}
