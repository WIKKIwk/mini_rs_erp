use reqwest::Method;
use serde::Deserialize;
use serde_json::Value;

use crate::core::werka::ports::WerkaPortError;
use crate::erpnext::client::ErpnextClient;

use super::ListResponse;

pub(super) async fn ensure_delivery_note_state_fields(
    client: &ErpnextClient,
) -> Result<(), WerkaPortError> {
    if *client.delivery_note_state_fields_ensured.read().await {
        return Ok(());
    }

    let required = required_delivery_note_fields();
    let fieldnames: Vec<_> = required.iter().map(|field| field.fieldname).collect();
    let filters = serde_json::json!([
        ["dt", "=", "Delivery Note"],
        ["fieldname", "in", fieldnames],
    ]);
    let existing: ListResponse<CustomFieldRow> = client
        .get_json(
            "/api/resource/Custom Field",
            &[
                (
                    "fields",
                    r#"["name","fieldname","label","fieldtype","insert_after","hidden","read_only","allow_on_submit","no_copy","options"]"#.to_string(),
                ),
                ("filters", filters.to_string()),
                ("limit_page_length", "20".to_string()),
            ],
        )
        .await?;

    for field in required {
        if let Some(existing_field) = existing
            .data
            .iter()
            .find(|row| row.fieldname.trim() == field.fieldname)
        {
            if custom_field_matches(existing_field, field) {
                continue;
            }
            let path = format!(
                "/api/resource/Custom Field/{}",
                urlencoding::encode(existing_field.name.trim())
            );
            client
                .empty_json_request(Method::PUT, &path, Some(field_payload(field, false)))
                .await?;
        } else if let Err(error) = client
            .empty_json_request(
                Method::POST,
                "/api/resource/Custom Field",
                Some(field_payload(field, true)),
            )
            .await
            && !error.to_string().to_lowercase().contains("duplicate")
        {
            return Err(error);
        }
    }

    *client.delivery_note_state_fields_ensured.write().await = true;
    Ok(())
}

fn required_delivery_note_fields() -> &'static [RequiredCustomField] {
    &[
        RequiredCustomField {
            fieldname: "accord_flow_state",
            label: "Accord Flow State",
            fieldtype: "Int",
            insert_after: "remarks",
            options: "",
            hidden: 1,
        },
        RequiredCustomField {
            fieldname: "accord_customer_state",
            label: "Accord Customer State",
            fieldtype: "Int",
            insert_after: "accord_flow_state",
            options: "",
            hidden: 1,
        },
        RequiredCustomField {
            fieldname: "accord_customer_reason",
            label: "Accord Customer Reason",
            fieldtype: "Small Text",
            insert_after: "accord_customer_state",
            options: "",
            hidden: 1,
        },
        RequiredCustomField {
            fieldname: "accord_delivery_actor",
            label: "Accord Delivery Actor",
            fieldtype: "Data",
            insert_after: "accord_customer_reason",
            options: "",
            hidden: 1,
        },
        RequiredCustomField {
            fieldname: "accord_source_key",
            label: "Accord Source Key",
            fieldtype: "Data",
            insert_after: "accord_delivery_actor",
            options: "",
            hidden: 1,
        },
        RequiredCustomField {
            fieldname: "accord_status_section",
            label: "Accord Status",
            fieldtype: "Section Break",
            insert_after: "posting_time",
            options: "",
            hidden: 0,
        },
        RequiredCustomField {
            fieldname: "accord_ui_status",
            label: "Accord UI Status",
            fieldtype: "Select",
            insert_after: "accord_status_section",
            options: "pending\nconfirm\npartial\nrejected",
            hidden: 0,
        },
    ]
}

fn custom_field_matches(existing: &CustomFieldRow, required: &RequiredCustomField) -> bool {
    existing.label.trim() == required.label
        && existing.fieldtype.trim() == required.fieldtype
        && existing.insert_after.trim() == required.insert_after
        && existing.hidden == required.hidden
        && existing.read_only == 1
        && existing.allow_on_submit == 1
        && existing.no_copy == 1
        && existing.options.trim() == required.options
}

fn field_payload(field: &RequiredCustomField, include_dt: bool) -> Value {
    let mut payload = serde_json::json!({
        "fieldname": field.fieldname,
        "label": field.label,
        "fieldtype": field.fieldtype,
        "insert_after": field.insert_after,
        "hidden": field.hidden,
        "read_only": 1,
        "allow_on_submit": 1,
        "no_copy": 1,
        "options": field.options,
    });
    if include_dt {
        payload["dt"] = Value::String("Delivery Note".to_string());
    }
    payload
}

#[derive(Debug, Clone, Copy)]
struct RequiredCustomField {
    fieldname: &'static str,
    label: &'static str,
    fieldtype: &'static str,
    insert_after: &'static str,
    options: &'static str,
    hidden: i32,
}

#[derive(Debug, Deserialize)]
struct CustomFieldRow {
    name: String,
    fieldname: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    fieldtype: String,
    #[serde(default)]
    insert_after: String,
    #[serde(default)]
    hidden: i32,
    #[serde(default)]
    read_only: i32,
    #[serde(default)]
    allow_on_submit: i32,
    #[serde(default)]
    no_copy: i32,
    #[serde(default)]
    options: String,
}
