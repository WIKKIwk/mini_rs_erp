mod builders;
mod comments;
mod items;
mod receipts;

use crate::core::werka::models::{
    DispatchRecord, SupplierHomeSummary, SupplierItem, SupplierStatusBreakdownEntry,
};
use crate::core::werka::ports::WerkaPortError;
use crate::core::werka::service::WerkaService;
use crate::core::werka::unannounced::item_supplier_permission_denied;

use self::builders::{
    build_supplier_history_from_receipts, build_supplier_status_breakdown_from_receipts,
    build_supplier_status_details_from_receipts, build_supplier_summary_from_receipts,
};
use self::items::{filter_supplier_items_by_query, limit_supplier_items};
use self::receipts::{collect_supplier_purchase_receipts, purchase_receipt_comments_by_name};

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
