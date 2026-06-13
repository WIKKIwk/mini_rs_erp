use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use super::{ListResponse, map_purchase_receipt};
use crate::core::werka::ports::{
    PurchaseReceiptComment, PurchaseReceiptDraft, SupplierPurchaseReceiptLookup, WerkaPortError,
};
use crate::erpnext::client::ErpnextClient;

#[async_trait]
impl SupplierPurchaseReceiptLookup for ErpnextClient {
    async fn list_supplier_purchase_receipts_page(
        &self,
        supplier_ref: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<PurchaseReceiptDraft>, WerkaPortError> {
        let page_limit = if limit == 0 || limit > 500 {
            100
        } else {
            limit
        };
        let filters = serde_json::json!([
            ["supplier", "=", supplier_ref.trim()],
            ["supplier_delivery_note", "like", "TG:%"],
        ]);
        let mut query = vec![
            (
                "fields",
                r#"["name","supplier","supplier_name","posting_date","supplier_delivery_note","status","docstatus","currency","remarks","items"]"#.to_string(),
            ),
            ("filters", filters.to_string()),
            ("limit_page_length", page_limit.to_string()),
            ("order_by", "modified desc".to_string()),
        ];
        if offset > 0 {
            query.push(("limit_start", offset.to_string()));
        }

        let payload: ListResponse<Value> = self
            .purchase_get_json("/api/resource/Purchase Receipt", &query)
            .await?;
        let mut items = Vec::with_capacity(payload.data.len());
        for row in payload.data {
            match map_purchase_receipt(row.clone()) {
                Ok(draft) => items.push(draft),
                Err(error) => {
                    let name = string_value(&row, "name");
                    if name.is_empty() {
                        return Err(error);
                    }
                    items.push(self.get_purchase_receipt(&name).await?);
                }
            }
        }
        Ok(items)
    }

    async fn list_supplier_purchase_receipt_comments_batch(
        &self,
        names: &[String],
        limit: usize,
    ) -> Result<HashMap<String, Vec<PurchaseReceiptComment>>, WerkaPortError> {
        let limit = if limit == 0 || limit > 100 { 50 } else { limit };
        let mut normalized_names = Vec::with_capacity(names.len());
        let mut seen_names = HashSet::with_capacity(names.len());
        for name in names {
            let trimmed = name.trim();
            if !trimmed.is_empty() && seen_names.insert(trimmed.to_string()) {
                normalized_names.push(trimmed.to_string());
            }
        }
        if normalized_names.is_empty() {
            return Ok(HashMap::new());
        }

        let filters = serde_json::json!([
            ["reference_doctype", "=", "Purchase Receipt"],
            ["reference_name", "in", normalized_names],
            ["comment_type", "=", "Comment"],
        ]);
        let payload: ListResponse<CommentRow> = self
            .purchase_get_json(
                "/api/resource/Comment",
                &[
                    (
                        "fields",
                        r#"["name","content","creation","reference_name"]"#.to_string(),
                    ),
                    ("filters", filters.to_string()),
                    ("order_by", "reference_name asc, creation asc".to_string()),
                    ("limit_page_length", (seen_names.len() * limit).to_string()),
                ],
            )
            .await?;

        let mut items_by_name = HashMap::with_capacity(seen_names.len());
        for row in payload.data {
            let name = row.reference_name.trim();
            if name.is_empty() {
                continue;
            }
            let items: &mut Vec<PurchaseReceiptComment> =
                items_by_name.entry(name.to_string()).or_default();
            if items.len() >= limit {
                continue;
            }
            items.push(PurchaseReceiptComment {
                id: row.name.trim().to_string(),
                content: row.content.trim().to_string(),
                created_at: row.creation.trim().to_string(),
            });
        }
        for name in seen_names {
            items_by_name.entry(name).or_default();
        }
        Ok(items_by_name)
    }
}

fn string_value(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

#[derive(Debug, Deserialize)]
struct CommentRow {
    name: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    creation: String,
    #[serde(default)]
    reference_name: String,
}
