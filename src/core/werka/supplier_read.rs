use std::collections::{HashMap, HashSet};

use crate::core::werka::models::{
    DispatchRecord, SupplierHomeSummary, SupplierItem, SupplierStatusBreakdownEntry,
};
use crate::core::werka::ports::{
    PurchaseReceiptComment, PurchaseReceiptDraft, SupplierPurchaseReceiptLookup, WerkaPortError,
};
use crate::core::werka::service::WerkaService;
use crate::core::werka::unannounced::{
    item_supplier_permission_denied, purchase_receipt_to_dispatch_record,
};

const SUPPLIER_RECEIPT_PAGE_SIZE: usize = 200;

impl WerkaService {
    pub async fn supplier_summary(
        &self,
        supplier_ref: &str,
        supplier_display_name: &str,
    ) -> Result<Option<SupplierHomeSummary>, WerkaPortError> {
        if let Some(lookup) = &self.supplier_read_lookup {
            return lookup.supplier_summary(supplier_ref).await.map(Some);
        }
        let Some(lookup) = &self.supplier_purchase_receipt_lookup else {
            return Ok(None);
        };

        let receipts = collect_supplier_purchase_receipts(lookup.as_ref(), supplier_ref).await?;
        Ok(Some(build_supplier_summary_from_receipts(
            receipts,
            supplier_display_name,
        )))
    }

    pub async fn supplier_history(
        &self,
        supplier_ref: &str,
        supplier_display_name: &str,
    ) -> Result<Option<Vec<DispatchRecord>>, WerkaPortError> {
        if let Some(lookup) = &self.supplier_read_lookup {
            return lookup.supplier_history(supplier_ref).await.map(Some);
        }
        let Some(lookup) = &self.supplier_purchase_receipt_lookup else {
            return Ok(None);
        };

        let receipts = collect_supplier_purchase_receipts(lookup.as_ref(), supplier_ref).await?;
        let comments_by_receipt =
            purchase_receipt_comments_by_name(lookup.as_ref(), &receipts).await?;
        Ok(Some(build_supplier_history_from_receipts(
            receipts,
            &comments_by_receipt,
            supplier_display_name,
        )))
    }

    pub async fn supplier_status_breakdown(
        &self,
        supplier_ref: &str,
        supplier_display_name: &str,
        kind: &str,
    ) -> Result<Option<Vec<SupplierStatusBreakdownEntry>>, WerkaPortError> {
        if let Some(lookup) = &self.supplier_read_lookup {
            return lookup
                .supplier_status_breakdown(supplier_ref, kind)
                .await
                .map(Some);
        }
        let Some(lookup) = &self.supplier_purchase_receipt_lookup else {
            return Ok(None);
        };

        let receipts = collect_supplier_purchase_receipts(lookup.as_ref(), supplier_ref).await?;
        Ok(Some(build_supplier_status_breakdown_from_receipts(
            receipts,
            supplier_display_name,
            kind,
        )))
    }

    pub async fn supplier_status_details(
        &self,
        supplier_ref: &str,
        supplier_display_name: &str,
        kind: &str,
        item_code: &str,
    ) -> Result<Option<Vec<DispatchRecord>>, WerkaPortError> {
        if let Some(lookup) = &self.supplier_read_lookup {
            return lookup
                .supplier_status_details(supplier_ref, kind, item_code)
                .await
                .map(Some);
        }
        let Some(lookup) = &self.supplier_purchase_receipt_lookup else {
            return Ok(None);
        };

        let receipts = collect_supplier_purchase_receipts(lookup.as_ref(), supplier_ref).await?;
        Ok(Some(build_supplier_status_details_from_receipts(
            receipts,
            supplier_display_name,
            kind,
            item_code,
        )))
    }

    pub async fn supplier_mobile_items(
        &self,
        supplier_ref: &str,
        query: &str,
        limit: usize,
    ) -> Result<Option<Vec<SupplierItem>>, WerkaPortError> {
        let state = if let Some(lookup) = &self.supplier_admin_state_lookup {
            lookup.werka_supplier_admin_state(supplier_ref).await?
        } else {
            Default::default()
        };
        if state.removed || state.blocked {
            return Ok(Some(Vec::new()));
        }
        if self.lookup.is_none() && self.supplier_item_lookup.is_none() {
            return Ok(None);
        }

        let mut items = self
            .admin_assigned_items(supplier_ref, &state.assigned_item_codes, limit)
            .await?;
        if !query.trim().is_empty() {
            items = filter_supplier_items_by_query(items, query);
        }
        if limit > 0 && items.len() > limit {
            items.truncate(limit);
        }
        Ok(Some(items))
    }

    async fn admin_assigned_items(
        &self,
        supplier_ref: &str,
        assigned_item_codes: &[String],
        limit: usize,
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        if let Some(lookup) = &self.lookup {
            let mut result = Vec::with_capacity(200);
            let mut offset = 0;
            loop {
                let page_limit = if limit > 0 {
                    let remaining = limit.saturating_sub(result.len());
                    if remaining == 0 {
                        break;
                    }
                    remaining.min(200)
                } else {
                    200
                };
                match lookup
                    .werka_supplier_items(supplier_ref, "", page_limit, offset)
                    .await
                {
                    Ok(page) => {
                        let page_len = page.len();
                        result.extend(page);
                        if page_len < page_limit {
                            return Ok(limit_supplier_items(result, limit));
                        }
                        if limit > 0 && result.len() >= limit {
                            return Ok(limit_supplier_items(result, limit));
                        }
                        offset += page_limit;
                    }
                    Err(_) => break,
                }
            }
            if !result.is_empty() {
                return Ok(limit_supplier_items(result, limit));
            }
        }

        let Some(lookup) = &self.supplier_item_lookup else {
            return Ok(Vec::new());
        };
        match lookup
            .list_assigned_supplier_items(supplier_ref, limit)
            .await
        {
            Ok(items) => Ok(items),
            Err(error) if item_supplier_permission_denied(&error) => {
                if assigned_item_codes.is_empty() {
                    Ok(Vec::new())
                } else {
                    lookup
                        .get_supplier_items_by_codes(assigned_item_codes)
                        .await
                }
            }
            Err(error) => Err(error),
        }
    }
}

async fn collect_supplier_purchase_receipts(
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

async fn purchase_receipt_comments_by_name(
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

fn build_supplier_summary_from_receipts(
    receipts: Vec<PurchaseReceiptDraft>,
    supplier_display_name: &str,
) -> SupplierHomeSummary {
    let mut summary = SupplierHomeSummary::default();
    for receipt in receipts {
        let record = purchase_receipt_to_dispatch_record(receipt, supplier_display_name);
        match record.status.as_str() {
            "pending" | "draft" => summary.pending_count += 1,
            "accepted" => summary.submitted_count += 1,
            "partial" | "rejected" | "cancelled" => summary.returned_count += 1,
            _ => {}
        }
    }
    summary
}

fn build_supplier_history_from_receipts(
    receipts: Vec<PurchaseReceiptDraft>,
    comments_by_receipt: &HashMap<String, Vec<PurchaseReceiptComment>>,
    supplier_display_name: &str,
) -> Vec<DispatchRecord> {
    receipts
        .into_iter()
        .map(|receipt| {
            let mut record =
                purchase_receipt_to_dispatch_record(receipt.clone(), supplier_display_name);
            for comment in comments_by_receipt
                .get(receipt.name.trim())
                .into_iter()
                .flatten()
            {
                if !is_supplier_acknowledgment_comment(&comment.content) {
                    continue;
                }
                if !record.note.contains("Supplier tasdiqladi:") {
                    if !record.note.trim().is_empty() {
                        record.note.push('\n');
                    }
                    record.note.push_str(
                        "Supplier tasdiqladi: Tasdiqlayman, shu holat bo‘lganini ko‘rdim.",
                    );
                }
                break;
            }
            record
        })
        .collect()
}

fn build_supplier_status_breakdown_from_receipts(
    receipts: Vec<PurchaseReceiptDraft>,
    supplier_display_name: &str,
    kind: &str,
) -> Vec<SupplierStatusBreakdownEntry> {
    let mut grouped = HashMap::<String, SupplierStatusBreakdownEntry>::new();
    for receipt in receipts {
        let record = purchase_receipt_to_dispatch_record(receipt, supplier_display_name);
        if !record_matches_supplier_breakdown(&record, kind) {
            continue;
        }
        let key = if record.item_code.trim().is_empty() {
            record.item_name.trim().to_string()
        } else {
            record.item_code.trim().to_string()
        };
        let entry = grouped
            .entry(key)
            .or_insert_with(|| SupplierStatusBreakdownEntry {
                item_code: record.item_code.clone(),
                item_name: record.item_name.clone(),
                uom: record.uom.clone(),
                ..SupplierStatusBreakdownEntry::default()
            });
        entry.receipt_count += 1;
        entry.total_sent_qty += record.sent_qty;
        entry.total_accepted_qty += record.accepted_qty;
        entry.total_returned_qty += (record.sent_qty - record.accepted_qty).max(0.0);
        if entry.uom.trim().is_empty() {
            entry.uom = record.uom;
        }
    }

    let mut result = grouped.into_values().collect::<Vec<_>>();
    result.sort_by(|left, right| {
        right.receipt_count.cmp(&left.receipt_count).then_with(|| {
            left.item_name
                .to_lowercase()
                .cmp(&right.item_name.to_lowercase())
        })
    });
    result
}

fn build_supplier_status_details_from_receipts(
    receipts: Vec<PurchaseReceiptDraft>,
    supplier_display_name: &str,
    kind: &str,
    item_code: &str,
) -> Vec<DispatchRecord> {
    let needle = item_code.trim();
    let mut result = Vec::with_capacity(receipts.len());
    for receipt in receipts {
        let record = purchase_receipt_to_dispatch_record(receipt, supplier_display_name);
        if !record_matches_supplier_breakdown(&record, kind) {
            continue;
        }
        if !needle.is_empty() && !record.item_code.trim().eq_ignore_ascii_case(needle) {
            continue;
        }
        result.push(record);
    }
    result
}

fn record_matches_supplier_breakdown(record: &DispatchRecord, kind: &str) -> bool {
    match kind.trim() {
        "pending" => record.status == "pending" || record.status == "draft",
        "submitted" => record.status == "accepted",
        "returned" => {
            record.status == "partial"
                || record.status == "rejected"
                || record.status == "cancelled"
        }
        _ => false,
    }
}

fn dispatch_record_needs_comment_scan(record: &DispatchRecord) -> bool {
    matches!(record.status.as_str(), "partial" | "rejected" | "cancelled")
        || !record.note.trim().is_empty()
}

fn is_supplier_acknowledgment_comment(content: &str) -> bool {
    let (author, body) = parse_notification_comment(content);
    author.starts_with("Supplier") && body.trim().to_lowercase().starts_with("tasdiqlayman")
}

fn parse_notification_comment(content: &str) -> (String, String) {
    let trimmed = sanitize_notification_comment(content);
    if trimmed.is_empty() {
        return (String::new(), String::new());
    }
    let lines = trimmed.lines().collect::<Vec<_>>();
    if lines.len() >= 2 {
        let head = lines[0].trim();
        let body = lines[1..].join("\n").trim().to_string();
        if !body.is_empty()
            && ["Supplier", "Werka", "Customer", "Admin"]
                .iter()
                .any(|prefix| head.starts_with(prefix))
        {
            return (head.to_string(), body);
        }
    }
    ("Tizim".to_string(), trimmed)
}

fn sanitize_notification_comment(content: &str) -> String {
    content
        .trim()
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("\r\n", "\n")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn filter_supplier_items_by_query(items: Vec<SupplierItem>, query: &str) -> Vec<SupplierItem> {
    let lower_query = query.trim().to_lowercase();
    if lower_query.is_empty() {
        return items;
    }

    items
        .into_iter()
        .filter(|item| {
            item.code.to_lowercase().contains(&lower_query)
                || item.name.to_lowercase().contains(&lower_query)
        })
        .collect()
}

fn limit_supplier_items(mut items: Vec<SupplierItem>, limit: usize) -> Vec<SupplierItem> {
    if limit > 0 && items.len() > limit {
        items.truncate(limit);
    }
    items
}
