use async_trait::async_trait;
use reqwest::Method;
use serde::Deserialize;
use serde_json::Value;
use time::OffsetDateTime;

use crate::core::werka::ports::{
    CreatePurchaseReceiptInput, PurchaseReceiptDraft, WerkaPortError, WerkaSupplierRecord,
    WerkaUnannouncedWriter,
};
use crate::erpnext::client::ErpnextClient;

mod response;
mod supplier_read;

#[async_trait]
impl WerkaUnannouncedWriter for ErpnextClient {
    async fn find_supplier_for_werka(
        &self,
        supplier_ref: &str,
    ) -> Result<WerkaSupplierRecord, WerkaPortError> {
        let payload: ResourceResponse<SupplierRow> = self
            .request_json(
                Method::GET,
                &format!(
                    "/api/resource/Supplier/{}",
                    urlencoding::encode(supplier_ref.trim())
                ),
                None,
            )
            .await?;
        Ok(WerkaSupplierRecord {
            id: payload.data.name.trim().to_string(),
            name: blank_default(&payload.data.supplier_name, payload.data.name.trim()),
            phone: payload.data.mobile_no.trim().to_string(),
        })
    }

    async fn validate_supplier_item_allowed(
        &self,
        supplier_ref: &str,
        item_code: &str,
    ) -> Result<(), WerkaPortError> {
        let filters = serde_json::json!([["supplier", "=", supplier_ref.trim()]]);
        let payload: ListResponse<ItemSupplierRow> = self
            .purchase_get_json(
                "/api/resource/Item Supplier",
                &[
                    ("fields", r#"["parent"]"#.to_string()),
                    ("filters", filters.to_string()),
                    ("limit_page_length", "500".to_string()),
                ],
            )
            .await?;
        let allowed = payload
            .data
            .into_iter()
            .any(|row| row.parent.trim().eq_ignore_ascii_case(item_code.trim()));
        if allowed {
            Ok(())
        } else {
            Err(WerkaPortError::WriteFailed(
                "item supplierga biriktirilmagan".to_string(),
            ))
        }
    }

    async fn resolve_warehouse(&self) -> Result<String, WerkaPortError> {
        if !self.default_warehouse().trim().is_empty() {
            return Ok(self.default_warehouse().trim().to_string());
        }
        let payload: ListResponse<NameRow> = self
            .purchase_get_json(
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

    async fn create_draft_purchase_receipt(
        &self,
        input: CreatePurchaseReceiptInput,
    ) -> Result<PurchaseReceiptDraft, WerkaPortError> {
        validate_purchase_receipt_input(&input)?;
        let company = self.fetch_warehouse_company(&input.warehouse).await?;
        let uom = if input.uom.trim().is_empty() {
            self.item_uom(&input.item_code).await?
        } else {
            input.uom.trim().to_string()
        };
        let uom = blank_default(&uom, "Nos");
        let now = OffsetDateTime::now_utc();
        let payload = serde_json::json!({
            "supplier": input.supplier.trim(),
            "company": company,
            "posting_date": now.date().to_string(),
            "set_warehouse": input.warehouse.trim(),
            "supplier_delivery_note": telegram_receipt_marker(&input.supplier_phone, input.qty, now),
            "items": [{
                "item_code": input.item_code.trim(),
                "warehouse": input.warehouse.trim(),
                "qty": input.qty,
                "received_qty": input.qty,
                "uom": uom,
                "stock_uom": uom,
                "conversion_factor": 1,
                "stock_qty": input.qty,
                "received_stock_qty": input.qty,
                "rate": 0,
                "allow_zero_valuation_rate": 1,
            }],
        });
        let created: ResourceResponse<NameRow> = self
            .request_json(
                Method::POST,
                "/api/resource/Purchase Receipt",
                Some(payload),
            )
            .await?;
        self.get_purchase_receipt(&created.data.name).await
    }

    async fn update_purchase_receipt_remarks(
        &self,
        name: &str,
        remarks: &str,
    ) -> Result<(), WerkaPortError> {
        self.request_empty(
            Method::PUT,
            &format!(
                "/api/resource/Purchase Receipt/{}",
                urlencoding::encode(name.trim())
            ),
            Some(serde_json::json!({ "remarks": remarks.trim() })),
        )
        .await
    }

    async fn add_purchase_receipt_comment(
        &self,
        name: &str,
        content: &str,
    ) -> Result<(), WerkaPortError> {
        if content.trim().is_empty() {
            return Ok(());
        }
        self.request_empty(
            Method::POST,
            "/api/resource/Comment",
            Some(serde_json::json!({
                "comment_type": "Comment",
                "reference_doctype": "Purchase Receipt",
                "reference_name": name.trim(),
                "content": content.trim(),
            })),
        )
        .await
    }
}

impl ErpnextClient {
    async fn fetch_warehouse_company(&self, warehouse: &str) -> Result<String, WerkaPortError> {
        let payload: ResourceResponse<WarehouseRow> = self
            .request_json(
                Method::GET,
                &format!(
                    "/api/resource/Warehouse/{}",
                    urlencoding::encode(warehouse.trim())
                ),
                None,
            )
            .await?;
        let company = payload.data.company.trim().to_string();
        if company.is_empty() {
            Err(WerkaPortError::WriteFailed(
                "warehouse company is not configured".to_string(),
            ))
        } else {
            Ok(company)
        }
    }

    async fn item_uom(&self, item_code: &str) -> Result<String, WerkaPortError> {
        let payload: ResourceResponse<ItemRow> = self
            .request_json(
                Method::GET,
                &format!(
                    "/api/resource/Item/{}",
                    urlencoding::encode(item_code.trim())
                ),
                None,
            )
            .await?;
        Ok(payload.data.stock_uom.trim().to_string())
    }

    async fn get_purchase_receipt(
        &self,
        name: &str,
    ) -> Result<PurchaseReceiptDraft, WerkaPortError> {
        let payload: ResourceResponse<Value> = self
            .request_json(
                Method::GET,
                &format!(
                    "/api/resource/Purchase Receipt/{}",
                    urlencoding::encode(name.trim())
                ),
                None,
            )
            .await?;
        map_purchase_receipt(payload.data)
    }

    async fn purchase_get_json<T: for<'de> Deserialize<'de>>(
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

    async fn request_json<T: for<'de> Deserialize<'de>>(
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

    async fn request_empty(
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

fn validate_purchase_receipt_input(
    input: &CreatePurchaseReceiptInput,
) -> Result<(), WerkaPortError> {
    if input.qty <= 0.0 {
        return Err(WerkaPortError::WriteFailed(
            "qty must be greater than 0".to_string(),
        ));
    }
    if input.item_code.trim().is_empty() {
        return Err(WerkaPortError::WriteFailed(
            "item code is required".to_string(),
        ));
    }
    if input.supplier.trim().is_empty() {
        return Err(WerkaPortError::WriteFailed(
            "supplier is required".to_string(),
        ));
    }
    if input.warehouse.trim().is_empty() {
        return Err(WerkaPortError::WriteFailed(
            "warehouse is required".to_string(),
        ));
    }
    Ok(())
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

fn map_purchase_receipt(doc: Value) -> Result<PurchaseReceiptDraft, WerkaPortError> {
    let first_item = doc
        .get("items")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .ok_or_else(|| WerkaPortError::WriteFailed("purchase receipt has no items".to_string()))?;
    let item_code = string_value(first_item, "item_code");
    let item_name = blank_default(&string_value(first_item, "item_name"), &item_code);
    let uom = blank_default(
        &string_value(first_item, "uom"),
        &string_value(first_item, "stock_uom"),
    );
    Ok(PurchaseReceiptDraft {
        name: string_value(&doc, "name"),
        doc_status: float_value(&doc, "docstatus") as i32,
        status: string_value(&doc, "status"),
        supplier: string_value(&doc, "supplier"),
        supplier_name: string_value(&doc, "supplier_name"),
        posting_date: string_value(&doc, "posting_date"),
        supplier_delivery_note: string_value(&doc, "supplier_delivery_note"),
        item_code,
        item_name,
        qty: float_value(first_item, "qty"),
        uom,
        warehouse: string_value(first_item, "warehouse"),
        amount: float_value(first_item, "amount"),
        currency: string_value(&doc, "currency"),
        remarks: string_value(&doc, "remarks"),
    })
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

fn telegram_receipt_marker(phone: &str, qty: f64, now: OffsetDateTime) -> String {
    let phone = blank_default(phone, "unknown");
    let format = time::format_description::parse("[year][month][day][hour][minute][second]")
        .expect("timestamp format");
    let timestamp = now.format(&format).unwrap_or_default();
    format!("TG:{phone}:{timestamp}:{qty:.4}")
}

fn blank_default(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn encoded_path(path: &str) -> String {
    path.trim_start_matches(' ').replace(' ', "%20")
}

fn request_error(error: reqwest::Error) -> WerkaPortError {
    WerkaPortError::WriteFailed(error.to_string())
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
struct SupplierRow {
    name: String,
    #[serde(default)]
    supplier_name: String,
    #[serde(default)]
    mobile_no: String,
}

#[derive(Debug, Deserialize)]
struct ItemSupplierRow {
    parent: String,
}

#[derive(Debug, Deserialize)]
struct NameRow {
    name: String,
}

#[derive(Debug, Deserialize)]
struct WarehouseRow {
    #[serde(default)]
    company: String,
}

#[derive(Debug, Deserialize)]
struct ItemRow {
    #[serde(default)]
    stock_uom: String,
}
