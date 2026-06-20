use super::super::helpers::*;
use super::super::*;

use crate::core::admin::models::{AdminItemGroup, AdminWarehouse};

impl AdminService {
    pub async fn items_page_by_group(
        &self,
        group: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        self.read_port()?
            .items_page_by_group(group, query, limit, offset)
            .await
    }

    pub async fn items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        self.read_port()?.items_by_codes(item_codes).await
    }

    pub async fn item_groups(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<String>, AdminPortError> {
        let groups = self.read_port()?.item_groups(query, limit).await?;
        if groups.is_empty() && query.trim().is_empty() {
            Ok(vec!["All Item Groups".to_string()])
        } else {
            Ok(dedupe_strings(groups))
        }
    }

    pub async fn warehouses(
        &self,
        query: &str,
        parent: &str,
        limit: usize,
    ) -> Result<Vec<AdminWarehouse>, AdminPortError> {
        self.read_port()?
            .warehouses(query, normalize_warehouse_parent(parent), limit)
            .await
    }

    pub async fn item_group_tree(&self) -> Result<Vec<AdminItemGroup>, AdminPortError> {
        let groups = self.read_port()?.item_group_tree().await?;
        if groups.is_empty() {
            Ok(vec![AdminItemGroup {
                name: "All Item Groups".to_string(),
                item_group_name: "All Item Groups".to_string(),
                parent_item_group: String::new(),
                is_group: true,
            }])
        } else {
            Ok(groups)
        }
    }
}

fn normalize_warehouse_parent(parent: &str) -> &str {
    if parent.trim().eq_ignore_ascii_case("Aparat") {
        "aparat - A"
    } else {
        parent
    }
}
