use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::core::auth::models::PrincipalRole;
use crate::core::werka::models::{CustomerDirectoryEntry, DispatchRecord, SupplierItem};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminSettings {
    pub default_target_warehouse: String,
    pub default_uom: String,
    pub werka_phone: String,
    pub werka_name: String,
    #[serde(default)]
    pub werka_avatar_url: String,
    pub werka_code: String,
    pub werka_code_locked: bool,
    pub werka_code_retry_after_sec: i64,
    pub admin_phone: String,
    pub admin_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AdminServerMonitorResponse {
    pub server: AdminServerMonitorServer,
    pub database: AdminServerMonitorDatabase,
    pub backups: AdminServerMonitorBackups,
    pub runtime: AdminServerMonitorRuntime,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminServerMonitorServer {
    pub bind_addr: String,
    pub started_at_unix: i64,
    pub uptime_seconds: i64,
    pub status: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminServerMonitorDatabase {
    pub configured: bool,
    pub reachable: bool,
    pub status: String,
    pub ping_ms: i64,
    pub error: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminServerMonitorBackups {
    pub directory: String,
    pub exists: bool,
    pub file_count: usize,
    pub latest: Option<AdminServerMonitorBackupFile>,
    pub files: Vec<AdminServerMonitorBackupFile>,
    pub error: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminServerMonitorBackupFile {
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
    pub modified_at_unix: i64,
    pub age_seconds: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AdminServerMonitorRuntime {
    pub cpu_percent: i64,
    pub memory_percent: i64,
    pub memory_used_mb: i64,
    pub memory_total_mb: i64,
    pub disk_path: String,
    pub disk_percent: i64,
    pub disk_used_mb: i64,
    pub disk_total_mb: i64,
    pub disk_available_mb: i64,
    pub load_average: f64,
    pub sample_seconds: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminWarehouse {
    pub warehouse: String,
    pub company: String,
    pub is_group: bool,
    pub parent_warehouse: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminCreateSupplierRequest {
    pub name: String,
    pub phone: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminCreateCustomerRequest {
    pub name: String,
    pub phone: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminCreateMaterialTaminotchiRequest {
    pub name: String,
    pub phone: String,
    #[serde(default)]
    pub assigned_item_groups: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminSupplier {
    #[serde(rename = "ref")]
    pub ref_: String,
    pub name: String,
    pub phone: String,
    pub code: String,
    pub blocked: bool,
    pub removed: bool,
    pub assigned_item_codes: Vec<String>,
    pub assigned_item_count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminSupplierSummary {
    pub total_suppliers: usize,
    pub active_suppliers: usize,
    pub blocked_suppliers: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AdminSuppliersPage {
    pub summary: AdminSupplierSummary,
    pub suppliers: Vec<AdminSupplier>,
    pub customers: Vec<CustomerDirectoryEntry>,
    pub settings: AdminSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminUserListEntry {
    pub id: String,
    pub source: String,
    #[serde(rename = "entity_ref")]
    pub entity_ref: String,
    pub principal_role: PrincipalRole,
    pub name: String,
    pub phone: String,
    pub role_label: String,
    pub blocked: bool,
    pub status: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminUserListPage {
    pub items: Vec<AdminUserListEntry>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AdminSupplierDetail {
    #[serde(rename = "ref")]
    pub ref_: String,
    pub name: String,
    pub phone: String,
    pub avatar_url: String,
    pub code: String,
    pub blocked: bool,
    pub removed: bool,
    pub code_locked: bool,
    pub code_retry_after_sec: i64,
    pub assigned_items: Vec<SupplierItem>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AdminCustomerDetail {
    #[serde(rename = "ref")]
    pub ref_: String,
    pub name: String,
    pub phone: String,
    pub avatar_url: String,
    pub code: String,
    pub code_locked: bool,
    pub code_retry_after_sec: i64,
    pub assigned_items: Vec<SupplierItem>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminWorkerDetail {
    pub id: String,
    pub name: String,
    pub phone: String,
    pub avatar_url: String,
    pub level: String,
    pub code: String,
    pub code_locked: bool,
    pub code_retry_after_sec: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminPhoneUpdateRequest {
    pub phone: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminSupplierStatusUpdateRequest {
    pub blocked: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminSupplierItemsUpdateRequest {
    pub item_codes: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminSupplierItemMutationRequest {
    pub item_code: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminCreateItemRequest {
    pub code: String,
    pub name: String,
    pub uom: String,
    pub item_group: String,
    #[serde(default)]
    pub customer_ref: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminCreateItemGroupRequest {
    pub name: String,
    pub parent: String,
    pub is_group: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminMoveItemGroupRequest {
    pub name: String,
    pub parent: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminItemGroup {
    pub name: String,
    pub item_group_name: String,
    pub parent_item_group: String,
    pub is_group: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminBulkMoveItemsRequest {
    pub item_codes: Vec<String>,
    pub item_group: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminItemGroupBulkMoveResult {
    pub item_group: String,
    pub requested_count: usize,
    pub updated_count: usize,
    pub failed_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub updated_item_codes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_item_codes: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdminDirectoryEntry {
    pub ref_: String,
    pub name: String,
    pub phone: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdminState {
    pub custom_code: String,
    pub blocked: bool,
    pub removed: bool,
    pub assigned_item_codes: Vec<String>,
    pub cooldown_until: Option<OffsetDateTime>,
    pub regen_window_started_at: Option<OffsetDateTime>,
    pub regen_window_count: i32,
    pub pending_persist_code: String,
    pub pending_persist_at: Option<OffsetDateTime>,
    pub assignments_configured: bool,
}

impl AdminState {
    pub fn code_locked(&self, now: OffsetDateTime) -> bool {
        self.cooldown_until.is_some_and(|until| now < until)
    }

    pub fn retry_after_seconds(&self, now: OffsetDateTime) -> i64 {
        let Some(until) = self.cooldown_until else {
            return 0;
        };
        if now >= until {
            return 0;
        }
        let seconds = (until - now).whole_seconds();
        seconds.max(1)
    }
}

pub type AdminActivity = Vec<DispatchRecord>;
