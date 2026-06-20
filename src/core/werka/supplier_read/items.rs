use crate::core::werka::models::SupplierItem;

pub(super) fn filter_supplier_items_by_query(
    items: Vec<SupplierItem>,
    query: &str,
) -> Vec<SupplierItem> {
    let lower_query = query.trim().to_lowercase();
    if lower_query.is_empty() {
        return items;
    }

    items
        .into_iter()
        .filter(|item| {
            item.code.to_lowercase().contains(&lower_query)
                || item.name.to_lowercase().contains(&lower_query)
        })
        .collect()
}

pub(super) fn limit_supplier_items(
    mut items: Vec<SupplierItem>,
    limit: usize,
) -> Vec<SupplierItem> {
    if limit > 0 && items.len() > limit {
        items.truncate(limit);
    }
    items
}
