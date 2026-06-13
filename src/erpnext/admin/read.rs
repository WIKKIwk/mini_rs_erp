use super::helpers::*;
use super::*;

use async_trait::async_trait;

#[async_trait]
impl AdminReadPort for ErpnextClient {
    async fn suppliers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        let mut params = vec![
            (
                "fields",
                r#"["name","supplier_name","mobile_no","supplier_details"]"#.to_string(),
            ),
            ("filters", r#"[["disabled","=",0]]"#.to_string()),
            (
                "limit_page_length",
                normalize_limit(limit, 20, 500).to_string(),
            ),
        ];
        if offset > 0 {
            params.push(("limit_start", offset.to_string()));
        }
        if !query.trim().is_empty() {
            params.push(("or_filters", supplier_or_filters(query)));
        }
        let payload: ListResponse<SupplierRow> = self
            .admin_get_json("/api/resource/Supplier", &params)
            .await?;
        Ok(payload.data.into_iter().map(supplier_entry).collect())
    }

    async fn supplier_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        let payload: GetResponse<SupplierRow> = self
            .admin_get_json(
                &format!(
                    "/api/resource/Supplier/{}",
                    urlencoding::encode(ref_.trim())
                ),
                &[(
                    "fields",
                    r#"["name","supplier_name","mobile_no","supplier_details"]"#.to_string(),
                )],
            )
            .await?;
        Ok(supplier_entry(payload.data))
    }

    async fn customers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        let mut params = vec![
            (
                "fields",
                r#"["name","customer_name","mobile_no","customer_details"]"#.to_string(),
            ),
            ("filters", r#"[["disabled","=",0]]"#.to_string()),
            (
                "limit_page_length",
                normalize_limit(limit, 20, 500).to_string(),
            ),
            ("order_by", "modified desc".to_string()),
        ];
        if offset > 0 {
            params.push(("limit_start", offset.to_string()));
        }
        if !query.trim().is_empty() {
            params.push(("or_filters", customer_or_filters(query)));
        }
        let payload: ListResponse<CustomerRow> = self
            .admin_get_json("/api/resource/Customer", &params)
            .await?;
        Ok(payload.data.into_iter().map(customer_entry).collect())
    }

    async fn customer_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        let payload: GetResponse<CustomerRow> = self
            .admin_get_json(
                &format!(
                    "/api/resource/Customer/{}",
                    urlencoding::encode(ref_.trim())
                ),
                &[(
                    "fields",
                    r#"["name","customer_name","mobile_no","customer_details"]"#.to_string(),
                )],
            )
            .await?;
        Ok(customer_entry(payload.data))
    }

    async fn items_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        let mut params = vec![
            (
                "fields",
                r#"["name","item_name","stock_uom","item_group"]"#.to_string(),
            ),
            (
                "filters",
                r#"[["disabled","=",0],["is_stock_item","=",1]]"#.to_string(),
            ),
            (
                "limit_page_length",
                normalize_limit(limit, 20, 500).to_string(),
            ),
            ("order_by", "item_name asc, name asc".to_string()),
        ];
        if offset > 0 {
            params.push(("limit_start", offset.to_string()));
        }
        if !query.trim().is_empty() {
            params.push(("or_filters", item_or_filters(query)));
        }
        let payload: ListResponse<ItemRow> =
            self.admin_get_json("/api/resource/Item", &params).await?;
        let warehouse = self.default_warehouse();
        Ok(payload
            .data
            .into_iter()
            .map(|row| supplier_item(row, &warehouse))
            .collect())
    }

    async fn items_page_by_group(
        &self,
        group: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        let group = group.trim();
        if group.is_empty() {
            return self.items_page(query, limit, offset).await;
        }
        let filters = serde_json::json!([
            ["disabled", "=", 0],
            ["is_stock_item", "=", 1],
            ["item_group", "=", group],
        ]);
        let mut params = vec![
            (
                "fields",
                r#"["name","item_name","stock_uom","item_group"]"#.to_string(),
            ),
            ("filters", filters.to_string()),
            (
                "limit_page_length",
                normalize_limit(limit, 20, 500).to_string(),
            ),
            ("order_by", "item_name asc, name asc".to_string()),
        ];
        if offset > 0 {
            params.push(("limit_start", offset.to_string()));
        }
        if !query.trim().is_empty() {
            params.push(("or_filters", item_or_filters(query)));
        }
        let payload: ListResponse<ItemRow> =
            self.admin_get_json("/api/resource/Item", &params).await?;
        let warehouse = self.default_warehouse();
        Ok(payload
            .data
            .into_iter()
            .map(|row| supplier_item(row, &warehouse))
            .collect())
    }

    async fn items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        if item_codes.is_empty() {
            return Ok(Vec::new());
        }
        let codes = item_codes
            .iter()
            .map(|code| code.trim())
            .filter(|code| !code.is_empty())
            .collect::<Vec<_>>();
        let filters = serde_json::json!([
            ["disabled", "=", 0],
            ["is_stock_item", "=", 1],
            ["name", "in", codes],
        ]);
        let payload: ListResponse<ItemRow> = self
            .admin_get_json(
                "/api/resource/Item",
                &[
                    (
                        "fields",
                        r#"["name","item_name","stock_uom","item_group"]"#.to_string(),
                    ),
                    ("filters", filters.to_string()),
                    ("limit_page_length", codes.len().to_string()),
                ],
            )
            .await?;
        let warehouse = self.default_warehouse();
        Ok(payload
            .data
            .into_iter()
            .map(|row| supplier_item(row, &warehouse))
            .collect())
    }

    async fn item_groups(&self, query: &str, limit: usize) -> Result<Vec<String>, AdminPortError> {
        let payload: SearchLinkResponse = self
            .admin_get_json(
                "/api/method/frappe.desk.search.search_link",
                &[
                    ("doctype", "Item Group".to_string()),
                    ("txt", query.trim().to_string()),
                    ("page_length", normalize_limit(limit, 50, 100).to_string()),
                ],
            )
            .await?;
        Ok(payload
            .results
            .into_iter()
            .map(|row| row.value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect())
    }

    async fn warehouses(
        &self,
        query: &str,
        parent: &str,
        limit: usize,
    ) -> Result<Vec<crate::core::admin::models::AdminWarehouse>, AdminPortError> {
        let filters = if parent.trim().is_empty() {
            r#"[["disabled","=",0]]"#.to_string()
        } else {
            serde_json::to_string(&serde_json::json!([
                ["disabled", "=", 0],
                ["parent_warehouse", "=", parent.trim()]
            ]))
            .unwrap_or_else(|_| r#"[["disabled","=",0]]"#.to_string())
        };
        let mut params = vec![
            (
                "fields",
                r#"["name","company","is_group","parent_warehouse"]"#.to_string(),
            ),
            ("filters", filters),
            (
                "limit_page_length",
                normalize_limit(limit, 30, 500).to_string(),
            ),
            ("order_by", "name asc".to_string()),
        ];
        if !query.trim().is_empty() {
            params.push(("or_filters", warehouse_or_filters(query)));
        }
        let payload: ListResponse<WarehouseRow> = self
            .admin_get_json("/api/resource/Warehouse", &params)
            .await?;
        Ok(payload
            .data
            .into_iter()
            .map(warehouse)
            .filter(|item| !item.warehouse.is_empty())
            .collect())
    }

    async fn item_group_tree(&self) -> Result<Vec<AdminItemGroup>, AdminPortError> {
        let payload: ListResponse<ItemGroupRow> = self
            .admin_get_json(
                "/api/resource/Item Group",
                &[
                    (
                        "fields",
                        r#"["name","item_group_name","parent_item_group","is_group","lft","rgt"]"#
                            .to_string(),
                    ),
                    ("limit_page_length", "500".to_string()),
                    ("order_by", "lft asc, name asc".to_string()),
                ],
            )
            .await?;
        Ok(payload
            .data
            .into_iter()
            .map(item_group)
            .filter(|group| !group.name.is_empty())
            .collect())
    }

    async fn assigned_supplier_items(
        &self,
        supplier_ref: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        let filters = serde_json::json!([["supplier", "=", supplier_ref.trim()]]);
        let payload: ListResponse<ItemSupplierRow> = self
            .admin_get_json(
                "/api/resource/Item Supplier",
                &[
                    ("parent", "Item".to_string()),
                    ("fields", r#"["parent"]"#.to_string()),
                    ("filters", filters.to_string()),
                    (
                        "limit_page_length",
                        normalize_limit(limit, 200, 500).to_string(),
                    ),
                ],
            )
            .await?;
        let codes = payload
            .data
            .into_iter()
            .map(|row| row.parent)
            .collect::<Vec<_>>();
        self.items_by_codes(&codes).await
    }

    async fn customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        let payload: GetResponse<CustomerItemsRow> = self
            .admin_get_json(
                &format!(
                    "/api/resource/Customer/{}",
                    urlencoding::encode(customer_ref.trim())
                ),
                &[("fields", r#"["custom_customer_items"]"#.to_string())],
            )
            .await?;
        let needle = query.trim().to_lowercase();
        let codes = payload
            .data
            .custom_customer_items
            .into_iter()
            .map(|row| row.item_code)
            .filter(|code| needle.is_empty() || code.to_lowercase().contains(&needle))
            .take(normalize_limit(limit, 200, 500))
            .collect::<Vec<_>>();
        self.items_by_codes(&codes).await
    }
}
