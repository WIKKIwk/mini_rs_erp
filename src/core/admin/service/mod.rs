mod helpers;
mod mutations;
mod read;
mod roles;

use std::collections::BTreeMap;
use std::sync::Arc;

use rand::Rng;
use time::OffsetDateTime;
use tokio::sync::RwLock;

use crate::config::AppConfig;
use crate::core::admin::models::{
    AdminActivity, AdminCustomerDetail, AdminDirectoryEntry, AdminItemGroupBulkMoveResult,
    AdminSettings, AdminState, AdminSupplier, AdminSupplierDetail, AdminSupplierSummary,
    AdminUserListEntry, AdminUserListPage,
};
use crate::core::admin::ports::{
    AdminAuthConfigSink, AdminEnvPersister, AdminPortError, AdminReadPort, AdminStatePort,
    AdminWritePort,
};
use crate::core::auth::access_codes::{SupplierAccessInput, supplier_access_code};
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::auth::service::normalize_phone;
use crate::core::authz::{
    Capability, MemoryRoleDefinitionStore, RoleAssignment, RoleAssignmentUpsert, RoleDefinition,
    RoleDefinitionStorePort, RoleDefinitionUpsert, capability_code, capability_codes_for_role,
    has_capability, normalize_custom_role, normalize_role_assignment, role_assignment_key,
    system_role_definitions,
};
use crate::core::werka::models::{CustomerDirectoryEntry, SupplierItem};

const CODE_REGEN_WINDOW_SECONDS: i64 = 60;
const MAX_CODE_REGENS_PER_WINDOW: i32 = 3;

#[derive(Clone)]
pub struct AdminService {
    config: Arc<RwLock<AdminConfig>>,
    read_port: Option<Arc<dyn AdminReadPort>>,
    write_port: Option<Arc<dyn AdminWritePort>>,
    state_port: Option<Arc<dyn AdminStatePort>>,
    env_persister: Option<Arc<dyn AdminEnvPersister>>,
    auth_config_sink: Option<Arc<dyn AdminAuthConfigSink>>,
    role_store: Arc<dyn RoleDefinitionStorePort>,
}

#[derive(Debug, Clone)]
struct AdminConfig {
    default_target_warehouse: String,
    default_uom: String,
    werka_phone: String,
    werka_name: String,
    werka_code: String,
    admin_phone: String,
    admin_name: String,
    supplier_prefix: String,
    werka_prefix: String,
}

impl AdminService {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(AdminConfig {
                default_target_warehouse: config.default_target_warehouse.clone(),
                default_uom: std::env::var("MINI_ERP_DEFAULT_UOM")
                    .ok()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| "Kg".to_string()),
                werka_phone: config.werka_phone.clone(),
                werka_name: config.werka_name.clone(),
                werka_code: config.werka_code.clone(),
                admin_phone: config.admin_phone.clone(),
                admin_name: config.admin_name.clone(),
                supplier_prefix: config.supplier_prefix.clone(),
                werka_prefix: config.werka_prefix.clone(),
            })),
            read_port: None,
            write_port: None,
            state_port: None,
            env_persister: None,
            auth_config_sink: None,
            role_store: Arc::new(MemoryRoleDefinitionStore::new()),
        }
    }

    pub fn with_read_port(mut self, read_port: Arc<dyn AdminReadPort>) -> Self {
        self.read_port = Some(read_port);
        self
    }

    pub fn with_write_port(mut self, write_port: Arc<dyn AdminWritePort>) -> Self {
        self.write_port = Some(write_port);
        self
    }

    pub fn with_state_port(mut self, state_port: Arc<dyn AdminStatePort>) -> Self {
        self.state_port = Some(state_port);
        self
    }

    pub fn with_env_persister(mut self, env_persister: Arc<dyn AdminEnvPersister>) -> Self {
        self.env_persister = Some(env_persister);
        self
    }

    pub fn with_auth_config_sink(mut self, auth_config_sink: Arc<dyn AdminAuthConfigSink>) -> Self {
        self.auth_config_sink = Some(auth_config_sink);
        self
    }

    pub async fn settings(&self) -> Result<AdminSettings, AdminPortError> {
        let config = self.config.read().await;
        let state = self.state_for("werka").await.unwrap_or_default();
        let now = OffsetDateTime::now_utc();
        Ok(AdminSettings {
            default_target_warehouse: config.default_target_warehouse.clone(),
            default_uom: config.default_uom.clone(),
            werka_phone: config.werka_phone.clone(),
            werka_name: config.werka_name.clone(),
            werka_code: config.werka_code.clone(),
            werka_code_locked: state.code_locked(now),
            werka_code_retry_after_sec: state.retry_after_seconds(now),
            admin_phone: config.admin_phone.clone(),
            admin_name: config.admin_name.clone(),
        })
    }

    pub async fn update_settings(
        &self,
        input: AdminSettings,
    ) -> Result<AdminSettings, AdminPortError> {
        let mut config = self.config.write().await;
        config.default_target_warehouse = input.default_target_warehouse.trim().to_string();
        config.default_uom = input.default_uom.trim().to_string();
        if config.default_uom.is_empty() {
            config.default_uom = "Kg".to_string();
        }
        config.werka_phone = input.werka_phone.trim().to_string();
        config.werka_name = input.werka_name.trim().to_string();
        config.werka_code = input.werka_code.trim().to_string();
        config.admin_phone = input.admin_phone.trim().to_string();
        config.admin_name = input.admin_name.trim().to_string();
        self.update_auth_runtime(
            &config.werka_phone,
            &config.werka_code,
            &config.werka_name,
            &config.admin_phone,
            &config.admin_name,
        );
        if let Some(persister) = &self.env_persister {
            persister.upsert(BTreeMap::from([
                (
                    "MINI_ERP_DEFAULT_TARGET_WAREHOUSE",
                    config.default_target_warehouse.clone(),
                ),
                ("MINI_ERP_DEFAULT_UOM", config.default_uom.clone()),
                ("WERKA_PHONE", config.werka_phone.clone()),
                ("WERKA_NAME", config.werka_name.clone()),
                ("MOBILE_DEV_WERKA_CODE", config.werka_code.clone()),
                ("ADMINKA_PHONE", config.admin_phone.clone()),
                ("ADMINKA_NAME", config.admin_name.clone()),
            ]))?;
        }
        drop(config);
        self.settings().await
    }

    fn read_port(&self) -> Result<&Arc<dyn AdminReadPort>, AdminPortError> {
        self.read_port.as_ref().ok_or(AdminPortError::LookupFailed)
    }

    fn write_port(&self) -> Result<&Arc<dyn AdminWritePort>, AdminPortError> {
        self.write_port.as_ref().ok_or(AdminPortError::LookupFailed)
    }

    fn update_auth_runtime(
        &self,
        werka_phone: &str,
        werka_code: &str,
        werka_name: &str,
        admin_phone: &str,
        admin_name: &str,
    ) {
        if let Some(sink) = &self.auth_config_sink {
            sink.set_runtime_identity(werka_phone, werka_code, werka_name, admin_phone, admin_name);
        }
    }

    async fn state_for(&self, ref_: &str) -> Result<AdminState, AdminPortError> {
        Ok(self
            .states()
            .await?
            .get(ref_.trim())
            .cloned()
            .unwrap_or_default())
    }

    async fn supplier_entries(
        &self,
        limit: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        const PAGE_SIZE: usize = 200;

        let read = self.read_port()?;
        let mut result = Vec::new();
        let mut offset = 0;
        loop {
            let mut page_limit = PAGE_SIZE;
            if limit > 0 {
                let remaining = limit.saturating_sub(result.len());
                if remaining == 0 {
                    break;
                }
                page_limit = page_limit.min(remaining);
            }
            let page = read.suppliers_page("", page_limit, offset).await?;
            let page_len = page.len();
            result.extend(page);
            if page_len < page_limit || (limit > 0 && result.len() >= limit) {
                break;
            }
            offset += page_limit;
        }
        if limit > 0 && result.len() > limit {
            result.truncate(limit);
        }
        Ok(result)
    }

    async fn supplier_entry_state(
        &self,
        ref_: &str,
        include_removed: bool,
    ) -> Result<(AdminDirectoryEntry, AdminState), AdminPortError> {
        let entry = self.read_port()?.supplier_by_ref(ref_.trim()).await?;
        let state = self.state_for(&entry.ref_).await?;
        if state.removed && !include_removed {
            return Err(AdminPortError::NotFound);
        }
        Ok((entry, state))
    }

    async fn states(&self) -> Result<BTreeMap<String, AdminState>, AdminPortError> {
        match &self.state_port {
            Some(port) => port.states().await,
            None => Ok(BTreeMap::new()),
        }
    }

    async fn put_state(&self, ref_: &str, state: AdminState) -> Result<(), AdminPortError> {
        self.state_port
            .as_ref()
            .ok_or(AdminPortError::LookupFailed)?
            .put_state(ref_, state)
            .await
    }

    async fn existing_state_codes(&self) -> Result<BTreeMap<String, ()>, AdminPortError> {
        Ok(self
            .states()
            .await?
            .into_values()
            .filter_map(|state| {
                let code = state.custom_code.trim();
                (!code.is_empty()).then(|| (code.to_string(), ()))
            })
            .collect())
    }

    async fn existing_codes(&self) -> Result<BTreeMap<String, ()>, AdminPortError> {
        let states = self.states().await?;
        let entries = self.read_port()?.suppliers_page("", 0, 0).await?;
        let mut existing = BTreeMap::new();
        for entry in entries {
            let state = states.get(entry.ref_.trim()).cloned().unwrap_or_default();
            if state.removed {
                continue;
            }
            if let Ok(code) = self.supplier_code(&entry, &state) {
                existing.insert(code, ());
            }
        }
        Ok(existing)
    }

    fn admin_suppliers_from_entries(
        &self,
        entries: Vec<AdminDirectoryEntry>,
        states: &BTreeMap<String, AdminState>,
    ) -> Result<Vec<AdminSupplier>, AdminPortError> {
        let mut result = Vec::with_capacity(entries.len());
        for entry in entries {
            let state = states.get(entry.ref_.trim()).cloned().unwrap_or_default();
            if state.removed {
                continue;
            }
            result.push(self.build_supplier(entry, state)?);
        }
        Ok(result)
    }

    fn build_supplier(
        &self,
        entry: AdminDirectoryEntry,
        state: AdminState,
    ) -> Result<AdminSupplier, AdminPortError> {
        let code = self.supplier_code(&entry, &state)?;
        Ok(AdminSupplier {
            ref_: entry.ref_,
            name: entry.name,
            phone: entry.phone,
            code,
            blocked: state.blocked,
            removed: state.removed,
            assigned_item_count: state.assigned_item_codes.len(),
            assigned_item_codes: state.assigned_item_codes,
        })
    }

    fn supplier_code(
        &self,
        entry: &AdminDirectoryEntry,
        state: &AdminState,
    ) -> Result<String, AdminPortError> {
        let custom = state.custom_code.trim();
        if !custom.is_empty() {
            return Ok(custom.to_string());
        }
        supplier_access_code(&SupplierAccessInput {
            ref_: entry.ref_.clone(),
            name: entry.name.clone(),
            phone: entry.phone.clone(),
        })
        .map_err(|_| AdminPortError::LookupFailed)
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
