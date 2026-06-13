use super::helpers::*;
use super::*;

use async_trait::async_trait;

impl ErpnextClient {
    async fn admin_item_group_row(&self, name: &str) -> Result<ItemGroupRow, AdminPortError> {
        let response: GetResponse<ItemGroupRow> = self
            .admin_get_json(
                &format!(
                    "/api/resource/Item Group/{}",
                    urlencoding::encode(name.trim())
                ),
                &[(
                    "fields",
                    r#"["name","item_group_name","parent_item_group","is_group","lft","rgt"]"#
                        .to_string(),
                )],
            )
            .await?;
        Ok(response.data)
    }

    async fn promote_item_group(&self, name: &str) -> Result<(), AdminPortError> {
        if name.trim().is_empty() || name.trim() == "All Item Groups" {
            return Ok(());
        }
        self.admin_empty_request(
            reqwest::Method::PUT,
            &format!(
                "/api/resource/Item Group/{}",
                urlencoding::encode(name.trim())
            ),
            serde_json::json!({"is_group": 1}),
        )
        .await
    }

    async fn ensure_item_group_accepts_children(&self, name: &str) -> Result<(), AdminPortError> {
        if name.trim().is_empty() || name.trim() == "All Item Groups" {
            return Ok(());
        }
        let row = self.admin_item_group_row(name).await?;
        if row.is_group == 0 {
            self.promote_item_group(name).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl AdminWritePort for ErpnextClient {
    async fn create_supplier(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        let payload = serde_json::json!({
            "supplier_name": name.trim(),
            "supplier_type": "Company",
            "supplier_group": "Services",
            "mobile_no": phone.trim(),
            "supplier_details": if phone.trim().is_empty() {
                String::new()
            } else {
                format!("Telefon: {}", phone.trim())
            },
        });
        let response: GetResponse<SupplierRow> = self
            .admin_json_request(reqwest::Method::POST, "/api/resource/Supplier", payload)
            .await?;
        Ok(supplier_entry(response.data))
    }

    async fn update_supplier_phone(&self, ref_: &str, phone: &str) -> Result<(), AdminPortError> {
        let current = self.supplier_by_ref(ref_).await?;
        let details = upsert_phone_in_details("", phone);
        let payload = serde_json::json!({
            "mobile_no": phone.trim(),
            "supplier_details": details,
        });
        self.admin_empty_request(
            reqwest::Method::PUT,
            &format!(
                "/api/resource/Supplier/{}",
                urlencoding::encode(&current.ref_)
            ),
            payload,
        )
        .await
    }

    async fn assign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        let payload = serde_json::json!({
            "parent": item_code.trim(),
            "parenttype": "Item",
            "parentfield": "supplier_items",
            "supplier": ref_.trim(),
        });
        self.admin_empty_request(
            reqwest::Method::POST,
            "/api/resource/Item%20Supplier",
            payload,
        )
        .await
    }

    async fn unassign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        let payload: GetResponse<ItemSuppliersRow> = self
            .admin_get_json(
                &format!(
                    "/api/resource/Item/{}",
                    urlencoding::encode(item_code.trim())
                ),
                &[(
                    "fields",
                    r#"["default_supplier","supplier_items"]"#.to_string(),
                )],
            )
            .await?;
        for row in payload.data.supplier_items {
            if row.supplier.trim().eq_ignore_ascii_case(ref_.trim()) && !row.name.trim().is_empty()
            {
                self.admin_empty_request(
                    reqwest::Method::DELETE,
                    &format!(
                        "/api/resource/Item%20Supplier/{}",
                        urlencoding::encode(row.name.trim())
                    ),
                    serde_json::Value::Null,
                )
                .await?;
            }
        }
        if payload
            .data
            .default_supplier
            .trim()
            .eq_ignore_ascii_case(ref_.trim())
        {
            self.admin_empty_request(
                reqwest::Method::PUT,
                &format!(
                    "/api/resource/Item/{}",
                    urlencoding::encode(item_code.trim())
                ),
                serde_json::json!({"default_supplier": ""}),
            )
            .await?;
        }
        Ok(())
    }

    async fn create_customer(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        let payload = serde_json::json!({
            "customer_name": name.trim(),
            "customer_type": "Company",
            "mobile_no": phone.trim(),
        });
        let response: GetResponse<CustomerRow> = self
            .admin_json_request(reqwest::Method::POST, "/api/resource/Customer", payload)
            .await?;
        Ok(customer_entry(response.data))
    }

    async fn update_customer_phone(&self, ref_: &str, phone: &str) -> Result<(), AdminPortError> {
        self.admin_empty_request(
            reqwest::Method::PUT,
            &format!(
                "/api/resource/Customer/{}",
                urlencoding::encode(ref_.trim())
            ),
            serde_json::json!({"customer_details": upsert_phone_in_details("", phone)}),
        )
        .await
    }

    async fn update_customer_code(&self, ref_: &str, code: &str) -> Result<(), AdminPortError> {
        self.admin_empty_request(
            reqwest::Method::PUT,
            &format!(
                "/api/resource/Customer/{}",
                urlencoding::encode(ref_.trim())
            ),
            serde_json::json!({"customer_details": format!("Accord kodi: {}", code.trim())}),
        )
        .await
    }

    async fn assign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        let payload = serde_json::json!({
            "parent": item_code.trim(),
            "parenttype": "Item",
            "parentfield": "customer_items",
            "customer_name": ref_.trim(),
            "ref_code": ref_.trim(),
        });
        self.admin_empty_request(
            reqwest::Method::POST,
            "/api/resource/Item%20Customer%20Detail",
            payload,
        )
        .await
    }

    async fn unassign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        let payload: GetResponse<ItemCustomersRow> = self
            .admin_get_json(
                &format!(
                    "/api/resource/Item/{}",
                    urlencoding::encode(item_code.trim())
                ),
                &[("fields", r#"["customer_items"]"#.to_string())],
            )
            .await?;
        let filtered = payload
            .data
            .customer_items
            .into_iter()
            .filter(|row| !row.customer_name.trim().eq_ignore_ascii_case(ref_.trim()))
            .map(|row| {
                serde_json::json!({
                    "doctype": "Item Customer Detail",
                    "name": row.name.trim(),
                    "customer_name": row.customer_name.trim(),
                    "customer_group": row.customer_group.trim(),
                    "ref_code": row.ref_code.trim(),
                })
            })
            .collect::<Vec<_>>();
        self.admin_empty_request(
            reqwest::Method::PUT,
            &format!(
                "/api/resource/Item/{}",
                urlencoding::encode(item_code.trim())
            ),
            serde_json::json!({"customer_items": filtered}),
        )
        .await
    }

    async fn create_item(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
    ) -> Result<SupplierItem, AdminPortError> {
        let code = code.trim();
        let name = if name.trim().is_empty() {
            code
        } else {
            name.trim()
        };
        let uom = if uom.trim().is_empty() {
            "Nos"
        } else {
            uom.trim()
        };
        let item_group = if item_group.trim().is_empty() {
            "All Item Groups"
        } else {
            item_group.trim()
        };
        let payload = serde_json::json!({
            "item_code": code,
            "item_name": name,
            "stock_uom": uom,
            "is_stock_item": 1,
            "item_group": item_group,
        });
        let response: GetResponse<ItemRow> = self
            .admin_json_request(reqwest::Method::POST, "/api/resource/Item", payload)
            .await?;
        Ok(supplier_item(response.data, &self.default_warehouse()))
    }

    async fn create_item_group(
        &self,
        name: &str,
        parent: &str,
        is_group: bool,
    ) -> Result<AdminItemGroup, AdminPortError> {
        let name = name.trim();
        let parent = if parent.trim().is_empty() {
            "All Item Groups"
        } else {
            parent.trim()
        };
        self.ensure_item_group_accepts_children(parent).await?;
        let payload = serde_json::json!({
            "item_group_name": name,
            "parent_item_group": parent,
            "is_group": if is_group { 1 } else { 0 },
        });
        let response: GetResponse<ItemGroupRow> = self
            .admin_json_request(reqwest::Method::POST, "/api/resource/Item Group", payload)
            .await?;
        Ok(item_group(response.data))
    }

    async fn move_item_group_parent(
        &self,
        name: &str,
        parent: &str,
    ) -> Result<AdminItemGroup, AdminPortError> {
        let parent = if parent.trim().is_empty() {
            "All Item Groups"
        } else {
            parent.trim()
        };
        let current = self.admin_item_group_row(name).await?;
        if current.is_group == 0 && current.rgt > current.lft + 1 {
            self.promote_item_group(name).await?;
        }
        self.ensure_item_group_accepts_children(parent).await?;
        let response: GetResponse<ItemGroupRow> = self
            .admin_json_request(
                reqwest::Method::PUT,
                &format!(
                    "/api/resource/Item Group/{}",
                    urlencoding::encode(name.trim())
                ),
                serde_json::json!({
                    "parent_item_group": parent,
                    "old_parent": current.parent_item_group.trim(),
                }),
            )
            .await?;
        Ok(item_group(response.data))
    }

    async fn update_item_group(
        &self,
        item_code: &str,
        item_group: &str,
    ) -> Result<(), AdminPortError> {
        self.admin_empty_request(
            reqwest::Method::PUT,
            &format!(
                "/api/resource/Item/{}",
                urlencoding::encode(item_code.trim())
            ),
            serde_json::json!({"item_group": item_group.trim()}),
        )
        .await
    }
}
