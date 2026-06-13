use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokio::sync::Mutex;

use crate::core::admin::models::{AdminDirectoryEntry, AdminItemGroup, AdminState, AdminWarehouse};
use crate::core::admin::ports::{AdminPortError, AdminReadPort, AdminStatePort, AdminWritePort};
use crate::core::auth::ports::{
    AdminAccessState, AdminAccessStateLookup, AuthPortError, CustomerLookup, CustomerRecord,
    SupplierLookup, SupplierRecord,
};
use crate::core::werka::models::SupplierItem;

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
    #[serde(default)]
    suppliers: BTreeMap<String, AdminDirectoryEntryData>,
    #[serde(default)]
    customers: BTreeMap<String, AdminDirectoryEntryData>,
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
    warehouse: String,
    item_group: String,
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
            suppliers: BTreeMap::new(),
            customers: BTreeMap::new(),
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

#[async_trait]
impl AdminReadPort for JsonAdminStore {
    async fn suppliers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        let data = self.data.lock().await;
        Ok(paginate(
            data.suppliers
                .values()
                .filter(|entry| entry_matches(entry, query))
                .map(AdminDirectoryEntry::from)
                .collect(),
            limit,
            offset,
        ))
    }

    async fn supplier_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        let data = self.data.lock().await;
        data.suppliers
            .get(ref_.trim())
            .map(AdminDirectoryEntry::from)
            .ok_or(AdminPortError::NotFound)
    }

    async fn customers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        let data = self.data.lock().await;
        Ok(paginate(
            data.customers
                .values()
                .filter(|entry| entry_matches(entry, query))
                .map(AdminDirectoryEntry::from)
                .collect(),
            limit,
            offset,
        ))
    }

    async fn customer_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        let data = self.data.lock().await;
        data.customers
            .get(ref_.trim())
            .map(AdminDirectoryEntry::from)
            .ok_or(AdminPortError::NotFound)
    }

    async fn items_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        let data = self.data.lock().await;
        Ok(paginate(
            data.items
                .values()
                .filter(|item| item_matches(item, query))
                .map(SupplierItem::from)
                .collect(),
            limit,
            offset,
        ))
    }

    async fn items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        let wanted = item_codes
            .iter()
            .map(|code| code.trim().to_lowercase())
            .collect::<BTreeSet<_>>();
        let data = self.data.lock().await;
        Ok(data
            .items
            .values()
            .filter(|item| wanted.contains(&item.code.trim().to_lowercase()))
            .map(SupplierItem::from)
            .collect())
    }

    async fn item_groups(&self, query: &str, limit: usize) -> Result<Vec<String>, AdminPortError> {
        let needle = query.trim().to_lowercase();
        let data = self.data.lock().await;
        Ok(paginate(
            data.item_groups
                .values()
                .filter(|group| needle.is_empty() || group.name.to_lowercase().contains(&needle))
                .map(|group| group.name.clone())
                .collect(),
            limit,
            0,
        ))
    }

    async fn warehouses(
        &self,
        query: &str,
        parent: &str,
        limit: usize,
    ) -> Result<Vec<AdminWarehouse>, AdminPortError> {
        if !parent.trim().is_empty() {
            return Ok(Vec::new());
        }
        let needle = query.trim().to_lowercase();
        let data = self.data.lock().await;
        let mut seen = BTreeSet::new();
        Ok(paginate(
            data.items
                .values()
                .filter_map(|item| {
                    let warehouse = item.warehouse.trim();
                    if warehouse.is_empty()
                        || !seen.insert(warehouse.to_lowercase())
                        || (!needle.is_empty() && !warehouse.to_lowercase().contains(&needle))
                    {
                        return None;
                    }
                    Some(AdminWarehouse {
                        warehouse: warehouse.to_string(),
                        company: String::new(),
                        is_group: false,
                        parent_warehouse: String::new(),
                    })
                })
                .collect(),
            limit,
            0,
        ))
    }

    async fn item_group_tree(&self) -> Result<Vec<AdminItemGroup>, AdminPortError> {
        let data = self.data.lock().await;
        Ok(data
            .item_groups
            .values()
            .map(AdminItemGroup::from)
            .collect())
    }

    async fn assigned_supplier_items(
        &self,
        supplier_ref: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        let data = self.data.lock().await;
        Ok(assigned_items(
            &data.items,
            data.supplier_items
                .get(supplier_ref.trim())
                .map(Vec::as_slice)
                .unwrap_or(&[]),
            "",
            limit,
        ))
    }

    async fn customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        let data = self.data.lock().await;
        Ok(assigned_items(
            &data.items,
            data.customer_items
                .get(customer_ref.trim())
                .map(Vec::as_slice)
                .unwrap_or(&[]),
            query,
            limit,
        ))
    }
}

#[async_trait]
impl AdminWritePort for JsonAdminStore {
    async fn create_supplier(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        let mut data = self.data.lock().await;
        let ref_ = next_ref("SUP", data.next_supplier_id);
        data.next_supplier_id += 1;
        let entry = AdminDirectoryEntryData::new(&ref_, name, phone);
        data.suppliers.insert(ref_.clone(), entry.clone());
        self.persist(&data).await?;
        Ok(AdminDirectoryEntry::from(&entry))
    }

    async fn update_supplier_phone(&self, ref_: &str, phone: &str) -> Result<(), AdminPortError> {
        let mut data = self.data.lock().await;
        let entry = data
            .suppliers
            .get_mut(ref_.trim())
            .ok_or(AdminPortError::NotFound)?;
        entry.phone = phone.trim().to_string();
        self.persist(&data).await
    }

    async fn assign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        let mut data = self.data.lock().await;
        ensure_item_exists(&data, item_code)?;
        push_unique(
            data.supplier_items
                .entry(ref_.trim().to_string())
                .or_default(),
            item_code,
        );
        self.persist(&data).await
    }

    async fn unassign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        let mut data = self.data.lock().await;
        remove_code(
            data.supplier_items
                .entry(ref_.trim().to_string())
                .or_default(),
            item_code,
        );
        self.persist(&data).await
    }

    async fn create_customer(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        let mut data = self.data.lock().await;
        let ref_ = next_ref("CUST", data.next_customer_id);
        data.next_customer_id += 1;
        let entry = AdminDirectoryEntryData::new(&ref_, name, phone);
        data.customers.insert(ref_.clone(), entry.clone());
        self.persist(&data).await?;
        Ok(AdminDirectoryEntry::from(&entry))
    }

    async fn update_customer_phone(&self, ref_: &str, phone: &str) -> Result<(), AdminPortError> {
        let mut data = self.data.lock().await;
        let entry = data
            .customers
            .get_mut(ref_.trim())
            .ok_or(AdminPortError::NotFound)?;
        entry.phone = phone.trim().to_string();
        self.persist(&data).await
    }

    async fn update_customer_code(&self, ref_: &str, code: &str) -> Result<(), AdminPortError> {
        let mut data = self.data.lock().await;
        data.states
            .entry(ref_.trim().to_string())
            .or_default()
            .custom_code = code.trim().to_string();
        self.persist(&data).await
    }

    async fn assign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        let mut data = self.data.lock().await;
        ensure_item_exists(&data, item_code)?;
        push_unique(
            data.customer_items
                .entry(ref_.trim().to_string())
                .or_default(),
            item_code,
        );
        self.persist(&data).await
    }

    async fn unassign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        let mut data = self.data.lock().await;
        remove_code(
            data.customer_items
                .entry(ref_.trim().to_string())
                .or_default(),
            item_code,
        );
        self.persist(&data).await
    }

    async fn create_item(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
    ) -> Result<SupplierItem, AdminPortError> {
        let code = code.trim();
        if code.is_empty() {
            return Err(AdminPortError::InvalidInput(
                "item code is required".to_string(),
            ));
        }
        let mut data = self.data.lock().await;
        let item = StoredSupplierItem {
            code: code.to_string(),
            name: name.trim().to_string(),
            uom: blank_default(uom, "Kg"),
            warehouse: String::new(),
            item_group: blank_default(item_group, "All Item Groups"),
        };
        data.items.insert(code.to_string(), item.clone());
        data.item_groups
            .entry(item.item_group.clone())
            .or_insert_with(|| StoredItemGroup {
                name: item.item_group.clone(),
                parent_item_group: "All Item Groups".to_string(),
                is_group: true,
            });
        self.persist(&data).await?;
        Ok(SupplierItem::from(&item))
    }

    async fn create_item_group(
        &self,
        name: &str,
        parent: &str,
        is_group: bool,
    ) -> Result<AdminItemGroup, AdminPortError> {
        let mut data = self.data.lock().await;
        let group = StoredItemGroup {
            name: name.trim().to_string(),
            parent_item_group: parent.trim().to_string(),
            is_group,
        };
        data.item_groups.insert(group.name.clone(), group.clone());
        self.persist(&data).await?;
        Ok(AdminItemGroup::from(&group))
    }

    async fn move_item_group_parent(
        &self,
        name: &str,
        parent: &str,
    ) -> Result<AdminItemGroup, AdminPortError> {
        let mut data = self.data.lock().await;
        let group = data
            .item_groups
            .get_mut(name.trim())
            .ok_or(AdminPortError::NotFound)?;
        group.parent_item_group = parent.trim().to_string();
        let result = AdminItemGroup::from(&*group);
        self.persist(&data).await?;
        Ok(result)
    }

    async fn update_item_group(
        &self,
        item_code: &str,
        item_group: &str,
    ) -> Result<(), AdminPortError> {
        let mut data = self.data.lock().await;
        let item = data
            .items
            .get_mut(item_code.trim())
            .ok_or(AdminPortError::NotFound)?;
        item.item_group = item_group.trim().to_string();
        self.persist(&data).await
    }
}

#[async_trait]
impl AdminStatePort for JsonAdminStore {
    async fn states(&self) -> Result<BTreeMap<String, AdminState>, AdminPortError> {
        let data = self.data.lock().await;
        Ok(data
            .states
            .iter()
            .map(|(key, state)| (key.clone(), AdminState::from(state)))
            .collect())
    }

    async fn put_state(&self, ref_: &str, state: AdminState) -> Result<(), AdminPortError> {
        let mut data = self.data.lock().await;
        data.states
            .insert(ref_.trim().to_string(), StoredAdminState::from(&state));
        self.persist(&data).await
    }
}

#[async_trait]
impl SupplierLookup for JsonAdminStore {
    async fn search_suppliers(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierRecord>, AuthPortError> {
        let data = self.data.lock().await;
        Ok(paginate(
            data.suppliers
                .values()
                .filter(|entry| entry_matches(entry, query))
                .map(SupplierRecord::from)
                .collect(),
            limit,
            0,
        ))
    }
}

#[async_trait]
impl CustomerLookup for JsonAdminStore {
    async fn search_customers(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<CustomerRecord>, AuthPortError> {
        let data = self.data.lock().await;
        Ok(paginate(
            data.customers
                .values()
                .filter(|entry| entry_matches(entry, query))
                .map(CustomerRecord::from)
                .collect(),
            limit,
            0,
        ))
    }
}

#[async_trait]
impl AdminAccessStateLookup for JsonAdminStore {
    async fn list_states(&self) -> Result<BTreeMap<String, AdminAccessState>, AuthPortError> {
        let data = self.data.lock().await;
        Ok(data
            .states
            .iter()
            .map(|(key, state)| {
                (
                    key.clone(),
                    AdminAccessState {
                        custom_code: state.custom_code.clone(),
                        blocked: state.blocked,
                        removed: state.removed,
                    },
                )
            })
            .collect())
    }
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

impl From<&StoredSupplierItem> for SupplierItem {
    fn from(value: &StoredSupplierItem) -> Self {
        Self {
            code: value.code.clone(),
            name: value.name.clone(),
            uom: value.uom.clone(),
            warehouse: value.warehouse.clone(),
            item_group: value.item_group.clone(),
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
