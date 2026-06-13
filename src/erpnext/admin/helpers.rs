use super::*;

#[derive(Debug, Deserialize)]
pub(super) struct ListResponse<T> {
    pub(super) data: Vec<T>,
}

#[derive(Debug, Deserialize)]
pub(super) struct GetResponse<T> {
    pub(super) data: T,
}

#[derive(Debug, Deserialize)]
pub(super) struct SupplierRow {
    pub(super) name: String,
    #[serde(default)]
    pub(super) supplier_name: String,
    #[serde(default)]
    pub(super) mobile_no: String,
    #[serde(default)]
    pub(super) supplier_details: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct CustomerRow {
    pub(super) name: String,
    #[serde(default)]
    pub(super) customer_name: String,
    #[serde(default)]
    pub(super) mobile_no: String,
    #[serde(default)]
    pub(super) customer_details: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ItemRow {
    pub(super) name: String,
    #[serde(default)]
    pub(super) item_name: String,
    #[serde(default)]
    pub(super) stock_uom: String,
    #[serde(default)]
    pub(super) item_group: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ItemGroupRow {
    pub(super) name: String,
    #[serde(default)]
    pub(super) item_group_name: String,
    #[serde(default)]
    pub(super) parent_item_group: String,
    #[serde(default)]
    pub(super) is_group: i32,
    #[serde(default)]
    pub(super) lft: i64,
    #[serde(default)]
    pub(super) rgt: i64,
}

#[derive(Debug, Deserialize)]
pub(super) struct WarehouseRow {
    pub(super) name: String,
    #[serde(default)]
    pub(super) company: String,
    #[serde(default)]
    pub(super) is_group: i32,
    #[serde(default)]
    pub(super) parent_warehouse: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ItemSupplierRow {
    pub(super) parent: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ItemSuppliersRow {
    #[serde(default)]
    pub(super) default_supplier: String,
    #[serde(default)]
    pub(super) supplier_items: Vec<ItemSupplierChildRow>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ItemSupplierChildRow {
    #[serde(default)]
    pub(super) name: String,
    #[serde(default)]
    pub(super) supplier: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ItemCustomersRow {
    #[serde(default)]
    pub(super) customer_items: Vec<ItemCustomerChildRow>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ItemCustomerChildRow {
    #[serde(default)]
    pub(super) name: String,
    #[serde(default)]
    pub(super) customer_name: String,
    #[serde(default)]
    pub(super) customer_group: String,
    #[serde(default)]
    pub(super) ref_code: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct CustomerItemsRow {
    #[serde(default)]
    pub(super) custom_customer_items: Vec<CustomerItemRow>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CustomerItemRow {
    #[serde(default)]
    pub(super) item_code: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct SearchLinkResponse {
    #[serde(default, alias = "message")]
    pub(super) results: Vec<SearchLinkRow>,
}

#[derive(Debug, Deserialize)]
pub(super) struct SearchLinkRow {
    #[serde(default)]
    pub(super) value: String,
}

pub(super) fn supplier_entry(row: SupplierRow) -> AdminDirectoryEntry {
    let phone = if row.mobile_no.trim().is_empty() {
        extract_phone_from_details(&row.supplier_details)
    } else {
        row.mobile_no.trim().to_string()
    };
    AdminDirectoryEntry {
        ref_: row.name.trim().to_string(),
        name: blank_default(&row.supplier_name, &row.name),
        phone,
    }
}

pub(super) fn customer_entry(row: CustomerRow) -> AdminDirectoryEntry {
    let phone = if row.mobile_no.trim().is_empty() {
        extract_phone_from_details(&row.customer_details)
    } else {
        row.mobile_no.trim().to_string()
    };
    AdminDirectoryEntry {
        ref_: row.name.trim().to_string(),
        name: blank_default(&row.customer_name, &row.name),
        phone,
    }
}

pub(super) fn supplier_item(row: ItemRow, warehouse: &str) -> SupplierItem {
    SupplierItem {
        code: row.name.trim().to_string(),
        name: blank_default(&row.item_name, &row.name),
        uom: row.stock_uom.trim().to_string(),
        warehouse: warehouse.trim().to_string(),
        item_group: row.item_group.trim().to_string(),
    }
}

pub(super) fn item_group(row: ItemGroupRow) -> AdminItemGroup {
    AdminItemGroup {
        name: row.name.trim().to_string(),
        item_group_name: blank_default(&row.item_group_name, &row.name),
        parent_item_group: row.parent_item_group.trim().to_string(),
        is_group: row.is_group != 0,
    }
}

pub(super) fn warehouse(row: WarehouseRow) -> crate::core::admin::models::AdminWarehouse {
    crate::core::admin::models::AdminWarehouse {
        warehouse: row.name.trim().to_string(),
        company: row.company.trim().to_string(),
        is_group: row.is_group != 0,
        parent_warehouse: row.parent_warehouse.trim().to_string(),
    }
}

pub(super) fn blank_default(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.trim().to_string()
    } else {
        trimmed.to_string()
    }
}

pub(super) fn extract_phone_from_details(details: &str) -> String {
    for line in details.replace("\r\n", "\n").lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();
        if lower.starts_with("telefon:") {
            return trimmed["telefon:".len()..].trim().to_string();
        }
        if lower.starts_with("phone:") {
            return trimmed["phone:".len()..].trim().to_string();
        }
    }
    String::new()
}

pub(super) fn normalize_limit(limit: usize, default: usize, max: usize) -> usize {
    if limit == 0 || limit > max {
        default
    } else {
        limit
    }
}

pub(super) fn supplier_or_filters(query: &str) -> String {
    let like = like_pattern(query);
    serde_json::json!([
        ["name", "like", like],
        ["supplier_name", "like", like],
        ["mobile_no", "like", like],
        ["supplier_details", "like", like],
    ])
    .to_string()
}

pub(super) fn customer_or_filters(query: &str) -> String {
    let like = like_pattern(query);
    serde_json::json!([
        ["name", "like", like],
        ["customer_name", "like", like],
        ["mobile_no", "like", like],
    ])
    .to_string()
}

pub(super) fn item_or_filters(query: &str) -> String {
    let like = like_pattern(query);
    serde_json::json!([["name", "like", like], ["item_name", "like", like],]).to_string()
}

pub(super) fn warehouse_or_filters(query: &str) -> String {
    let like = like_pattern(query);
    serde_json::json!([["name", "like", like], ["warehouse_name", "like", like],]).to_string()
}

pub(super) fn like_pattern(query: &str) -> String {
    format!("%{}%", query.trim().replace('"', ""))
}

pub(super) fn upsert_phone_in_details(_details: &str, phone: &str) -> String {
    format!("Telefon: {}", phone.trim())
}
