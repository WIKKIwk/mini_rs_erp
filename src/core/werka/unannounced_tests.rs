use std::sync::Arc;

use async_trait::async_trait;

use crate::core::werka::models::SupplierItem;
use crate::core::werka::ports::{
    CreatePurchaseReceiptInput, PurchaseReceiptDraft, WerkaHomeLookup, WerkaPortError,
    WerkaSupplierAdminState, WerkaSupplierAdminStateLookup, WerkaSupplierRecord,
    WerkaUnannouncedWriter,
};
use crate::core::werka::service::WerkaService;

#[tokio::test]
async fn unannounced_create_uses_admin_assigned_items_on_permission_error_like_go() {
    let service = WerkaService::new()
        .with_unannounced_writer(Arc::new(FakeUnannouncedWriter {
            validation_error: Some(WerkaPortError::WriteFailed(
                r#"status 403: {"exception":"frappe.exceptions.PermissionError"}"#.to_string(),
            )),
        }))
        .with_supplier_admin_state_lookup(Arc::new(FakeAdminStateLookup {
            state: WerkaSupplierAdminState {
                assigned_item_codes: vec!["ITEM-001".to_string()],
                ..WerkaSupplierAdminState::default()
            },
        }));

    let record = service
        .create_werka_unannounced_draft("SUP-001", "ITEM-001", 2.0, "Werka")
        .await
        .expect("create succeeds through assigned-code fallback")
        .expect("record");

    assert_eq!(record.id, "PR-001");
    assert_eq!(record.event_type, "werka_unannounced_pending");
}

#[tokio::test]
async fn unannounced_create_prefers_direct_supplier_item_lookup_like_go_reader() {
    let service = WerkaService::new()
        .with_lookup(Arc::new(FakeWerkaLookup {
            items: vec![SupplierItem {
                code: "ITEM-001".to_string(),
                name: "Item 001".to_string(),
                uom: "Nos".to_string(),
                warehouse: "Stores - A".to_string(),
                item_group: String::new(),
                customer_names: Vec::new(),
            }],
        }))
        .with_unannounced_writer(Arc::new(FakeUnannouncedWriter {
            validation_error: Some(WerkaPortError::WriteFailed(
                "catalog validation should not be needed".to_string(),
            )),
        }));

    let record = service
        .create_werka_unannounced_draft("SUP-001", "ITEM-001", 2.0, "Werka")
        .await
        .expect("create succeeds through direct lookup")
        .expect("record");

    assert_eq!(record.id, "PR-001");
}

#[tokio::test]
async fn unannounced_create_does_not_use_assigned_code_fallback_for_non_permission_errors() {
    let service = WerkaService::new()
        .with_unannounced_writer(Arc::new(FakeUnannouncedWriter {
            validation_error: Some(WerkaPortError::WriteFailed(
                "item supplierga biriktirilmagan".to_string(),
            )),
        }))
        .with_supplier_admin_state_lookup(Arc::new(FakeAdminStateLookup {
            state: WerkaSupplierAdminState {
                assigned_item_codes: vec!["ITEM-001".to_string()],
                ..WerkaSupplierAdminState::default()
            },
        }));

    let error = service
        .create_werka_unannounced_draft("SUP-001", "ITEM-001", 2.0, "Werka")
        .await
        .expect_err("non-permission validation errors should stay fatal");

    assert!(
        error
            .to_string()
            .contains("item supplierga biriktirilmagan")
    );
}

struct FakeUnannouncedWriter {
    validation_error: Option<WerkaPortError>,
}

#[async_trait]
impl WerkaUnannouncedWriter for FakeUnannouncedWriter {
    async fn find_supplier_for_werka(
        &self,
        supplier_ref: &str,
    ) -> Result<WerkaSupplierRecord, WerkaPortError> {
        Ok(WerkaSupplierRecord {
            id: supplier_ref.to_string(),
            name: "Supplier".to_string(),
            phone: "+998901111111".to_string(),
        })
    }

    async fn validate_supplier_item_allowed(
        &self,
        _supplier_ref: &str,
        _item_code: &str,
    ) -> Result<(), WerkaPortError> {
        if let Some(error) = &self.validation_error {
            return Err(match error {
                WerkaPortError::WriteFailed(message) => {
                    WerkaPortError::WriteFailed(message.clone())
                }
                WerkaPortError::Database(message) => WerkaPortError::Database(message.clone()),
                WerkaPortError::LookupFailed => WerkaPortError::LookupFailed,
                WerkaPortError::InvalidInput => WerkaPortError::InvalidInput,
                WerkaPortError::NotFound => WerkaPortError::NotFound,
                WerkaPortError::LookupUnavailable => WerkaPortError::LookupUnavailable,
                WerkaPortError::InsufficientStock => WerkaPortError::InsufficientStock,
                WerkaPortError::DuplicateCustomerIssueSource => {
                    WerkaPortError::DuplicateCustomerIssueSource
                }
            });
        }
        Ok(())
    }

    async fn resolve_warehouse(&self) -> Result<String, WerkaPortError> {
        Ok("Stores - A".to_string())
    }

    async fn create_draft_purchase_receipt(
        &self,
        _input: CreatePurchaseReceiptInput,
    ) -> Result<PurchaseReceiptDraft, WerkaPortError> {
        Ok(PurchaseReceiptDraft {
            name: "PR-001".to_string(),
            doc_status: 0,
            status: "Draft".to_string(),
            supplier: "SUP-001".to_string(),
            supplier_name: "Supplier".to_string(),
            item_code: "ITEM-001".to_string(),
            item_name: "Item 001".to_string(),
            qty: 2.0,
            uom: "Nos".to_string(),
            ..PurchaseReceiptDraft::default()
        })
    }

    async fn update_purchase_receipt_remarks(
        &self,
        _name: &str,
        _remarks: &str,
    ) -> Result<(), WerkaPortError> {
        Ok(())
    }

    async fn add_purchase_receipt_comment(
        &self,
        _name: &str,
        _content: &str,
    ) -> Result<(), WerkaPortError> {
        Ok(())
    }
}

struct FakeAdminStateLookup {
    state: WerkaSupplierAdminState,
}

#[async_trait]
impl WerkaSupplierAdminStateLookup for FakeAdminStateLookup {
    async fn werka_supplier_admin_state(
        &self,
        _supplier_ref: &str,
    ) -> Result<WerkaSupplierAdminState, WerkaPortError> {
        Ok(self.state.clone())
    }
}

struct FakeWerkaLookup {
    items: Vec<SupplierItem>,
}

#[async_trait]
impl WerkaHomeLookup for FakeWerkaLookup {
    async fn werka_supplier_items(
        &self,
        _supplier_ref: &str,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        Ok(self.items.clone())
    }
}
