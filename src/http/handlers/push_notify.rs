use std::collections::HashMap;

use crate::app::AppState;
use crate::core::auth::models::PrincipalRole;
use crate::core::werka::models::{DispatchRecord, WerkaCustomerIssueRecord};

#[allow(clippy::too_many_arguments)]
pub async fn send_dispatch_record(
    state: &AppState,
    key: String,
    title: &str,
    body: &str,
    record: &DispatchRecord,
    target_role: PrincipalRole,
    target_ref: &str,
    context: &'static str,
) {
    let data = dispatch_record_data_for_target(record, target_role, target_ref);
    if let Err(error) = state.push.send_to_key(&key, title, body, data).await {
        tracing::warn!(%error, "push send failed for {context}");
    }
}

pub async fn send_customer_issue(
    state: &AppState,
    record: &WerkaCustomerIssueRecord,
    context: &'static str,
) {
    let dispatch_record = DispatchRecord {
        id: record.entry_id.clone(),
        supplier_ref: record.customer_ref.clone(),
        supplier_name: record.customer_name.clone(),
        item_code: record.item_code.clone(),
        item_name: record.item_name.clone(),
        uom: record.uom.clone(),
        sent_qty: record.qty,
        accepted_qty: 0.0,
        status: "pending".to_string(),
        created_label: record.created_label.clone(),
        ..DispatchRecord::default()
    };
    send_dispatch_record(
        state,
        format!("customer:{}", record.customer_ref.trim()),
        "Werka mahsulot jo'natdi",
        &format!(
            "{} {:.0} {} jo'natildi",
            record.item_code.trim(),
            record.qty,
            record.uom.trim()
        ),
        &dispatch_record,
        PrincipalRole::Customer,
        &record.customer_ref,
        context,
    )
    .await;
}

pub fn dispatch_record_data_for_target(
    record: &DispatchRecord,
    role: PrincipalRole,
    target_ref: &str,
) -> HashMap<String, String> {
    let mut data = dispatch_record_data(record);
    data.insert("target_role".to_string(), role_key(&role).to_string());
    data.insert("target_ref".to_string(), target_ref.trim().to_string());
    data
}

fn dispatch_record_data(record: &DispatchRecord) -> HashMap<String, String> {
    HashMap::from([
        ("id".to_string(), record.id.clone()),
        ("record_type".to_string(), record.record_type.clone()),
        ("supplier_ref".to_string(), record.supplier_ref.clone()),
        ("supplier_name".to_string(), record.supplier_name.clone()),
        ("item_code".to_string(), record.item_code.clone()),
        ("item_name".to_string(), record.item_name.clone()),
        ("uom".to_string(), record.uom.clone()),
        ("sent_qty".to_string(), format!("{:.4}", record.sent_qty)),
        (
            "accepted_qty".to_string(),
            format!("{:.4}", record.accepted_qty),
        ),
        ("amount".to_string(), format!("{:.4}", record.amount)),
        ("currency".to_string(), record.currency.clone()),
        ("note".to_string(), record.note.clone()),
        ("event_type".to_string(), record.event_type.clone()),
        ("highlight".to_string(), record.highlight.clone()),
        ("status".to_string(), record.status.clone()),
        ("created_label".to_string(), record.created_label.clone()),
    ])
}

fn role_key(role: &PrincipalRole) -> &'static str {
    match role {
        PrincipalRole::Supplier => "supplier",
        PrincipalRole::Werka => "werka",
        PrincipalRole::Customer => "customer",
        PrincipalRole::Aparatchi => "aparatchi",
        PrincipalRole::Qolipchi => "qolipchi",
        PrincipalRole::MaterialTaminotchi => "material_taminotchi",
        PrincipalRole::Admin => "admin",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_record_data_matches_go_format() {
        let record = DispatchRecord {
            id: "PR-001".to_string(),
            supplier_ref: "SUP-001".to_string(),
            supplier_name: "Supplier".to_string(),
            item_code: "ITEM-001".to_string(),
            item_name: "Rice".to_string(),
            uom: "Kg".to_string(),
            sent_qty: 10.0,
            accepted_qty: 7.5,
            amount: 12.3,
            status: "partial".to_string(),
            created_label: "Bugun".to_string(),
            ..DispatchRecord::default()
        };

        let data = dispatch_record_data_for_target(&record, PrincipalRole::Supplier, "SUP-001");

        assert_eq!(data["id"], "PR-001");
        assert_eq!(data["sent_qty"], "10.0000");
        assert_eq!(data["accepted_qty"], "7.5000");
        assert_eq!(data["amount"], "12.3000");
        assert_eq!(data["target_role"], "supplier");
        assert_eq!(data["target_ref"], "SUP-001");
    }
}
