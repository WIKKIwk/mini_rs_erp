use std::collections::HashMap;

use async_trait::async_trait;
use mini_rs_erp::core::werka::models::{
    DispatchRecord, SupplierHomeSummary, SupplierItem, SupplierStatusBreakdownEntry,
};
use mini_rs_erp::core::werka::ports::{
    PurchaseReceiptComment, PurchaseReceiptDraft, SupplierItemLookup,
    SupplierPurchaseReceiptLookup, SupplierReadLookup, WerkaPortError,
};

pub(super) struct VerifierSupplierLookup;

#[async_trait]
impl SupplierReadLookup for VerifierSupplierLookup {
    async fn supplier_summary(
        &self,
        supplier_ref: &str,
    ) -> Result<SupplierHomeSummary, WerkaPortError> {
        require_supplier(supplier_ref)?;
        Ok(SupplierHomeSummary {
            pending_count: 2,
            submitted_count: 1,
            returned_count: 3,
        })
    }

    async fn supplier_history(
        &self,
        supplier_ref: &str,
    ) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        require_supplier(supplier_ref)?;
        Ok(vec![DispatchRecord {
            id: "PR-001".to_string(),
            record_type: "purchase_receipt".to_string(),
            supplier_ref: "SUP-001".to_string(),
            supplier_name: "Supplier".to_string(),
            item_code: "ITEM-001".to_string(),
            item_name: "Item".to_string(),
            uom: "Nos".to_string(),
            sent_qty: 5.0,
            accepted_qty: 3.0,
            status: "partial".to_string(),
            created_label: "2026-01-26".to_string(),
            ..DispatchRecord::default()
        }])
    }

    async fn supplier_status_breakdown(
        &self,
        supplier_ref: &str,
        kind: &str,
    ) -> Result<Vec<SupplierStatusBreakdownEntry>, WerkaPortError> {
        require_supplier(supplier_ref)?;
        require(kind == "submitted")?;
        Ok(vec![SupplierStatusBreakdownEntry {
            item_code: "ITEM-001".to_string(),
            item_name: "Item".to_string(),
            receipt_count: 2,
            total_sent_qty: 5.0,
            total_accepted_qty: 5.0,
            total_returned_qty: 0.0,
            uom: "Nos".to_string(),
        }])
    }

    async fn supplier_status_details(
        &self,
        supplier_ref: &str,
        kind: &str,
        item_code: &str,
    ) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        require_supplier(supplier_ref)?;
        require(kind == "submitted" && item_code.eq_ignore_ascii_case("ITEM-001"))?;
        Ok(receipts()
            .into_iter()
            .filter(|receipt| receipt.item_code == "ITEM-001")
            .map(|receipt| DispatchRecord {
                id: receipt.name,
                record_type: "purchase_receipt".to_string(),
                supplier_ref: receipt.supplier,
                supplier_name: receipt.supplier_name,
                item_code: receipt.item_code,
                item_name: receipt.item_name,
                uom: receipt.uom,
                sent_qty: receipt.qty,
                accepted_qty: receipt.qty,
                status: "accepted".to_string(),
                created_label: receipt.posting_date,
                ..DispatchRecord::default()
            })
            .collect())
    }
}

#[async_trait]
impl SupplierItemLookup for VerifierSupplierLookup {
    async fn list_assigned_supplier_items(
        &self,
        supplier_ref: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        require_supplier(supplier_ref)?;
        require(limit == 20)?;
        Ok(vec![
            supplier_item("ITEM-MILK", "Fresh Milk"),
            supplier_item("ITEM-BREAD", "Bread"),
        ])
    }

    async fn get_supplier_items_by_codes(
        &self,
        _item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl SupplierPurchaseReceiptLookup for VerifierSupplierLookup {
    async fn list_supplier_purchase_receipts_page(
        &self,
        supplier_ref: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<PurchaseReceiptDraft>, WerkaPortError> {
        require_supplier(supplier_ref)?;
        require(limit == 200 && offset == 0)?;
        Ok(receipts())
    }

    async fn list_supplier_purchase_receipt_comments_batch(
        &self,
        _names: &[String],
        _limit: usize,
    ) -> Result<HashMap<String, Vec<PurchaseReceiptComment>>, WerkaPortError> {
        Ok(HashMap::new())
    }
}

fn require_supplier(supplier_ref: &str) -> Result<(), WerkaPortError> {
    require(supplier_ref == "SUP-001")
}

fn require(condition: bool) -> Result<(), WerkaPortError> {
    if condition {
        Ok(())
    } else {
        Err(WerkaPortError::LookupFailed)
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

fn receipts() -> Vec<PurchaseReceiptDraft> {
    vec![
        receipt("PR-001", "ITEM-001", "Item A", 3.0),
        receipt("PR-002", "ITEM-001", "Item A", 2.0),
        receipt("PR-003", "ITEM-002", "Item B", 4.0),
    ]
}

fn receipt(name: &str, item_code: &str, item_name: &str, qty: f64) -> PurchaseReceiptDraft {
    PurchaseReceiptDraft {
        name: name.to_string(),
        doc_status: 1,
        status: "Completed".to_string(),
        supplier: "SUP-001".to_string(),
        supplier_name: "Supplier".to_string(),
        posting_date: "2026-01-26".to_string(),
        supplier_delivery_note: format!("TG:+998:20260126090000:{qty:.4}"),
        item_code: item_code.to_string(),
        item_name: item_name.to_string(),
        qty,
        uom: "Nos".to_string(),
        ..PurchaseReceiptDraft::default()
    }
}
