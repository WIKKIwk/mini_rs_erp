use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokio::sync::Mutex;

use crate::core::admin::item_customer_policy::item_group_requires_customer;
use crate::core::admin::models::{
    AdminDirectoryEntry, AdminItemDetail, AdminItemGroup, AdminState, AdminWarehouse,
};
use crate::core::admin::ports::{AdminPortError, AdminReadPort, AdminStatePort, AdminWritePort};
use crate::core::auth::ports::{
    AdminAccessState, AdminAccessStateLookup, AuthPortError, CustomerLookup, CustomerRecord,
    MaterialTaminotchiLookup, MaterialTaminotchiRecord, SupplierLookup, SupplierRecord,
};
use crate::core::werka::models::{CustomerDirectoryEntry, SupplierItem};

mod access_ports;
mod item_customer_writes;
mod read_port;
#[cfg(test)]
mod tests;
mod write_port;

#[derive(Debug)]
pub struct JsonAdminStore {
    path: PathBuf,
    data: Mutex<StoredAdminData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredAdminData {
    #[serde(default = "one")]
    next_supplier_id: u64,
    #[serde(default = "one")]
    next_customer_id: u64,
    #[serde(default = "one")]
    next_material_taminotchi_id: u64,
    #[serde(default)]
    suppliers: BTreeMap<String, AdminDirectoryEntryData>,
    #[serde(default)]
    customers: BTreeMap<String, AdminDirectoryEntryData>,
    #[serde(default)]
    material_taminotchilar: BTreeMap<String, AdminDirectoryEntryData>,
    #[serde(default)]
    items: BTreeMap<String, StoredSupplierItem>,
    #[serde(default = "default_item_groups")]
    item_groups: BTreeMap<String, StoredItemGroup>,
    #[serde(default)]
    supplier_items: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    customer_items: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    states: BTreeMap<String, StoredAdminState>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AdminDirectoryEntryData {
    #[serde(rename = "ref")]
    ref_: String,
    name: String,
    phone: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StoredSupplierItem {
    code: String,
    name: String,
    uom: String,
    item_group: String,
    #[serde(default)]
    created_at_unix: i64,
    #[serde(default)]
    updated_at_unix: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StoredItemGroup {
    name: String,
    parent_item_group: String,
    is_group: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StoredAdminState {
    #[serde(default)]
    custom_code: String,
    #[serde(default)]
    blocked: bool,
    #[serde(default)]
    removed: bool,
    #[serde(default)]
    assigned_item_codes: Vec<String>,
    #[serde(default)]
    cooldown_until_unix: Option<i64>,
    #[serde(default)]
    regen_window_started_at_unix: Option<i64>,
    #[serde(default)]
    regen_window_count: i32,
    #[serde(default)]
    pending_persist_code: String,
    #[serde(default)]
    pending_persist_at_unix: Option<i64>,
    #[serde(default)]
    assignments_configured: bool,
}

fn one() -> u64 {
    1
}

fn default_item_groups() -> BTreeMap<String, StoredItemGroup> {
    BTreeMap::from([(
        "All Item Groups".to_string(),
        StoredItemGroup {
            name: "All Item Groups".to_string(),
            parent_item_group: String::new(),
            is_group: true,
        },
    )])
}

impl Default for StoredAdminData {
    fn default() -> Self {
        Self {
            next_supplier_id: 1,
            next_customer_id: 1,
            next_material_taminotchi_id: 1,
            suppliers: BTreeMap::new(),
            customers: BTreeMap::new(),
            material_taminotchilar: BTreeMap::new(),
            items: BTreeMap::new(),
            item_groups: default_item_groups(),
            supplier_items: BTreeMap::new(),
            customer_items: BTreeMap::new(),
            states: BTreeMap::new(),
        }
    }
}

impl JsonAdminStore {
    pub fn new(path: PathBuf) -> Self {
        let data = read_data(&path).unwrap_or_default();
        Self {
            path,
            data: Mutex::new(data),
        }
    }

    async fn persist(&self, data: &StoredAdminData) -> Result<(), AdminPortError> {
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|_| AdminPortError::LookupFailed)?;
        }
        let raw = serde_json::to_vec_pretty(data).map_err(|_| AdminPortError::LookupFailed)?;
        let tmp_path = self.path.with_extension("json.tmp");
        tokio::fs::write(&tmp_path, raw)
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        tokio::fs::rename(tmp_path, &self.path)
            .await
            .map_err(|_| AdminPortError::LookupFailed)
    }
}

fn read_data(path: &Path) -> Result<StoredAdminData, AdminPortError> {
    if !path.exists() {
        return Ok(StoredAdminData::default());
    }
    let raw = std::fs::read(path).map_err(|_| AdminPortError::LookupFailed)?;
    if raw.is_empty() {
        return Ok(StoredAdminData::default());
    }
    serde_json::from_slice(&raw).map_err(|_| AdminPortError::LookupFailed)
}

impl AdminDirectoryEntryData {
    fn new(ref_: &str, name: &str, phone: &str) -> Self {
        Self {
            ref_: ref_.trim().to_string(),
            name: name.trim().to_string(),
            phone: phone.trim().to_string(),
        }
    }
}

impl From<&AdminDirectoryEntryData> for AdminDirectoryEntry {
    fn from(value: &AdminDirectoryEntryData) -> Self {
        Self {
            ref_: value.ref_.clone(),
            name: value.name.clone(),
            phone: value.phone.clone(),
        }
    }
}

impl From<&AdminDirectoryEntryData> for SupplierRecord {
    fn from(value: &AdminDirectoryEntryData) -> Self {
        Self {
            id: value.ref_.clone(),
            name: value.name.clone(),
            phone: value.phone.clone(),
        }
    }
}

impl From<&AdminDirectoryEntryData> for CustomerRecord {
    fn from(value: &AdminDirectoryEntryData) -> Self {
        Self {
            id: value.ref_.clone(),
            name: value.name.clone(),
            phone: value.phone.clone(),
        }
    }
}

impl From<&AdminDirectoryEntryData> for MaterialTaminotchiRecord {
    fn from(value: &AdminDirectoryEntryData) -> Self {
        Self {
            id: value.ref_.clone(),
            name: value.name.clone(),
            phone: value.phone.clone(),
        }
    }
}

impl From<&StoredSupplierItem> for SupplierItem {
    fn from(value: &StoredSupplierItem) -> Self {
        Self {
            code: value.code.clone(),
            name: value.name.clone(),
            uom: value.uom.clone(),
            warehouse: String::new(),
            item_group: value.item_group.clone(),
            customer_names: Vec::new(),
        }
    }
}

impl From<&StoredItemGroup> for AdminItemGroup {
    fn from(value: &StoredItemGroup) -> Self {
        Self {
            name: value.name.clone(),
            item_group_name: value.name.clone(),
            parent_item_group: value.parent_item_group.clone(),
            is_group: value.is_group,
        }
    }
}

impl From<&StoredAdminState> for AdminState {
    fn from(value: &StoredAdminState) -> Self {
        Self {
            custom_code: value.custom_code.clone(),
            blocked: value.blocked,
            removed: value.removed,
            assigned_item_codes: value.assigned_item_codes.clone(),
            cooldown_until: unix_to_time(value.cooldown_until_unix),
            regen_window_started_at: unix_to_time(value.regen_window_started_at_unix),
            regen_window_count: value.regen_window_count,
            pending_persist_code: value.pending_persist_code.clone(),
            pending_persist_at: unix_to_time(value.pending_persist_at_unix),
            assignments_configured: value.assignments_configured,
        }
    }
}

impl From<&AdminState> for StoredAdminState {
    fn from(value: &AdminState) -> Self {
        Self {
            custom_code: value.custom_code.clone(),
            blocked: value.blocked,
            removed: value.removed,
            assigned_item_codes: value.assigned_item_codes.clone(),
            cooldown_until_unix: time_to_unix(value.cooldown_until),
            regen_window_started_at_unix: time_to_unix(value.regen_window_started_at),
            regen_window_count: value.regen_window_count,
            pending_persist_code: value.pending_persist_code.clone(),
            pending_persist_at_unix: time_to_unix(value.pending_persist_at),
            assignments_configured: value.assignments_configured,
        }
    }
}

fn unix_to_time(value: Option<i64>) -> Option<OffsetDateTime> {
    value.and_then(|unix| OffsetDateTime::from_unix_timestamp(unix).ok())
}

fn time_to_unix(value: Option<OffsetDateTime>) -> Option<i64> {
    value.map(|time| time.unix_timestamp())
}

fn paginate<T>(items: Vec<T>, limit: usize, offset: usize) -> Vec<T> {
    let iter = items.into_iter().skip(offset);
    if limit == 0 {
        iter.collect()
    } else {
        iter.take(limit).collect()
    }
}

fn next_ref(prefix: &str, id: u64) -> String {
    format!("{prefix}-{id:03}")
}

fn blank_default(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn entry_matches(entry: &AdminDirectoryEntryData, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || entry.ref_.to_lowercase().contains(&query)
        || entry.name.to_lowercase().contains(&query)
        || entry.phone.to_lowercase().contains(&query)
}

fn item_matches(item: &StoredSupplierItem, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || item.code.to_lowercase().contains(&query)
        || item.name.to_lowercase().contains(&query)
        || item.item_group.to_lowercase().contains(&query)
}

fn ensure_item_exists(data: &StoredAdminData, item_code: &str) -> Result<(), AdminPortError> {
    if data.items.contains_key(item_code.trim()) {
        Ok(())
    } else {
        Err(AdminPortError::NotFound)
    }
}

fn push_unique(values: &mut Vec<String>, item_code: &str) {
    let item_code = item_code.trim();
    if item_code.is_empty()
        || values
            .iter()
            .any(|value| value.trim().eq_ignore_ascii_case(item_code))
    {
        return;
    }
    values.push(item_code.to_string());
}

fn remove_code(values: &mut Vec<String>, item_code: &str) {
    values.retain(|value| !value.trim().eq_ignore_ascii_case(item_code.trim()));
}

fn replace_assigned_item_code(
    assignments: &mut BTreeMap<String, Vec<String>>,
    original_code: &str,
    code: &str,
) {
    for values in assignments.values_mut() {
        for value in values.iter_mut() {
            if value.trim().eq_ignore_ascii_case(original_code) {
                *value = code.to_string();
            }
        }
        let mut seen = BTreeSet::new();
        values.retain(|value| seen.insert(value.trim().to_ascii_lowercase()));
    }
}

fn stored_item_detail(data: &StoredAdminData, item: &StoredSupplierItem) -> AdminItemDetail {
    let mut customers = data
        .customer_items
        .iter()
        .filter(|(_, item_codes)| {
            item_codes
                .iter()
                .any(|code| code.trim().eq_ignore_ascii_case(&item.code))
        })
        .filter_map(|(customer_ref, _)| data.customers.get(customer_ref))
        .map(|customer| CustomerDirectoryEntry {
            ref_: customer.ref_.clone(),
            name: customer.name.clone(),
            phone: customer.phone.clone(),
        })
        .collect::<Vec<_>>();
    customers.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
            .then_with(|| left.ref_.cmp(&right.ref_))
    });
    AdminItemDetail {
        code: item.code.clone(),
        name: item.name.clone(),
        uom: item.uom.clone(),
        item_group: item.item_group.clone(),
        is_finished_goods: stored_item_group_is_finished_goods(data, &item.item_group),
        created_at_unix: item.created_at_unix,
        updated_at_unix: item.updated_at_unix,
        customers,
    }
}

fn stored_item_summary(data: &StoredAdminData, item: &StoredSupplierItem) -> SupplierItem {
    let mut summary = SupplierItem::from(item);
    summary.customer_names = stored_item_customer_names(data, &item.code);
    summary
}

fn stored_item_customer_names(data: &StoredAdminData, item_code: &str) -> Vec<String> {
    let mut names = data
        .customer_items
        .iter()
        .filter(|(_, item_codes)| {
            item_codes
                .iter()
                .any(|code| code.trim().eq_ignore_ascii_case(item_code.trim()))
        })
        .filter_map(|(customer_ref, _)| data.customers.get(customer_ref))
        .map(|customer| {
            let name = customer.name.trim();
            if name.is_empty() {
                customer.ref_.trim().to_string()
            } else {
                name.to_string()
            }
        })
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    names.sort_by_key(|name| name.to_ascii_lowercase());
    names.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    names
}

fn stored_item_group_is_finished_goods(data: &StoredAdminData, item_group: &str) -> bool {
    let groups = data
        .item_groups
        .values()
        .map(AdminItemGroup::from)
        .collect::<Vec<_>>();
    item_group_requires_customer(item_group, &groups)
}

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
