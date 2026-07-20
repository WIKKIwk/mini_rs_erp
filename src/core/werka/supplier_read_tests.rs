use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use super::ports::{
    PurchaseReceiptComment, PurchaseReceiptDraft, SupplierItemLookup,
    SupplierPurchaseReceiptLookup, WerkaPortError,
};
use super::service::WerkaService;
use crate::core::werka::models::SupplierItem;

#[tokio::test]
async fn supplier_summary_counts_receipt_statuses_like_go_fallback() {
    let service = WerkaService::new().with_supplier_purchase_receipt_lookup(std::sync::Arc::new(
        FakeSupplierReceipts {
            calls: Mutex::new(Vec::new()),
            comments_calls: Mutex::new(Vec::new()),
            receipts: vec![
                receipt(
                    "PR-PENDING",
                    0,
                    "To Bill",
                    5.0,
                    "TG:+998:20260126090000:5.0000",
                ),
                receipt("PR-DRAFT", 0, "Draft", 5.0, "TG:+998:20260126090001:5.0000"),
                receipt(
                    "PR-OK",
                    1,
                    "Completed",
                    5.0,
                    "TG:+998:20260126090002:5.0000",
                ),
                receipt(
                    "PR-PARTIAL",
                    1,
                    "Completed",
                    3.0,
                    "TG:+998:20260126090003:5.0000",
                ),
                receipt(
                    "PR-CANCELLED",
                    2,
                    "Cancelled",
                    5.0,
                    "TG:+998:20260126090004:5.0000",
                ),
            ],
        },
    ));

    let summary = service
        .supplier_summary("SUP-001", "Supplier")
        .await
        .expect("summary result")
        .expect("summary");

    assert_eq!(summary.pending_count, 2);
    assert_eq!(summary.submitted_count, 1);
    assert_eq!(summary.returned_count, 2);
}

#[tokio::test]
async fn supplier_history_adds_supplier_ack_note_only_for_records_that_need_scan() {
    let service = WerkaService::new().with_supplier_purchase_receipt_lookup(std::sync::Arc::new(
        FakeSupplierReceipts {
            calls: Mutex::new(Vec::new()),
            comments_calls: Mutex::new(Vec::new()),
            receipts: vec![
                receipt(
                    "PR-CLEAN",
                    0,
                    "To Bill",
                    5.0,
                    "TG:+998:20260126090000:5.0000",
                ),
                receipt(
                    "PR-PARTIAL",
                    1,
                    "Completed",
                    3.0,
                    "TG:+998:20260126090001:5.0000",
                ),
            ],
        },
    ));

    let items = service
        .supplier_history("SUP-001", "Supplier")
        .await
        .expect("history result")
        .expect("history");

    assert_eq!(items.len(), 2);
    assert!(items[0].note.is_empty());
    assert_eq!(
        items[1].note,
        "Supplier tasdiqladi: Tasdiqlayman, shu holat bo‘lganini ko‘rdim."
    );
}

#[tokio::test]
async fn supplier_status_breakdown_groups_filters_and_sorts_like_go() {
    let service = WerkaService::new().with_supplier_purchase_receipt_lookup(std::sync::Arc::new(
        FakeSupplierReceipts {
            calls: Mutex::new(Vec::new()),
            comments_calls: Mutex::new(Vec::new()),
            receipts: vec![
                receipt(
                    "PR-A1",
                    1,
                    "Completed",
                    4.0,
                    "TG:+998:20260126090000:4.0000",
                ),
                receipt(
                    "PR-A2",
                    1,
                    "Completed",
                    2.0,
                    "TG:+998:20260126090001:3.0000",
                ),
                receipt(
                    "PR-B1",
                    1,
                    "Completed",
                    1.0,
                    "TG:+998:20260126090002:1.0000",
                ),
                receipt("PR-C1", 0, "Draft", 5.0, "TG:+998:20260126090003:5.0000"),
            ],
        },
    ));

    let items = service
        .supplier_status_breakdown("SUP-001", "Supplier", "submitted")
        .await
        .expect("breakdown result")
        .expect("breakdown");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].item_code, "ITEM-001");
    assert_eq!(items[0].receipt_count, 2);
    assert_eq!(items[0].total_sent_qty, 5.0);
    assert_eq!(items[0].total_accepted_qty, 5.0);
    assert_eq!(items[0].total_returned_qty, 0.0);
}

#[tokio::test]
async fn supplier_status_details_filters_kind_and_item_code_like_go() {
    let service = WerkaService::new().with_supplier_purchase_receipt_lookup(std::sync::Arc::new(
        FakeSupplierReceipts {
            calls: Mutex::new(Vec::new()),
            comments_calls: Mutex::new(Vec::new()),
            receipts: vec![
                receipt(
                    "PR-A1",
                    1,
                    "Completed",
                    4.0,
                    "TG:+998:20260126090000:4.0000",
                ),
                receipt(
                    "PR-A2",
                    1,
                    "Completed",
                    2.0,
                    "TG:+998:20260126090001:2.0000",
                ),
                receipt("PR-DRAFT", 0, "Draft", 5.0, "TG:+998:20260126090002:5.0000"),
            ],
        },
    ));

    let items = service
        .supplier_status_details("SUP-001", "Supplier", "submitted", "item-001")
        .await
        .expect("details result")
        .expect("details");

    assert_eq!(items.len(), 2);
    assert_eq!(items[0].id, "PR-A1");
    assert_eq!(items[1].id, "PR-A2");
    assert!(items.iter().all(|item| item.status == "accepted"));
}

#[tokio::test]
async fn supplier_items_filters_query_and_limits_like_go() {
    let service = WerkaService::new().with_supplier_item_lookup(std::sync::Arc::new(
        FakeSupplierItemLookup {
            list_error: None,
            assigned: vec![
                supplier_item("ITEM-MILK", "Fresh Milk"),
                supplier_item("ITEM-BREAD", "Bread"),
                supplier_item("ITEM-MILK-2", "Milk 2"),
            ],
            fallback: Vec::new(),
        },
    ));

    let items = service
        .supplier_mobile_items("SUP-001", "milk", 1)
        .await
        .expect("items result")
        .expect("items");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].code, "ITEM-MILK");
}

#[tokio::test]
async fn supplier_items_uses_assigned_codes_on_permission_error_like_go() {
    let service = WerkaService::new()
        .with_supplier_item_lookup(std::sync::Arc::new(FakeSupplierItemLookup {
            list_error: Some("PermissionError: no access".to_string()),
            assigned: Vec::new(),
            fallback: vec![supplier_item("ITEM-FALLBACK", "Fallback")],
        }))
        .with_supplier_admin_state_lookup(std::sync::Arc::new(FakeSupplierState));

    let items = service
        .supplier_mobile_items("SUP-001", "", 20)
        .await
        .expect("items result")
        .expect("items");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].code, "ITEM-FALLBACK");
}

struct FakeSupplierReceipts {
    calls: Mutex<Vec<(usize, usize)>>,
    comments_calls: Mutex<Vec<Vec<String>>>,
    receipts: Vec<PurchaseReceiptDraft>,
}

struct FakeSupplierItemLookup {
    list_error: Option<String>,
    assigned: Vec<SupplierItem>,
    fallback: Vec<SupplierItem>,
}

#[async_trait]
impl SupplierItemLookup for FakeSupplierItemLookup {
    async fn list_assigned_supplier_items(
        &self,
        supplier_ref: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        assert_eq!(supplier_ref, "SUP-001");
        assert!(limit <= 20);
        if let Some(error) = &self.list_error {
            Err(WerkaPortError::WriteFailed(error.clone()))
        } else {
            Ok(self.assigned.clone())
        }
    }

    async fn get_supplier_items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        assert_eq!(item_codes, ["ITEM-FALLBACK"]);
        Ok(self.fallback.clone())
    }
}

struct FakeSupplierState;

#[async_trait]
impl super::ports::WerkaSupplierAdminStateLookup for FakeSupplierState {
    async fn werka_supplier_admin_state(
        &self,
        supplier_ref: &str,
    ) -> Result<super::ports::WerkaSupplierAdminState, WerkaPortError> {
        assert_eq!(supplier_ref, "SUP-001");
        Ok(super::ports::WerkaSupplierAdminState {
            assigned_item_codes: vec!["ITEM-FALLBACK".to_string()],
            ..Default::default()
        })
    }
}

#[async_trait]
impl SupplierPurchaseReceiptLookup for FakeSupplierReceipts {
    async fn list_supplier_purchase_receipts_page(
        &self,
        _supplier_ref: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<PurchaseReceiptDraft>, WerkaPortError> {
        self.calls.lock().expect("calls").push((limit, offset));
        Ok(self.receipts.clone())
    }

    async fn list_supplier_purchase_receipt_comments_batch(
        &self,
        names: &[String],
        _limit: usize,
    ) -> Result<HashMap<String, Vec<PurchaseReceiptComment>>, WerkaPortError> {
        self.comments_calls
            .lock()
            .expect("comments calls")
            .push(names.to_vec());
        let mut result = HashMap::new();
        for name in names {
            result.insert(
                name.clone(),
                vec![PurchaseReceiptComment {
                    id: "COMM-001".to_string(),
                    content: "Supplier • Supplier\nTasdiqlayman, shu holat bo‘lganini ko‘rdim."
                        .to_string(),
                    created_at: "2026-01-26 09:00:00".to_string(),
                }],
            );
        }
        Ok(result)
    }
}

fn receipt(
    name: &str,
    doc_status: i32,
    status: &str,
    qty: f64,
    marker: &str,
) -> PurchaseReceiptDraft {
    PurchaseReceiptDraft {
        name: name.to_string(),
        doc_status,
        status: status.to_string(),
        supplier: "SUP-001".to_string(),
        supplier_name: "Supplier".to_string(),
        posting_date: "2026-01-26".to_string(),
        supplier_delivery_note: marker.to_string(),
        item_code: "ITEM-001".to_string(),
        item_name: "Item".to_string(),
        qty,
        uom: "Nos".to_string(),
        ..PurchaseReceiptDraft::default()
    }
}

fn supplier_item(code: &str, name: &str) -> SupplierItem {
    SupplierItem {
        code: code.to_string(),
        name: name.to_string(),
        uom: "Nos".to_string(),
        warehouse: "Stores - CH".to_string(),
        item_group: String::new(),
        customer_names: Vec::new(),
    }
}
