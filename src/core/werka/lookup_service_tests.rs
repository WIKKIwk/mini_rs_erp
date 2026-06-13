use std::sync::Arc;

use async_trait::async_trait;
use time::Date;

use super::models::{
    CustomerDirectoryEntry, CustomerItemOption, DispatchRecord, SupplierDirectoryEntry,
    SupplierItem, WerkaArchiveResponse, WerkaArchiveSummary, WerkaHomeData, WerkaHomeSummary,
    WerkaStatusBreakdownEntry,
};
use super::ports::{WerkaHomeLookup, WerkaPortError};
use super::service::WerkaService;

#[tokio::test]
async fn home_preloads_from_lookup_with_limit() {
    let data = WerkaService::new()
        .with_lookup(Arc::new(FakeWerkaHomeLookup))
        .home(20)
        .await
        .expect("home result")
        .expect("home data");

    assert_eq!(data.summary.pending_count, 1);
    assert_eq!(data.pending_items[0].id, "PR-001");
}

#[tokio::test]
async fn summary_uses_lookup() {
    let summary = WerkaService::new()
        .with_lookup(Arc::new(FakeWerkaHomeLookup))
        .summary()
        .await
        .expect("summary result")
        .expect("summary data");

    assert_eq!(summary.pending_count, 1);
    assert_eq!(summary.confirmed_count, 2);
    assert_eq!(summary.returned_count, 3);
}

#[tokio::test]
async fn pending_uses_lookup_with_limit() {
    let items = WerkaService::new()
        .with_lookup(Arc::new(FakeWerkaHomeLookup))
        .pending(7)
        .await
        .expect("pending result")
        .expect("pending data");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "PR-001");
}

#[tokio::test]
async fn history_uses_lookup() {
    let items = WerkaService::new()
        .with_lookup(Arc::new(FakeWerkaHomeLookup))
        .history()
        .await
        .expect("history result")
        .expect("history data");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].event_type, "supplier_ack");
}

#[tokio::test]
async fn status_breakdown_uses_lookup_with_kind() {
    let items = WerkaService::new()
        .with_lookup(Arc::new(FakeWerkaHomeLookup))
        .status_breakdown("returned")
        .await
        .expect("status breakdown result")
        .expect("status breakdown data");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].supplier_ref, "SUP-001");
}

#[tokio::test]
async fn status_details_uses_lookup_with_kind_and_supplier() {
    let items = WerkaService::new()
        .with_lookup(Arc::new(FakeWerkaHomeLookup))
        .status_details("pending", "SUP-001")
        .await
        .expect("status details result")
        .expect("status details data");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "PR-001");
}

#[tokio::test]
async fn archive_uses_lookup_with_filters() {
    let items = WerkaService::new()
        .with_lookup(Arc::new(FakeWerkaHomeLookup))
        .archive("sent", "monthly", None, None)
        .await
        .expect("archive result")
        .expect("archive data");

    assert_eq!(items.kind, "sent");
    assert_eq!(items.period, "monthly");
    assert_eq!(items.summary.record_count, 1);
}

#[tokio::test]
async fn suppliers_uses_lookup_with_pagination() {
    let items = WerkaService::new()
        .with_lookup(Arc::new(FakeWerkaHomeLookup))
        .suppliers("Ali", 20, 3)
        .await
        .expect("suppliers result")
        .expect("suppliers data");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].ref_, "SUP-001");
}

#[tokio::test]
async fn customers_uses_lookup_with_pagination() {
    let items = WerkaService::new()
        .with_lookup(Arc::new(FakeWerkaHomeLookup))
        .customers("Ali", 20, 3)
        .await
        .expect("customers result")
        .expect("customers data");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].ref_, "CUST-001");
}

#[tokio::test]
async fn item_searches_use_lookup_with_filters() {
    let service = WerkaService::new().with_lookup(Arc::new(FakeWerkaHomeLookup));

    let supplier_items = service
        .supplier_items("SUP-001", "milk", 20, 3)
        .await
        .expect("supplier items result")
        .expect("supplier items data");
    let customer_items = service
        .customer_items("CUST-001", "milk", 20, 3)
        .await
        .expect("customer items result")
        .expect("customer items data");
    let options = service
        .customer_item_options("milk", 20, 3)
        .await
        .expect("customer item options result")
        .expect("customer item options data");

    assert_eq!(supplier_items[0].code, "SUP-ITEM");
    assert_eq!(customer_items[0].code, "CUST-ITEM");
    assert_eq!(options[0].customer_ref, "CUST-001");
}

struct FakeWerkaHomeLookup;

#[async_trait]
impl WerkaHomeLookup for FakeWerkaHomeLookup {
    async fn werka_summary(&self) -> Result<WerkaHomeSummary, WerkaPortError> {
        Ok(WerkaHomeSummary {
            pending_count: 1,
            confirmed_count: 2,
            returned_count: 3,
        })
    }

    async fn werka_home(&self, pending_limit: usize) -> Result<WerkaHomeData, WerkaPortError> {
        assert_eq!(pending_limit, 20);
        Ok(WerkaHomeData {
            summary: WerkaHomeSummary {
                pending_count: 1,
                confirmed_count: 2,
                returned_count: 3,
            },
            pending_items: vec![DispatchRecord {
                id: "PR-001".to_string(),
                supplier_name: "Supplier".to_string(),
                item_code: "ITEM-001".to_string(),
                item_name: "Item".to_string(),
                uom: "Kg".to_string(),
                sent_qty: 10.0,
                accepted_qty: 0.0,
                status: "pending".to_string(),
                created_label: "2026-01-16T10:00:00Z".to_string(),
                ..DispatchRecord::default()
            }],
        })
    }

    async fn werka_pending(&self, limit: usize) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        assert_eq!(limit, 7);
        Ok(vec![DispatchRecord {
            id: "PR-001".to_string(),
            supplier_name: "Supplier".to_string(),
            item_code: "ITEM-001".to_string(),
            item_name: "Item".to_string(),
            uom: "Kg".to_string(),
            sent_qty: 10.0,
            accepted_qty: 0.0,
            status: "pending".to_string(),
            created_label: "2026-01-16T10:00:00Z".to_string(),
            ..DispatchRecord::default()
        }])
    }

    async fn werka_history(&self) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        Ok(vec![DispatchRecord {
            id: "supplier_ack:COMM-001".to_string(),
            supplier_name: "Supplier".to_string(),
            item_code: "ITEM-001".to_string(),
            item_name: "Item".to_string(),
            uom: "Kg".to_string(),
            sent_qty: 10.0,
            accepted_qty: 10.0,
            event_type: "supplier_ack".to_string(),
            status: "accepted".to_string(),
            created_label: "2026-01-16T10:00:00Z".to_string(),
            ..DispatchRecord::default()
        }])
    }

    async fn werka_status_breakdown(
        &self,
        kind: &str,
    ) -> Result<Vec<WerkaStatusBreakdownEntry>, WerkaPortError> {
        assert_eq!(kind, "returned");
        Ok(vec![WerkaStatusBreakdownEntry {
            supplier_ref: "SUP-001".to_string(),
            supplier_name: "Supplier".to_string(),
            receipt_count: 1,
            total_sent_qty: 10.0,
            total_accepted_qty: 8.0,
            total_returned_qty: 2.0,
            uom: "Kg".to_string(),
        }])
    }

    async fn werka_status_details(
        &self,
        kind: &str,
        supplier_ref: &str,
    ) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        assert_eq!(kind, "pending");
        assert_eq!(supplier_ref, "SUP-001");
        Ok(vec![DispatchRecord {
            id: "PR-001".to_string(),
            supplier_ref: "SUP-001".to_string(),
            supplier_name: "Supplier".to_string(),
            item_code: "ITEM-001".to_string(),
            item_name: "Item".to_string(),
            uom: "Kg".to_string(),
            sent_qty: 10.0,
            accepted_qty: 0.0,
            status: "pending".to_string(),
            created_label: "2026-01-16T10:00:00Z".to_string(),
            ..DispatchRecord::default()
        }])
    }

    async fn werka_archive(
        &self,
        kind: &str,
        period: &str,
        from: Option<Date>,
        to: Option<Date>,
    ) -> Result<WerkaArchiveResponse, WerkaPortError> {
        assert_eq!(kind, "sent");
        assert_eq!(period, "monthly");
        assert!(from.is_none());
        assert!(to.is_none());
        Ok(WerkaArchiveResponse {
            kind: "sent".to_string(),
            period: "monthly".to_string(),
            summary: WerkaArchiveSummary {
                record_count: 1,
                totals_by_uom: Vec::new(),
            },
            items: Vec::new(),
            ..WerkaArchiveResponse::default()
        })
    }

    async fn werka_suppliers(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierDirectoryEntry>, WerkaPortError> {
        assert_eq!(query, "Ali");
        assert_eq!(limit, 20);
        assert_eq!(offset, 3);
        Ok(vec![SupplierDirectoryEntry {
            ref_: "SUP-001".to_string(),
            name: "Ali".to_string(),
            phone: "+998901111111".to_string(),
        }])
    }

    async fn werka_customers(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<CustomerDirectoryEntry>, WerkaPortError> {
        assert_eq!(query, "Ali");
        assert_eq!(limit, 20);
        assert_eq!(offset, 3);
        Ok(vec![CustomerDirectoryEntry {
            ref_: "CUST-001".to_string(),
            name: "Ali Market".to_string(),
            phone: "+998902222222".to_string(),
        }])
    }

    async fn werka_supplier_items(
        &self,
        supplier_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        assert_eq!(supplier_ref, "SUP-001");
        assert_eq!(query, "milk");
        assert_eq!(limit, 20);
        assert_eq!(offset, 3);
        Ok(vec![supplier_item("SUP-ITEM", "Supplier Milk")])
    }

    async fn werka_customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        assert_eq!(customer_ref, "CUST-001");
        assert_eq!(query, "milk");
        assert_eq!(limit, 20);
        assert_eq!(offset, 3);
        Ok(vec![supplier_item("CUST-ITEM", "Customer Milk")])
    }

    async fn werka_customer_item_options(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<CustomerItemOption>, WerkaPortError> {
        assert_eq!(query, "milk");
        assert_eq!(limit, 20);
        assert_eq!(offset, 3);
        Ok(vec![CustomerItemOption {
            customer_ref: "CUST-001".to_string(),
            customer_name: "Ali Market".to_string(),
            customer_phone: "+998902222222".to_string(),
            item_code: "ITEM-001".to_string(),
            item_name: "Milk".to_string(),
            uom: "Kg".to_string(),
            warehouse: "Stores - A".to_string(),
        }])
    }
}

fn supplier_item(code: &str, name: &str) -> SupplierItem {
    SupplierItem {
        code: code.to_string(),
        name: name.to_string(),
        uom: "Kg".to_string(),
        warehouse: "Stores - A".to_string(),
        item_group: String::new(),
    }
}
