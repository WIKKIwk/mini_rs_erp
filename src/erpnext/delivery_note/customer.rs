use async_trait::async_trait;
use reqwest::Method;
use serde::Deserialize;
use serde_json::Value;

use crate::core::customer::ports::{
    CustomerDeliveryNoteDraft, CustomerDeliveryPort, CustomerPortError,
};
use crate::core::werka::ports::{DeliveryNoteStateUpdate, WerkaPortError};
use crate::erpnext::client::ErpnextClient;

use super::{ListResponse, ResourceResponse, blank_default, custom_fields};

#[async_trait]
impl CustomerDeliveryPort for ErpnextClient {
    async fn list_customer_delivery_notes_page(
        &self,
        customer: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<CustomerDeliveryNoteDraft>, CustomerPortError> {
        custom_fields::ensure_delivery_note_state_fields(self)
            .await
            .map_err(customer_port_error)?;
        let limit = if limit == 0 || limit > 500 {
            100
        } else {
            limit
        };
        let filters = serde_json::json!([["customer", "=", customer.trim()]]);
        let mut query = vec![
            (
                "fields",
                r#"["name","customer","customer_name","posting_date","modified","status","docstatus","accord_flow_state","accord_customer_state","accord_delivery_actor","accord_source_key"]"#.to_string(),
            ),
            ("filters", filters.to_string()),
            ("limit_page_length", limit.to_string()),
            ("order_by", "modified desc".to_string()),
        ];
        if offset > 0 {
            query.push(("limit_start", offset.to_string()));
        }
        let payload: ListResponse<Value> = self
            .get_json("/api/resource/Delivery Note", &query)
            .await
            .map_err(customer_port_error)?;

        let mut items = Vec::with_capacity(payload.data.len());
        for row in payload.data {
            let mut doc = map_customer_delivery_note_draft(&row);
            if doc.item_code.is_empty()
                || doc.item_name.is_empty()
                || doc.qty <= 0.0
                || doc.doc_status == 0
            {
                doc = CustomerDeliveryPort::get_delivery_note(self, &doc.name).await?;
            }
            items.push(doc);
        }
        Ok(items)
    }

    async fn get_delivery_note(
        &self,
        name: &str,
    ) -> Result<CustomerDeliveryNoteDraft, CustomerPortError> {
        let payload: ResourceResponse<Value> = self
            .json_request(
                Method::GET,
                &format!(
                    "/api/resource/Delivery Note/{}",
                    urlencoding::encode(name.trim())
                ),
                None,
            )
            .await
            .map_err(customer_port_error)?;
        Ok(map_customer_delivery_note_draft(&payload.data))
    }

    async fn create_and_submit_delivery_note_return(
        &self,
        source_name: &str,
    ) -> Result<(), CustomerPortError> {
        self.create_and_submit_delivery_note_return_with_qty(source_name, 0.0)
            .await
    }

    async fn create_and_submit_partial_delivery_note_return(
        &self,
        source_name: &str,
        returned_qty: f64,
    ) -> Result<(), CustomerPortError> {
        if returned_qty <= 0.0 {
            return Err(CustomerPortError::Failed(
                "returned qty must be greater than 0".to_string(),
            ));
        }
        self.create_and_submit_delivery_note_return_with_qty(source_name, returned_qty)
            .await
    }

    async fn update_delivery_note_remarks(
        &self,
        name: &str,
        remarks: &str,
    ) -> Result<(), CustomerPortError> {
        self.empty_json_request(
            Method::PUT,
            &format!(
                "/api/resource/Delivery Note/{}",
                urlencoding::encode(name.trim())
            ),
            Some(serde_json::json!({ "remarks": remarks.trim() })),
        )
        .await
        .map_err(customer_port_error)
    }

    async fn update_delivery_note_state(
        &self,
        name: &str,
        update: DeliveryNoteStateUpdate,
    ) -> Result<(), CustomerPortError> {
        custom_fields::ensure_delivery_note_state_fields(self)
            .await
            .map_err(customer_port_error)?;
        self.empty_json_request(
            Method::PUT,
            &format!(
                "/api/resource/Delivery Note/{}",
                urlencoding::encode(name.trim())
            ),
            Some(serde_json::json!({
                "accord_flow_state": update.flow_state.trim(),
                "accord_customer_state": update.customer_state.trim(),
                "accord_customer_reason": update.customer_reason.trim(),
                "accord_delivery_actor": update.delivery_actor.trim(),
                "accord_ui_status": update.ui_status.trim(),
            })),
        )
        .await
        .map_err(customer_port_error)
    }
}

impl ErpnextClient {
    async fn create_and_submit_delivery_note_return_with_qty(
        &self,
        source_name: &str,
        returned_qty: f64,
    ) -> Result<(), CustomerPortError> {
        let mapped: MessageResponse<Value> = self
            .json_request(
                Method::GET,
                &format!(
                    "/api/method/erpnext.stock.doctype.delivery_note.delivery_note.make_sales_return?source_name={}",
                    urlencoding::encode(source_name.trim())
                ),
                None,
            )
            .await
            .map_err(customer_port_error)?;
        let mut mapped_doc = mapped.message;
        if mapped_doc
            .as_object()
            .is_none_or(|object| object.is_empty())
        {
            return Err(CustomerPortError::Failed(
                "delivery note return mapping returned empty document".to_string(),
            ));
        }
        if returned_qty > 0.0 {
            apply_partial_delivery_return_qty(&mut mapped_doc, returned_qty)?;
        }

        let inserted: MessageResponse<Value> = self
            .json_request(
                Method::POST,
                "/api/method/frappe.client.insert",
                Some(serde_json::json!({ "doc": mapped_doc })),
            )
            .await
            .map_err(customer_port_error)?;
        if inserted
            .message
            .as_object()
            .is_none_or(|object| object.is_empty())
        {
            return Err(CustomerPortError::Failed(
                "delivery note return insert returned empty document".to_string(),
            ));
        }
        if string_value(&inserted.message, "name").is_empty() {
            return Err(CustomerPortError::Failed(
                "delivery note return insert did not return name".to_string(),
            ));
        }
        self.empty_json_request(
            Method::POST,
            "/api/method/frappe.client.submit",
            Some(serde_json::json!({ "doc": inserted.message })),
        )
        .await
        .map_err(customer_port_error)
    }
}

fn map_customer_delivery_note_draft(doc: &Value) -> CustomerDeliveryNoteDraft {
    let first_item = doc
        .get("items")
        .and_then(Value::as_array)
        .and_then(|items| items.first());
    let item_code = first_item
        .map(|item| string_value(item, "item_code"))
        .unwrap_or_default();
    let item_name = first_item
        .map(|item| blank_default(&string_value(item, "item_name"), &item_code))
        .unwrap_or_default();
    let uom = first_item
        .map(|item| {
            let uom = string_value(item, "uom");
            if uom.is_empty() {
                string_value(item, "stock_uom")
            } else {
                uom
            }
        })
        .unwrap_or_default();

    CustomerDeliveryNoteDraft {
        name: string_value(doc, "name"),
        customer: string_value(doc, "customer"),
        customer_name: string_value(doc, "customer_name"),
        posting_date: string_value(doc, "posting_date"),
        modified: string_value(doc, "modified"),
        status: string_value(doc, "status"),
        doc_status: float_value(doc, "docstatus") as i32,
        remarks: string_value(doc, "remarks"),
        accord_flow_state: string_value(doc, "accord_flow_state"),
        accord_customer_state: string_value(doc, "accord_customer_state"),
        accord_customer_reason: string_value(doc, "accord_customer_reason"),
        accord_delivery_actor: string_value(doc, "accord_delivery_actor"),
        accord_ui_status: string_value(doc, "accord_ui_status"),
        accord_source_key: string_value(doc, "accord_source_key"),
        item_code,
        item_name,
        qty: first_item
            .map(|item| float_value(item, "qty"))
            .unwrap_or(0.0),
        returned_qty: first_item
            .map(|item| float_value(item, "returned_qty"))
            .unwrap_or(0.0),
        uom,
    }
}

fn apply_partial_delivery_return_qty(
    doc: &mut Value,
    returned_qty: f64,
) -> Result<(), CustomerPortError> {
    let Some(items) = doc.get_mut("items").and_then(Value::as_array_mut) else {
        return Err(CustomerPortError::Failed(
            "delivery note return document has no items".to_string(),
        ));
    };
    let Some(first_item) = items.first_mut().and_then(Value::as_object_mut) else {
        return Err(CustomerPortError::Failed(
            "delivery note return item has invalid shape".to_string(),
        ));
    };
    first_item.insert("qty".to_string(), Value::from(-returned_qty));
    Ok(())
}

fn customer_port_error(error: WerkaPortError) -> CustomerPortError {
    CustomerPortError::Failed(error.to_string())
}

fn string_value(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(value)) => value.trim().to_string(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Bool(value)) => value.to_string(),
        _ => String::new(),
    }
}

fn float_value(value: &Value, key: &str) -> f64 {
    match value.get(key) {
        Some(Value::Number(value)) => value.as_f64().unwrap_or(0.0),
        Some(Value::String(value)) => value.trim().parse::<f64>().unwrap_or(0.0),
        _ => 0.0,
    }
}

#[derive(Debug, Deserialize)]
struct MessageResponse<T> {
    message: T,
}
