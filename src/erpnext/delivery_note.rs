use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;

use crate::core::werka::ports::{
    CreateDeliveryNoteInput, DeliveryNoteDraft, DeliveryNoteStateUpdate, ErpItem,
    WerkaCustomerIssueWriter, WerkaPortError,
};
use crate::erpnext::client::ErpnextClient;

mod custom_fields;
mod customer;

#[async_trait]
impl WerkaCustomerIssueWriter for ErpnextClient {
    async fn get_items_by_codes(&self, codes: &[String]) -> Result<Vec<ErpItem>, WerkaPortError> {
        if codes.is_empty() {
            return Ok(Vec::new());
        }
        let filters = serde_json::json!([["name", "in", codes]]);
        let payload: ListResponse<ItemRow> = self
            .get_json(
                "/api/resource/Item",
                &[
                    (
                        "fields",
                        r#"["name","item_name","stock_uom","item_group"]"#.to_string(),
                    ),
                    ("filters", filters.to_string()),
                    ("limit_page_length", codes.len().max(20).to_string()),
                ],
            )
            .await?;

        Ok(payload
            .data
            .into_iter()
            .map(|row| ErpItem {
                code: row.name.trim().to_string(),
                name: if row.item_name.trim().is_empty() {
                    row.name.trim().to_string()
                } else {
                    row.item_name.trim().to_string()
                },
                uom: row.stock_uom.trim().to_string(),
                item_group: row.item_group.trim().to_string(),
            })
            .collect())
    }

    async fn resolve_warehouse(&self) -> Result<String, WerkaPortError> {
        if !self.default_warehouse().trim().is_empty() {
            return Ok(self.default_warehouse().trim().to_string());
        }
        let payload: ListResponse<NameRow> = self
            .get_json(
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

    async fn resolve_company(&self) -> Result<String, WerkaPortError> {
        let payload: ListResponse<NameRow> = self
            .get_json(
                "/api/resource/Company",
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
            .ok_or_else(|| WerkaPortError::WriteFailed("company is not configured".to_string()))
    }

    async fn customer_issue_source_exists_by_scan(
        &self,
        customer_ref: &str,
        marker: &str,
    ) -> Result<bool, WerkaPortError> {
        custom_fields::ensure_delivery_note_state_fields(self).await?;
        let marker = marker.trim();
        if marker.is_empty() {
            return Ok(false);
        }
        let filters = serde_json::json!([
            ["customer", "=", customer_ref.trim()],
            ["docstatus", "<", 2],
        ]);
        let payload: ListResponse<DeliveryNoteDraftRow> = self
            .get_json(
                "/api/resource/Delivery Note",
                &[
                    (
                        "fields",
                        r#"["name","remarks","accord_source_key"]"#.to_string(),
                    ),
                    ("filters", filters.to_string()),
                    ("limit_page_length", "200".to_string()),
                    ("limit_start", "0".to_string()),
                    ("order_by", "name desc".to_string()),
                ],
            )
            .await?;

        Ok(payload
            .data
            .into_iter()
            .map(|row| DeliveryNoteDraft {
                name: row.name.trim().to_string(),
                remarks: row.remarks.trim().to_string(),
                accord_source_key: row.accord_source_key.trim().to_string(),
            })
            .any(|note| {
                note.accord_source_key == marker
                    || (!note.remarks.trim().is_empty() && note.remarks.contains(marker))
            }))
    }

    async fn create_draft_delivery_note(
        &self,
        input: CreateDeliveryNoteInput,
    ) -> Result<String, WerkaPortError> {
        custom_fields::ensure_delivery_note_state_fields(self).await?;
        if input.qty <= 0.0 {
            return Err(WerkaPortError::WriteFailed(
                "qty must be greater than 0".to_string(),
            ));
        }
        if input.customer.trim().is_empty() {
            return Err(WerkaPortError::WriteFailed(
                "customer is required".to_string(),
            ));
        }
        if input.company.trim().is_empty() {
            return Err(WerkaPortError::WriteFailed(
                "company is required".to_string(),
            ));
        }
        if input.warehouse.trim().is_empty() {
            return Err(WerkaPortError::WriteFailed(
                "warehouse is required".to_string(),
            ));
        }
        if input.item_code.trim().is_empty() {
            return Err(WerkaPortError::WriteFailed(
                "item code is required".to_string(),
            ));
        }
        let uom = blank_default(&input.uom, "Nos");
        let mut payload = serde_json::json!({
            "customer": input.customer.trim(),
            "company": input.company.trim(),
            "set_warehouse": input.warehouse.trim(),
            "items": [{
                "item_code": input.item_code.trim(),
                "qty": input.qty,
                "uom": uom,
                "stock_uom": uom,
                "conversion_factor": 1,
                "warehouse": input.warehouse.trim(),
            }],
        });
        if !input.source_key.trim().is_empty() {
            payload["accord_source_key"] = Value::String(input.source_key.trim().to_string());
        }
        let response: ResourceResponse<NameRow> = self
            .json_request(Method::POST, "/api/resource/Delivery Note", Some(payload))
            .await?;
        let name = response.data.name.trim().to_string();
        if name.is_empty() {
            Err(WerkaPortError::WriteFailed(
                "delivery note create response did not return name".to_string(),
            ))
        } else {
            Ok(name)
        }
    }

    async fn update_delivery_note_state(
        &self,
        name: &str,
        update: DeliveryNoteStateUpdate,
    ) -> Result<(), WerkaPortError> {
        custom_fields::ensure_delivery_note_state_fields(self).await?;
        let path = format!(
            "/api/resource/Delivery Note/{}",
            urlencoding::encode(name.trim())
        );
        let payload = serde_json::json!({
            "accord_flow_state": update.flow_state.trim(),
            "accord_customer_state": update.customer_state.trim(),
            "accord_customer_reason": update.customer_reason.trim(),
            "accord_delivery_actor": update.delivery_actor.trim(),
            "accord_ui_status": update.ui_status.trim(),
        });
        self.empty_json_request(Method::PUT, &path, Some(payload))
            .await
    }

    async fn submit_delivery_note(&self, name: &str) -> Result<(), WerkaPortError> {
        let path = format!(
            "/api/resource/Delivery Note/{}",
            urlencoding::encode(name.trim())
        );
        let mut last_error = None;
        for attempt in 0..2 {
            let latest: ResourceResponse<Value> =
                self.json_request(Method::GET, &path, None).await?;
            let payload = serde_json::json!({ "doc": latest.data });
            match self
                .empty_json_request(
                    Method::POST,
                    "/api/method/frappe.client.submit",
                    Some(payload),
                )
                .await
            {
                Ok(()) => return Ok(()),
                Err(error)
                    if attempt == 0 && error.to_string().contains("TimestampMismatchError") =>
                {
                    last_error = Some(error);
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_error.unwrap_or_else(|| {
            WerkaPortError::WriteFailed("delivery note submit failed".to_string())
        }))
    }

    async fn delete_delivery_note(&self, name: &str) -> Result<(), WerkaPortError> {
        let path = format!(
            "/api/resource/Delivery Note/{}",
            urlencoding::encode(name.trim())
        );
        self.empty_json_request(Method::DELETE, &path, None).await
    }
}

impl ErpnextClient {
    async fn get_json<T: DeserializeOwned>(
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

    async fn json_request<T: DeserializeOwned>(
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

    async fn empty_json_request(
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
        decode_empty_response(response).await
    }
}

fn encoded_path(path: &str) -> String {
    path.trim_start_matches(' ').replace(' ', "%20")
}

async fn decode_response<T: DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, WerkaPortError> {
    let status = response.status();
    let body = response.text().await.map_err(request_error)?;
    if !status.is_success() {
        return Err(map_erp_error(body));
    }
    serde_json::from_str(&body).map_err(|error| WerkaPortError::WriteFailed(error.to_string()))
}

async fn decode_empty_response(response: reqwest::Response) -> Result<(), WerkaPortError> {
    let status = response.status();
    let body = response.text().await.map_err(request_error)?;
    if !status.is_success() {
        return Err(map_erp_error(body));
    }
    Ok(())
}

fn map_erp_error(body: String) -> WerkaPortError {
    if body.trim().to_lowercase().contains("negativestockerror") {
        WerkaPortError::InsufficientStock
    } else {
        WerkaPortError::WriteFailed(body)
    }
}

fn request_error(error: reqwest::Error) -> WerkaPortError {
    let text = error.to_string();
    if text.to_lowercase().contains("negativestockerror") {
        WerkaPortError::InsufficientStock
    } else {
        WerkaPortError::WriteFailed(text)
    }
}

fn blank_default(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

#[derive(Debug, Deserialize)]
struct ListResponse<T> {
    data: Vec<T>,
}

#[derive(Debug, Deserialize)]
struct ResourceResponse<T> {
    data: T,
}

#[derive(Debug, Deserialize)]
struct ItemRow {
    name: String,
    #[serde(default)]
    item_name: String,
    #[serde(default)]
    stock_uom: String,
    #[serde(default)]
    item_group: String,
}

#[derive(Debug, Deserialize)]
struct NameRow {
    name: String,
}

#[derive(Debug, Deserialize)]
struct DeliveryNoteDraftRow {
    name: String,
    #[serde(default)]
    remarks: String,
    #[serde(default)]
    accord_source_key: String,
}
