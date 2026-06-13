use async_trait::async_trait;
use serde::Deserialize;

use crate::core::werka::models::SupplierItem;
use crate::core::werka::ports::{SupplierItemLookup, WerkaPortError};
use crate::erpnext::client::ErpnextClient;

#[async_trait]
impl SupplierItemLookup for ErpnextClient {
    async fn list_assigned_supplier_items(
        &self,
        supplier_ref: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        let limit = if limit == 0 || limit > 200 {
            100
        } else {
            limit
        };
        let supplier_link = self.resolve_supplier_link(supplier_ref).await?;
        let item_codes = self
            .fetch_supplier_item_codes(&supplier_link, limit)
            .await?;
        self.supplier_items_by_codes(&item_codes).await
    }

    async fn get_supplier_items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        self.supplier_items_by_codes(item_codes).await
    }
}

impl ErpnextClient {
    async fn resolve_supplier_link(&self, supplier_ref: &str) -> Result<String, WerkaPortError> {
        let payload: SearchLinkResponse = self
            .supplier_get_json(
                "/api/method/frappe.desk.search.search_link",
                &[
                    ("doctype", "Supplier".to_string()),
                    ("txt", supplier_ref.trim().to_string()),
                    ("page_length", "5".to_string()),
                ],
            )
            .await?;
        let links = payload
            .message
            .into_iter()
            .map(|row| row.value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        if links.is_empty() {
            return Err(WerkaPortError::WriteFailed(format!(
                "supplier not found: {}",
                supplier_ref.trim()
            )));
        }
        let needle = supplier_ref.trim();
        Ok(links
            .iter()
            .find(|value| value.trim().eq_ignore_ascii_case(needle))
            .cloned()
            .unwrap_or_else(|| links[0].clone()))
    }

    async fn fetch_supplier_item_codes(
        &self,
        supplier_link: &str,
        limit: usize,
    ) -> Result<Vec<String>, WerkaPortError> {
        let limit = if limit == 0 { 200 } else { limit.min(500) };
        let filters = serde_json::json!([["supplier", "=", supplier_link.trim()]]);
        let payload: ListResponse<ItemSupplierRow> = self
            .supplier_get_json(
                "/api/resource/Item Supplier",
                &[
                    ("parent", "Item".to_string()),
                    ("fields", r#"["parent"]"#.to_string()),
                    ("filters", filters.to_string()),
                    ("limit_page_length", limit.to_string()),
                ],
            )
            .await?;

        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::with_capacity(payload.data.len());
        for row in payload.data {
            let code = row.parent.trim();
            if !code.is_empty() && seen.insert(code.to_string()) {
                result.push(code.to_string());
            }
        }
        Ok(result)
    }

    async fn supplier_items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        if item_codes.is_empty() {
            return Ok(Vec::new());
        }
        let codes = if item_codes.len() > 500 {
            &item_codes[..500]
        } else {
            item_codes
        };
        let filters = serde_json::json!([
            ["disabled", "=", 0],
            ["is_stock_item", "=", 1],
            ["name", "in", codes],
        ]);
        let payload: ListResponse<ItemRow> = self
            .supplier_get_json(
                "/api/resource/Item",
                &[
                    ("fields", r#"["name","item_name","stock_uom"]"#.to_string()),
                    ("filters", filters.to_string()),
                    ("limit_page_length", codes.len().to_string()),
                ],
            )
            .await?;
        let warehouse = self.resolve_supplier_item_warehouse().await?;

        Ok(payload
            .data
            .into_iter()
            .map(|row| SupplierItem {
                code: row.name.trim().to_string(),
                name: if row.item_name.trim().is_empty() {
                    row.name.trim().to_string()
                } else {
                    row.item_name.trim().to_string()
                },
                uom: row.stock_uom.trim().to_string(),
                warehouse: warehouse.clone(),
                item_group: String::new(),
            })
            .collect())
    }

    async fn resolve_supplier_item_warehouse(&self) -> Result<String, WerkaPortError> {
        if !self.default_warehouse().trim().is_empty() {
            return Ok(self.default_warehouse().trim().to_string());
        }
        let payload: ListResponse<NameRow> = self
            .supplier_get_json(
                "/api/resource/Warehouse",
                &[
                    ("fields", r#"["name"]"#.to_string()),
                    ("limit_page_length", "1".to_string()),
                ],
            )
            .await?;
        payload
            .data
            .into_iter()
            .map(|row| row.name.trim().to_string())
            .find(|name| !name.is_empty())
            .ok_or_else(|| WerkaPortError::WriteFailed("warehouse is not configured".to_string()))
    }

    async fn supplier_get_json<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T, WerkaPortError> {
        let response = self
            .http
            .get(format!("{}{}", self.base_url(), path))
            .header(reqwest::header::AUTHORIZATION, self.auth_header().await)
            .query(query)
            .send()
            .await
            .map_err(|error| WerkaPortError::WriteFailed(error.to_string()))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| WerkaPortError::WriteFailed(error.to_string()))?;
        if !status.is_success() {
            return Err(WerkaPortError::WriteFailed(body));
        }
        serde_json::from_str(&body).map_err(|error| WerkaPortError::WriteFailed(error.to_string()))
    }
}

#[derive(Debug, Deserialize)]
struct SearchLinkResponse {
    #[serde(default)]
    message: Vec<SearchLinkRow>,
}

#[derive(Debug, Deserialize)]
struct SearchLinkRow {
    #[serde(default)]
    value: String,
}

#[derive(Debug, Deserialize)]
struct ListResponse<T> {
    data: Vec<T>,
}

#[derive(Debug, Deserialize)]
struct ItemSupplierRow {
    parent: String,
}

#[derive(Debug, Deserialize)]
struct ItemRow {
    name: String,
    #[serde(default)]
    item_name: String,
    #[serde(default)]
    stock_uom: String,
}

#[derive(Debug, Deserialize)]
struct NameRow {
    name: String,
}
