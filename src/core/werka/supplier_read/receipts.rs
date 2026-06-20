use std::collections::{HashMap, HashSet};

use crate::core::werka::ports::{
    PurchaseReceiptComment, PurchaseReceiptDraft, SupplierPurchaseReceiptLookup, WerkaPortError,
};
use crate::core::werka::unannounced::purchase_receipt_to_dispatch_record;

use super::SUPPLIER_RECEIPT_PAGE_SIZE;
use super::comments::dispatch_record_needs_comment_scan;

pub(super) async fn collect_supplier_purchase_receipts(
    lookup: &dyn SupplierPurchaseReceiptLookup,
    supplier_ref: &str,
) -> Result<Vec<PurchaseReceiptDraft>, WerkaPortError> {
    let mut result = Vec::with_capacity(SUPPLIER_RECEIPT_PAGE_SIZE);
    let mut seen = HashSet::with_capacity(SUPPLIER_RECEIPT_PAGE_SIZE);
    let mut offset = 0;
    loop {
        let items = lookup
            .list_supplier_purchase_receipts_page(supplier_ref, SUPPLIER_RECEIPT_PAGE_SIZE, offset)
            .await?;
        for item in &items {
            let name = item.name.trim();
            if !name.is_empty() && seen.insert(name.to_string()) {
                result.push(item.clone());
            }
        }
        if items.len() < SUPPLIER_RECEIPT_PAGE_SIZE {
            return Ok(result);
        }
        offset += SUPPLIER_RECEIPT_PAGE_SIZE;
    }
}

pub(super) async fn purchase_receipt_comments_by_name(
    lookup: &dyn SupplierPurchaseReceiptLookup,
    receipts: &[PurchaseReceiptDraft],
) -> Result<HashMap<String, Vec<PurchaseReceiptComment>>, WerkaPortError> {
    let mut names = Vec::new();
    let mut seen = HashSet::with_capacity(receipts.len());
    for receipt in receipts {
        let record = purchase_receipt_to_dispatch_record(receipt.clone(), &receipt.supplier_name);
        if !dispatch_record_needs_comment_scan(&record) {
            continue;
        }
        let name = receipt.name.trim();
        if !name.is_empty() && seen.insert(name.to_string()) {
            names.push(name.to_string());
        }
    }
    if names.is_empty() {
        return Ok(HashMap::new());
    }
    lookup
        .list_supplier_purchase_receipt_comments_batch(&names, 100)
        .await
}
