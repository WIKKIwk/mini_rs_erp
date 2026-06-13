use async_trait::async_trait;
use reqwest::Method;
use serde::Deserialize;
use serde_json::Value;

use crate::core::werka::ports::{
    DeliveryNoteNotificationDraft, NotificationDetailWriter, PurchaseReceiptComment,
    PurchaseReceiptDraft, SupplierUnannouncedWriter, WerkaPortError,
};
use crate::erpnext::client::ErpnextClient;

#[async_trait]
impl NotificationDetailWriter for ErpnextClient {
    async fn get_notification_purchase_receipt(
        &self,
        name: &str,
    ) -> Result<PurchaseReceiptDraft, WerkaPortError> {
        SupplierUnannouncedWriter::get_purchase_receipt(self, name).await
    }

    async fn list_notification_purchase_receipt_comments(
        &self,
        name: &str,
        limit: usize,
    ) -> Result<Vec<PurchaseReceiptComment>, WerkaPortError> {
        SupplierUnannouncedWriter::list_purchase_receipt_comments(self, name, limit).await
    }

    async fn get_notification_delivery_note(
        &self,
        name: &str,
    ) -> Result<DeliveryNoteNotificationDraft, WerkaPortError> {
        let payload: ResourceResponse<Value> = self
            .notification_request_json(
                Method::GET,
                &format!(
                    "/api/resource/Delivery Note/{}",
                    urlencoding::encode(name.trim())
                ),
                None,
            )
            .await?;
        map_delivery_note(payload.data)
    }

    async fn list_notification_delivery_note_comments(
        &self,
        name: &str,
        limit: usize,
    ) -> Result<Vec<PurchaseReceiptComment>, WerkaPortError> {
        let limit = if limit == 0 || limit > 100 { 50 } else { limit };
        let filters = serde_json::json!([
            ["reference_doctype", "=", "Delivery Note"],
            ["reference_name", "in", [name.trim()]],
            ["comment_type", "=", "Comment"]
        ]);
        let payload: ListResponse<CommentRow> = self
            .notification_get_json(
                "/api/resource/Comment",
                &[
                    (
                        "fields",
                        r#"["name","content","creation","reference_name"]"#.to_string(),
                    ),
                    ("filters", filters.to_string()),
                    ("order_by", "reference_name asc, creation asc".to_string()),
                    ("limit_page_length", limit.to_string()),
                ],
            )
            .await?;
        Ok(payload
            .data
            .into_iter()
            .filter(|row| row.reference_name.trim() == name.trim())
            .map(|row| PurchaseReceiptComment {
                id: row.name.trim().to_string(),
                content: row.content.trim().to_string(),
                created_at: row.creation.trim().to_string(),
            })
            .collect())
    }

    async fn add_notification_purchase_receipt_comment(
        &self,
        name: &str,
        content: &str,
    ) -> Result<(), WerkaPortError> {
        SupplierUnannouncedWriter::add_purchase_receipt_comment(self, name, content).await
    }

    async fn update_notification_purchase_receipt_remarks(
        &self,
        name: &str,
        remarks: &str,
    ) -> Result<(), WerkaPortError> {
        SupplierUnannouncedWriter::update_purchase_receipt_remarks(self, name, remarks).await
    }

    async fn add_notification_delivery_note_comment(
        &self,
        name: &str,
        content: &str,
    ) -> Result<(), WerkaPortError> {
        if content.trim().is_empty() {
            return Ok(());
        }
        self.notification_request_empty(
            Method::POST,
            "/api/resource/Comment",
            Some(serde_json::json!({
                "comment_type": "Comment",
                "reference_doctype": "Delivery Note",
                "reference_name": name.trim(),
                "content": content.trim(),
            })),
        )
        .await
    }
}

impl ErpnextClient {
    async fn notification_get_json<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T, WerkaPortError> {
        let response = self
            .http
            .get(format!("{}{}", self.base_url(), encoded_path(path)))
            .header(reqwest::header::AUTHORIZATION, self.auth_header().await)
            .query(query)
            .send()
            .await
            .map_err(request_error)?;
        decode_response(response).await
    }

    async fn notification_request_json<T: for<'de> Deserialize<'de>>(
        &self,
        method: Method,
        path: &str,
        payload: Option<Value>,
    ) -> Result<T, WerkaPortError> {
        let mut request = self
            .http
            .request(method, format!("{}{}", self.base_url(), encoded_path(path)))
            .header(reqwest::header::AUTHORIZATION, self.auth_header().await);
        if let Some(payload) = payload {
            request = request.json(&payload);
        }
        let response = request.send().await.map_err(request_error)?;
        decode_response(response).await
    }

    async fn notification_request_empty(
        &self,
        method: Method,
        path: &str,
        payload: Option<Value>,
    ) -> Result<(), WerkaPortError> {
        let mut request = self
            .http
            .request(method, format!("{}{}", self.base_url(), encoded_path(path)))
            .header(reqwest::header::AUTHORIZATION, self.auth_header().await);
        if let Some(payload) = payload {
            request = request.json(&payload);
        }
        let response = request.send().await.map_err(request_error)?;
        let status = response.status();
        let body = response.text().await.map_err(request_error)?;
        if status.is_success() {
            Ok(())
        } else {
            Err(WerkaPortError::WriteFailed(body))
        }
    }
}

fn map_delivery_note(doc: Value) -> Result<DeliveryNoteNotificationDraft, WerkaPortError> {
    let first_item = doc
        .get("items")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .ok_or_else(|| WerkaPortError::WriteFailed("delivery note has no items".to_string()))?;
    let item_code = string_value(first_item, "item_code");
    Ok(DeliveryNoteNotificationDraft {
        name: string_value(&doc, "name"),
        customer: string_value(&doc, "customer"),
        customer_name: string_value(&doc, "customer_name"),
        doc_status: float_value(&doc, "docstatus") as i32,
        modified: string_value(&doc, "modified"),
        posting_date: string_value(&doc, "posting_date"),
        qty: float_value(first_item, "qty"),
        returned_qty: float_value(first_item, "returned_qty"),
        accord_customer_reason: string_value(&doc, "accord_customer_reason"),
        item_code: item_code.clone(),
        item_name: blank_default(&string_value(first_item, "item_name"), &item_code),
        uom: string_value(first_item, "uom"),
        accord_flow_state: float_value(&doc, "accord_flow_state") as i32,
        accord_customer_state: float_value(&doc, "accord_customer_state") as i32,
        remarks: string_value(&doc, "remarks"),
    })
}

async fn decode_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
) -> Result<T, WerkaPortError> {
    let status = response.status();
    let body = response.text().await.map_err(request_error)?;
    if !status.is_success() {
        return Err(WerkaPortError::WriteFailed(body));
    }
    serde_json::from_str(&body).map_err(|error| WerkaPortError::WriteFailed(error.to_string()))
}

fn string_value(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

fn float_value(value: &Value, key: &str) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn blank_default(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.trim().to_string()
    }
}

fn encoded_path(path: &str) -> String {
    path.trim_start_matches(' ').replace(' ', "%20")
}

fn request_error(error: reqwest::Error) -> WerkaPortError {
    WerkaPortError::WriteFailed(error.to_string())
}

#[derive(Debug, Deserialize)]
struct ResourceResponse<T> {
    data: T,
}

#[derive(Debug, Deserialize)]
struct ListResponse<T> {
    data: Vec<T>,
}

#[derive(Debug, Deserialize)]
struct CommentRow {
    name: String,
    content: String,
    creation: String,
    reference_name: String,
}
