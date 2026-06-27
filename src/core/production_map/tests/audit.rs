use std::sync::Arc;

use crate::core::production_map::*;

use super::fixtures::apparatus_stage_map;

#[tokio::test]
async fn production_workflow_audit_reports_duplicate_qr_and_unknown_order_batch() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = ProductionMapService::new(store.clone());
    store
        .put_map(apparatus_stage_map("zakaz-audit-ok", "7 ta rangli pechat"))
        .await
        .expect("map");

    let first = audit_test_batch("batch-1", "zakaz-audit-ok", "DUPLICATE-QR");
    let second = audit_test_batch("batch-2", "zakaz-audit-ok", "DUPLICATE-QR");
    let orphan = audit_test_batch("batch-3", "missing-order", "ORPHAN-QR");
    store
        .put_order_progress_batch(first)
        .await
        .expect("first batch");
    store
        .put_order_progress_batch(second)
        .await
        .expect("second batch");
    store
        .put_order_progress_batch(orphan)
        .await
        .expect("orphan batch");

    let report = service.audit_production_workflow().await.expect("audit");
    assert!(!report.ok);
    assert!(
        report
            .violations
            .iter()
            .any(|violation| violation.code == "duplicate_qr_payload"
                && violation.order_id == "zakaz-audit-ok"
                && violation.subject == "DUPLICATE-QR")
    );
    assert!(
        report
            .violations
            .iter()
            .any(|violation| violation.code == "unknown_order_progress_batch"
                && violation.order_id == "missing-order"
                && violation.subject == "batch-3")
    );
}

fn audit_test_batch(batch_id: &str, order_id: &str, qr_payload: &str) -> OrderProgressBatch {
    OrderProgressBatch {
        batch_id: batch_id.to_string(),
        session_id: format!("session-{batch_id}"),
        apparatus: "7 ta rangli pechat".to_string(),
        order_id: order_id.to_string(),
        action: queue_state::ApparatusQueueAction::Pause,
        status: OrderProgressBatchStatus::Paused,
        produced_qty: 1.0,
        uom: "m".to_string(),
        qr_payload: qr_payload.to_string(),
        label_item_code: order_id.to_string(),
        label_item_name: order_id.to_string(),
        executor_name: "Worker".to_string(),
        worker_role: "aparatchi".to_string(),
        worker_ref: "worker-audit".to_string(),
        worker_display_name: "Worker Audit".to_string(),
        wip_status: OrderProgressBatchWipStatus::Waiting,
        status_detail: OrderProgressBatchStatusDetail::default(),
        current_apparatus: "7 ta rangli pechat".to_string(),
        current_apparatus_key: queue_state::apparatus_search_key("7 ta rangli pechat"),
        current_location: "7 ta rangli pechat".to_string(),
        next_apparatus: "Laminatsiya 1".to_string(),
        parent_batch_id: String::new(),
        used_by_session_id: String::new(),
        used_by_apparatus: String::new(),
        processed_by_session_id: String::new(),
        processed_by_apparatus: String::new(),
        return_ink_kg: None,
        lamination_print_leftover_rolls: None,
        lamination_film_leftover_rolls: None,
        rezka_bosma_waste: None,
        rezka_lamination_waste: None,
        rezka_edge_waste: None,
        total_waste: None,
        finished_goods_kg: None,
        finished_goods_meter: None,
        description: String::new(),
        payload_json: serde_json::json!({}),
    }
}
