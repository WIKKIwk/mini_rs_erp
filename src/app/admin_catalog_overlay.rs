use std::sync::Arc;

use async_trait::async_trait;

use crate::core::admin::models::{AdminDirectoryEntry, AdminItemGroup, AdminWarehouse};
use crate::core::admin::ports::{AdminPortError, AdminReadPort, AdminWritePort};
use crate::core::werka::models::SupplierItem;
use crate::db::postgres::PostgresConfig;
use crate::db::postgres_admin_catalog::PostgresAdminCatalogStore;
use crate::db::postgres_customer::PostgresCustomerStore;
use crate::store::admin_store::JsonAdminStore;

pub(super) fn build_admin_catalog_ports(
    admin_store: Arc<JsonAdminStore>,
    customer_store: Option<Arc<PostgresCustomerStore>>,
) -> (Arc<dyn AdminReadPort>, Arc<dyn AdminWritePort>) {
    let config = match PostgresConfig::from_env() {
        Ok(config) => config,
        Err(error) => {
            tracing::warn!(?error, "mini ERP postgres item catalog store disabled");
            return unavailable_admin_catalog_ports();
        }
    };
    match config.pool_options().connect_lazy(&config.database_url) {
        Ok(pool) => {
            tracing::info!("mini ERP postgres item catalog store configured without JSON seed");
            let catalog = Arc::new(PostgresAdminCatalogStore::new(pool));
            let overlay = Arc::new(AdminCatalogOverlay {
                admin_store,
                catalog,
                customer_store,
            });
            (
                overlay.clone() as Arc<dyn AdminReadPort>,
                overlay as Arc<dyn AdminWritePort>,
            )
        }
        Err(error) => {
            tracing::warn!(%error, "mini ERP postgres item catalog store disabled");
            unavailable_admin_catalog_ports()
        }
    }
}

fn unavailable_admin_catalog_ports() -> (Arc<dyn AdminReadPort>, Arc<dyn AdminWritePort>) {
    let store = Arc::new(AdminCatalogUnavailable);
    (
        store.clone() as Arc<dyn AdminReadPort>,
        store as Arc<dyn AdminWritePort>,
    )
}

struct AdminCatalogOverlay {
    admin_store: Arc<JsonAdminStore>,
    catalog: Arc<PostgresAdminCatalogStore>,
    customer_store: Option<Arc<PostgresCustomerStore>>,
}

struct AdminCatalogUnavailable;

impl AdminCatalogOverlay {
    async fn sync_item_to_admin_store(&self, item: &SupplierItem) -> Result<(), AdminPortError> {
        self.admin_store
            .create_item(&item.code, &item.name, &item.uom, &item.item_group)
            .await?;
        Ok(())
    }

    async fn ensure_assignment_item(&self, item_code: &str) -> Result<(), AdminPortError> {
        let item_code = item_code.trim();
        let items = self
            .catalog
            .items_by_codes(&[item_code.to_string()])
            .await?;
        let item = items
            .iter()
            .find(|item| item.code.trim().eq_ignore_ascii_case(item_code))
            .ok_or(AdminPortError::NotFound)?;
        self.sync_item_to_admin_store(item).await
    }
}

#[async_trait]
impl AdminReadPort for AdminCatalogOverlay {
    async fn suppliers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        self.admin_store.suppliers_page(query, limit, offset).await
    }

    async fn supplier_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        self.admin_store.supplier_by_ref(ref_).await
    }

    async fn customers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        match &self.customer_store {
            Some(store) => store.customers_page(query, limit, offset).await,
            None => self.admin_store.customers_page(query, limit, offset).await,
        }
    }

    async fn customer_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        match &self.customer_store {
            Some(store) => store.customer_by_ref(ref_).await,
            None => self.admin_store.customer_by_ref(ref_).await,
        }
    }

    async fn material_taminotchilar_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        self.admin_store
            .material_taminotchilar_page(query, limit, offset)
            .await
    }

    async fn material_taminotchi_by_ref(
        &self,
        ref_: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        self.admin_store.material_taminotchi_by_ref(ref_).await
    }

    async fn items_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        self.catalog.items_page(query, limit, offset).await
    }

    async fn items_page_by_warehouse(
        &self,
        warehouse: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        self.catalog
            .items_page_by_warehouse(warehouse, query, limit, offset)
            .await
    }

    async fn items_page_by_group(
        &self,
        group: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        self.catalog
            .items_page_by_group(group, query, limit, offset)
            .await
    }

    async fn items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        self.catalog.items_by_codes(item_codes).await
    }

    async fn item_groups(&self, query: &str, limit: usize) -> Result<Vec<String>, AdminPortError> {
        self.catalog.item_groups(query, limit).await
    }

    async fn warehouses(
        &self,
        query: &str,
        parent: &str,
        limit: usize,
    ) -> Result<Vec<AdminWarehouse>, AdminPortError> {
        self.catalog.warehouses(query, parent, limit).await
    }

    async fn item_group_tree(&self) -> Result<Vec<AdminItemGroup>, AdminPortError> {
        self.catalog.item_group_tree().await
    }

    async fn assigned_supplier_items(
        &self,
        supplier_ref: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        self.admin_store
            .assigned_supplier_items(supplier_ref, limit)
            .await
    }

    async fn customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        match &self.customer_store {
            Some(store) => store.customer_items(customer_ref, query, limit).await,
            None => self
                .admin_store
                .customer_items(customer_ref, query, limit)
                .await,
        }
    }
}

#[async_trait]
impl AdminWritePort for AdminCatalogOverlay {
    async fn create_supplier(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        self.admin_store.create_supplier(name, phone).await
    }

    async fn update_supplier_phone(&self, ref_: &str, phone: &str) -> Result<(), AdminPortError> {
        self.admin_store.update_supplier_phone(ref_, phone).await
    }

    async fn assign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        self.ensure_assignment_item(item_code).await?;
        self.admin_store.assign_supplier_item(ref_, item_code).await
    }

    async fn unassign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        self.admin_store
            .unassign_supplier_item(ref_, item_code)
            .await
    }

    async fn create_customer(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        match &self.customer_store {
            Some(store) => store.create_customer(name, phone).await,
            None => self.admin_store.create_customer(name, phone).await,
        }
    }

    async fn update_customer_phone(&self, ref_: &str, phone: &str) -> Result<(), AdminPortError> {
        match &self.customer_store {
            Some(store) => store.update_customer_phone(ref_, phone).await,
            None => self.admin_store.update_customer_phone(ref_, phone).await,
        }
    }

    async fn update_customer_code(&self, ref_: &str, code: &str) -> Result<(), AdminPortError> {
        self.admin_store.update_customer_code(ref_, code).await
    }

    async fn create_material_taminotchi(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        self.admin_store
            .create_material_taminotchi(name, phone)
            .await
    }

    async fn update_material_taminotchi_phone(
        &self,
        ref_: &str,
        phone: &str,
    ) -> Result<(), AdminPortError> {
        self.admin_store
            .update_material_taminotchi_phone(ref_, phone)
            .await
    }

    async fn update_material_taminotchi_code(
        &self,
        ref_: &str,
        code: &str,
    ) -> Result<(), AdminPortError> {
        self.admin_store
            .update_material_taminotchi_code(ref_, code)
            .await
    }

    async fn assign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        self.ensure_assignment_item(item_code).await?;
        match &self.customer_store {
            Some(store) => store.assign_customer_item(ref_, item_code).await,
            None => self.admin_store.assign_customer_item(ref_, item_code).await,
        }
    }

    async fn unassign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        match &self.customer_store {
            Some(store) => store.unassign_customer_item(ref_, item_code).await,
            None => self
                .admin_store
                .unassign_customer_item(ref_, item_code)
                .await,
        }
    }

    async fn create_item(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
    ) -> Result<SupplierItem, AdminPortError> {
        let item = self
            .catalog
            .create_item(code, name, uom, item_group)
            .await?;
        self.sync_item_to_admin_store(&item).await?;
        Ok(item)
    }

    async fn create_item_group(
        &self,
        name: &str,
        parent: &str,
        is_group: bool,
    ) -> Result<AdminItemGroup, AdminPortError> {
        let group = self
            .catalog
            .create_item_group(name, parent, is_group)
            .await?;
        self.admin_store
            .create_item_group(&group.name, &group.parent_item_group, group.is_group)
            .await?;
        Ok(group)
    }

    async fn move_item_group_parent(
        &self,
        name: &str,
        parent: &str,
    ) -> Result<AdminItemGroup, AdminPortError> {
        self.catalog.move_item_group_parent(name, parent).await
    }

    async fn update_item_group(
        &self,
        item_code: &str,
        item_group: &str,
    ) -> Result<(), AdminPortError> {
        self.catalog.update_item_group(item_code, item_group).await
    }
}

#[async_trait]
impl AdminReadPort for AdminCatalogUnavailable {
    async fn suppliers_page(
        &self,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn supplier_by_ref(&self, _ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn customers_page(
        &self,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn customer_by_ref(&self, _ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn items_page(
        &self,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn items_page_by_group(
        &self,
        _group: &str,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn items_by_codes(
        &self,
        _item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn item_groups(
        &self,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<String>, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn warehouses(
        &self,
        _query: &str,
        _parent: &str,
        _limit: usize,
    ) -> Result<Vec<AdminWarehouse>, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn item_group_tree(&self) -> Result<Vec<AdminItemGroup>, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn assigned_supplier_items(
        &self,
        _supplier_ref: &str,
        _limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn customer_items(
        &self,
        _customer_ref: &str,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }
}

#[async_trait]
impl AdminWritePort for AdminCatalogUnavailable {
    async fn create_supplier(
        &self,
        _name: &str,
        _phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn update_supplier_phone(&self, _ref_: &str, _phone: &str) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn assign_supplier_item(
        &self,
        _ref_: &str,
        _item_code: &str,
    ) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn unassign_supplier_item(
        &self,
        _ref_: &str,
        _item_code: &str,
    ) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn create_customer(
        &self,
        _name: &str,
        _phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn update_customer_phone(&self, _ref_: &str, _phone: &str) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn update_customer_code(&self, _ref_: &str, _code: &str) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn assign_customer_item(
        &self,
        _ref_: &str,
        _item_code: &str,
    ) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn unassign_customer_item(
        &self,
        _ref_: &str,
        _item_code: &str,
    ) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn create_item(
        &self,
        _code: &str,
        _name: &str,
        _uom: &str,
        _item_group: &str,
    ) -> Result<SupplierItem, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn create_item_group(
        &self,
        _name: &str,
        _parent: &str,
        _is_group: bool,
    ) -> Result<AdminItemGroup, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn move_item_group_parent(
        &self,
        _name: &str,
        _parent: &str,
    ) -> Result<AdminItemGroup, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn update_item_group(
        &self,
        _item_code: &str,
        _item_group: &str,
    ) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }
}
