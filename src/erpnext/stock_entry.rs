use async_trait::async_trait;
use reqwest::Method;
use serde::Deserialize;
use serde_json::Value;

use crate::core::gscale::models::{CreateMaterialReceiptDraftInput, MaterialReceiptDraft};
use crate::core::gscale::ports::{GscalePortError, MaterialReceiptErpPort};
use crate::core::rezka::models::{
    CreateRezkaRepackDraftInput, RezkaOutputLabel, RezkaRepackDraft, RezkaSourceEntry,
};
use crate::core::rezka::ports::{RezkaErpPort, RezkaPortError};
use crate::erpnext::client::ErpnextClient;

#[async_trait]
impl MaterialReceiptErpPort for ErpnextClient {
    async fn create_material_receipt_draft(
        &self,
        input: CreateMaterialReceiptDraftInput,
    ) -> Result<MaterialReceiptDraft, GscalePortError> {
        validate_create_input(&input)?;
        let company = self.stock_entry_warehouse_company(&input.warehouse).await?;
        let uom = blank_default(&self.stock_entry_item_uom(&input.item_code).await?, "Kg");
        let item = build_material_receipt_item(&input, &uom);
        let payload = serde_json::json!({
            "stock_entry_type": "Material Receipt",
            "company": company,
            "to_warehouse": input.warehouse.trim(),
            "items": [item],
        });
        let created: ResourceResponse<NameRow> = self
            .stock_entry_json_request(Method::POST, "/api/resource/Stock Entry", Some(payload))
            .await?;
        let name = created.data.name.trim().to_string();
        if name.is_empty() {
            return Err(GscalePortError::ErpWrite(
                "erp stock entry name bo'sh".to_string(),
            ));
        }
        Ok(MaterialReceiptDraft {
            name,
            item_code: input.item_code.trim().to_string(),
            warehouse: input.warehouse.trim().to_string(),
            qty: input.qty,
            uom,
            barcode: input.barcode.trim().to_ascii_uppercase(),
        })
    }

    async fn submit_stock_entry_draft(&self, name: &str) -> Result<(), GscalePortError> {
        let path = stock_entry_path(name)?;
        let mut last_error = None;
        for attempt in 0..2 {
            let latest: ResourceResponse<Value> = self
                .stock_entry_json_request(Method::GET, &path, None)
                .await?;
            let payload = serde_json::json!({ "doc": latest.data });
            match self
                .stock_entry_empty_request(
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
            GscalePortError::ErpWrite("erp stock entry submit failed".to_string())
        }))
    }

    async fn delete_stock_entry_draft(&self, name: &str) -> Result<(), GscalePortError> {
        let path = stock_entry_path(name)?;
        self.stock_entry_empty_request(Method::DELETE, &path, None)
            .await
    }
}

#[async_trait]
impl RezkaErpPort for ErpnextClient {
    async fn create_rezka_repack_draft(
        &self,
        input: CreateRezkaRepackDraftInput,
    ) -> Result<RezkaRepackDraft, RezkaPortError> {
        validate_rezka_repack_input(&input)?;
        let company = if input.source.company.trim().is_empty() {
            self.stock_entry_warehouse_company(&input.source.warehouse)
                .await
                .map_err(rezka_port_error)?
        } else {
            input.source.company.trim().to_string()
        };
        let mut items = Vec::with_capacity(input.outputs.len() + 1);
        items.push(build_rezka_source_item(&input.source));
        for output in &input.outputs {
            let uom = if output.uom.trim().is_empty() {
                blank_default(
                    &self
                        .stock_entry_item_uom(&output.item_code)
                        .await
                        .map_err(rezka_port_error)?,
                    "Kg",
                )
            } else {
                output.uom.trim().to_string()
            };
            items.push(build_rezka_output_item(output, &uom));
        }
        let payload = serde_json::json!({
            "stock_entry_type": "Repack",
            "purpose": "Repack",
            "company": company,
            "from_warehouse": input.source.warehouse.trim(),
            "remarks": build_rezka_remarks(&input),
            "items": items,
        });
        let created: ResourceResponse<NameRow> = self
            .stock_entry_json_request(Method::POST, "/api/resource/Stock Entry", Some(payload))
            .await
            .map_err(rezka_port_error)?;
        let name = created.data.name.trim().to_string();
        if name.is_empty() {
            return Err(RezkaPortError::ErpWrite(
                "erp repack stock entry name bo'sh".to_string(),
            ));
        }
        Ok(RezkaRepackDraft { name })
    }

    async fn submit_rezka_repack_draft(&self, name: &str) -> Result<(), RezkaPortError> {
        <Self as MaterialReceiptErpPort>::submit_stock_entry_draft(self, name)
            .await
            .map_err(rezka_port_error)
    }

    async fn delete_rezka_repack_draft(&self, name: &str) -> Result<(), RezkaPortError> {
        <Self as MaterialReceiptErpPort>::delete_stock_entry_draft(self, name)
            .await
            .map_err(rezka_port_error)
    }
}

impl ErpnextClient {
    async fn stock_entry_warehouse_company(
        &self,
        warehouse: &str,
    ) -> Result<String, GscalePortError> {
        let payload: ResourceResponse<WarehouseRow> = self
            .stock_entry_json_request(
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
            Err(GscalePortError::ErpWrite(
                "warehouse company is not configured".to_string(),
            ))
        } else {
            Ok(company)
        }
    }

    async fn stock_entry_item_uom(&self, item_code: &str) -> Result<String, GscalePortError> {
        let payload: ResourceResponse<ItemRow> = self
            .stock_entry_json_request(
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

    async fn stock_entry_json_request<T: for<'de> Deserialize<'de>>(
        &self,
        method: Method,
        path: &str,
        payload: Option<Value>,
    ) -> Result<T, GscalePortError> {
        let mut request = self
            .http
            .request(method, format!("{}{}", self.base_url(), encoded_path(path)))
            .header(reqwest::header::AUTHORIZATION, self.auth_header().await);
        if let Some(payload) = payload {
            request = request.json(&payload);
        }
        let response = request
            .send()
            .await
            .map_err(|error| GscalePortError::ErpWrite(error.to_string()))?;
        decode_response(response).await
    }

    async fn stock_entry_empty_request(
        &self,
        method: Method,
        path: &str,
        payload: Option<Value>,
    ) -> Result<(), GscalePortError> {
        let mut request = self
            .http
            .request(method, format!("{}{}", self.base_url(), encoded_path(path)))
            .header(reqwest::header::AUTHORIZATION, self.auth_header().await);
        if let Some(payload) = payload {
            request = request.json(&payload);
        }
        let response = request
            .send()
            .await
            .map_err(|error| GscalePortError::ErpWrite(error.to_string()))?;
        decode_empty_response(response).await
    }
}

fn validate_create_input(input: &CreateMaterialReceiptDraftInput) -> Result<(), GscalePortError> {
    if input.item_code.trim().is_empty() || input.warehouse.trim().is_empty() {
        return Err(GscalePortError::InvalidInput(
            "item code and warehouse are required".to_string(),
        ));
    }
    if input.qty <= 0.0 {
        return Err(GscalePortError::InvalidInput(
            "qty must be greater than 0".to_string(),
        ));
    }
    Ok(())
}

fn stock_entry_path(name: &str) -> Result<String, GscalePortError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(GscalePortError::InvalidInput(
            "stock entry name is required".to_string(),
        ));
    }
    Ok(format!(
        "/api/resource/Stock Entry/{}",
        urlencoding::encode(name)
    ))
}

async fn decode_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
) -> Result<T, GscalePortError> {
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| GscalePortError::ErpWrite(error.to_string()))?;
    if !status.is_success() {
        return Err(GscalePortError::ErpWrite(body));
    }
    serde_json::from_str(&body).map_err(|error| GscalePortError::ErpWrite(error.to_string()))
}

async fn decode_empty_response(response: reqwest::Response) -> Result<(), GscalePortError> {
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| GscalePortError::ErpWrite(error.to_string()))?;
    if status.is_success() {
        Ok(())
    } else {
        Err(GscalePortError::ErpWrite(body))
    }
}

fn encoded_path(path: &str) -> String {
    path.trim_start_matches(' ').replace(' ', "%20")
}

fn blank_default(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn build_material_receipt_item(input: &CreateMaterialReceiptDraftInput, uom: &str) -> Value {
    serde_json::json!({
        "item_code": input.item_code.trim(),
        "t_warehouse": input.warehouse.trim(),
        "qty": input.qty,
        "uom": uom,
        "stock_uom": uom,
        "conversion_factor": 1,
        "basic_rate": 1.0,
        "valuation_rate": 1.0,
        "barcode": input.barcode.trim().to_ascii_uppercase(),
    })
}

fn validate_rezka_repack_input(input: &CreateRezkaRepackDraftInput) -> Result<(), RezkaPortError> {
    if input.source.barcode.trim().is_empty()
        || input.source.item_code.trim().is_empty()
        || input.source.warehouse.trim().is_empty()
        || input.source.qty <= 0.0
        || input.outputs.len() < 2
    {
        return Err(RezkaPortError::InvalidInput(
            "source_and_outputs_required".to_string(),
        ));
    }
    for output in &input.outputs {
        if output.item_code.trim().is_empty()
            || output.warehouse.trim().is_empty()
            || output.qty <= 0.0
            || (output.print_qr && output.epc.trim().is_empty())
        {
            return Err(RezkaPortError::InvalidInput(
                "output_item_warehouse_qty_epc_required".to_string(),
            ));
        }
    }
    Ok(())
}

fn build_rezka_source_item(source: &RezkaSourceEntry) -> Value {
    let uom = blank_default(&source.uom, "Kg");
    serde_json::json!({
        "item_code": source.item_code.trim(),
        "s_warehouse": source.warehouse.trim(),
        "qty": source.qty,
        "uom": uom,
        "stock_uom": uom,
        "conversion_factor": 1,
        "barcode": source.barcode.trim().to_ascii_uppercase(),
    })
}

fn build_rezka_output_item(output: &RezkaOutputLabel, uom: &str) -> Value {
    let mut item = serde_json::json!({
        "item_code": output.item_code.trim(),
        "t_warehouse": output.warehouse.trim(),
        "qty": output.qty,
        "uom": uom,
        "stock_uom": uom,
        "conversion_factor": 1,
        "is_finished_item": 1,
        "set_basic_rate_manually": 1,
        "basic_rate": 1.0,
        "valuation_rate": 1.0,
    });
    if output.print_qr {
        item["barcode"] = Value::String(output.epc.trim().to_ascii_uppercase());
    }
    item
}

fn build_rezka_remarks(input: &CreateRezkaRepackDraftInput) -> String {
    let mut parts = vec![
        "Accord mobile Rezka split".to_string(),
        format!("source_barcode={}", input.source.barcode.trim()),
        format!(
            "source_stock_entry={}",
            input.source.stock_entry_name.trim()
        ),
    ];
    let reason = input.reason.trim();
    if !reason.is_empty() {
        parts.push(format!("reason={reason}"));
    }
    for output in &input.outputs {
        let output_reason = output.reason.trim();
        if output_reason.is_empty() {
            continue;
        }
        parts.push(format!(
            "output_reason[{}:{}]={}",
            output.item_code.trim(),
            output.epc.trim(),
            output_reason
        ));
    }
    parts.join(" | ")
}

fn rezka_port_error(error: GscalePortError) -> RezkaPortError {
    match error {
        GscalePortError::InvalidInput(value) => RezkaPortError::InvalidInput(value),
        GscalePortError::ErpWrite(value)
        | GscalePortError::Driver(value)
        | GscalePortError::NotConfigured(value) => RezkaPortError::ErpWrite(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_receipt_item_payload_uses_nonzero_default_rate() {
        let item = build_material_receipt_item(
            &CreateMaterialReceiptDraftInput {
                item_code: " TEST-ITEM ".to_string(),
                warehouse: " Stores - A ".to_string(),
                qty: 5.0,
                barcode: " epc-1 ".to_string(),
            },
            "Kg",
        );

        assert_eq!(item["item_code"], "TEST-ITEM");
        assert_eq!(item["basic_rate"], 1.0);
        assert_eq!(item["valuation_rate"], 1.0);
    }
}

#[derive(Debug, Deserialize)]
struct ResourceResponse<T> {
    data: T,
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
