use std::sync::Arc;

use async_trait::async_trait;

use crate::core::admin::models::{AdminDirectoryEntry, AdminItemGroup, AdminWarehouse};
use crate::core::admin::ports::{AdminPortError, AdminReadPort, AdminWritePort};
use crate::core::werka::models::SupplierItem;
use crate::db::postgres::PostgresConfig;
use crate::db::postgres_admin_catalog::PostgresAdminCatalogStore;
use crate::store::admin_store::JsonAdminStore;

pub(super) fn build_admin_catalog_ports(
    fallback: Arc<JsonAdminStore>,
) -> (Arc<dyn AdminReadPort>, Arc<dyn AdminWritePort>) {
    let config = match PostgresConfig::from_env() {
        Ok(config) => config,
        Err(_) => return (fallback.clone(), fallback),
    };
    match config.pool_options().connect_lazy(&config.database_url) {
        Ok(pool) => {
            tracing::info!("mini ERP postgres item catalog store configured");
            let catalog = Arc::new(PostgresAdminCatalogStore::new(pool));
            spawn_admin_catalog_seed(catalog.clone(), fallback.clone());
            let overlay = Arc::new(AdminCatalogOverlay { fallback, catalog });
            (
                overlay.clone() as Arc<dyn AdminReadPort>,
                overlay as Arc<dyn AdminWritePort>,
            )
        }
        Err(error) => {
            tracing::warn!(%error, "mini ERP postgres item catalog store disabled");
            (fallback.clone(), fallback)
        }
    }
}

fn spawn_admin_catalog_seed(
    catalog: Arc<PostgresAdminCatalogStore>,
    fallback: Arc<JsonAdminStore>,
) {
    tokio::spawn(async move {
        if let Err(error) = catalog.seed_from_read_port(fallback.as_ref()).await {
            tracing::warn!(%error, "mini ERP postgres item catalog seed failed");
        }
    });
}

struct AdminCatalogOverlay {
    fallback: Arc<JsonAdminStore>,
    catalog: Arc<PostgresAdminCatalogStore>,
}

#[async_trait]
impl AdminReadPort for AdminCatalogOverlay {
    async fn suppliers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        self.fallback.suppliers_page(query, limit, offset).await
    }

    async fn supplier_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        self.fallback.supplier_by_ref(ref_).await
    }

    async fn customers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        self.fallback.customers_page(query, limit, offset).await
    }

    async fn customer_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        self.fallback.customer_by_ref(ref_).await
    }

    async fn items_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        self.catalog.items_page(query, limit, offset).await
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
        if parent.trim().is_empty() {
            self.catalog.warehouses(query, parent, limit).await
        } else {
            self.fallback.warehouses(query, parent, limit).await
        }
    }

    async fn item_group_tree(&self) -> Result<Vec<AdminItemGroup>, AdminPortError> {
        self.catalog.item_group_tree().await
    }

    async fn assigned_supplier_items(
        &self,
        supplier_ref: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        self.fallback
            .assigned_supplier_items(supplier_ref, limit)
            .await
    }

    async fn customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        self.fallback
            .customer_items(customer_ref, query, limit)
            .await
    }
}

#[async_trait]
impl AdminWritePort for AdminCatalogOverlay {
    async fn create_supplier(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        self.fallback.create_supplier(name, phone).await
    }

    async fn update_supplier_phone(&self, ref_: &str, phone: &str) -> Result<(), AdminPortError> {
        self.fallback.update_supplier_phone(ref_, phone).await
    }

    async fn assign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        self.fallback.assign_supplier_item(ref_, item_code).await
    }

    async fn unassign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        self.fallback.unassign_supplier_item(ref_, item_code).await
    }

    async fn create_customer(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        self.fallback.create_customer(name, phone).await
    }

    async fn update_customer_phone(&self, ref_: &str, phone: &str) -> Result<(), AdminPortError> {
        self.fallback.update_customer_phone(ref_, phone).await
    }

    async fn update_customer_code(&self, ref_: &str, code: &str) -> Result<(), AdminPortError> {
        self.fallback.update_customer_code(ref_, code).await
    }

    async fn assign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        self.fallback.assign_customer_item(ref_, item_code).await
    }

    async fn unassign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        self.fallback.unassign_customer_item(ref_, item_code).await
    }

    async fn create_item(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
    ) -> Result<SupplierItem, AdminPortError> {
        self.catalog.create_item(code, name, uom, item_group).await
    }

    async fn create_item_group(
        &self,
        name: &str,
        parent: &str,
        is_group: bool,
    ) -> Result<AdminItemGroup, AdminPortError> {
        self.catalog.create_item_group(name, parent, is_group).await
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
