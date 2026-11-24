fn assigned_items(
    items: &BTreeMap<String, StoredSupplierItem>,
    item_codes: &[String],
    query: &str,
    limit: usize,
) -> Vec<SupplierItem> {
    let query = query.trim().to_lowercase();
    paginate(
        item_codes
            .iter()
            .filter_map(|code| items.get(code.trim()))
            .filter(|item| query.is_empty() || item_matches(item, &query))
            .map(SupplierItem::from)
            .collect(),
        limit,
        0,
    )
}
