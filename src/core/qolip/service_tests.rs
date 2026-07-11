use async_trait::async_trait;

use crate::core::auth::models::{Principal, PrincipalRole};

use super::models::{
    QolipBlock, QolipCellQr, QolipCheckout, QolipError, QolipLocation, QolipProduct,
    QolipLocationUpsert, QolipProductSpec, QolipProductSpecUpsert,
};
use super::memory_store::MemoryQolipStore;
use super::ports::QolipStorePort;
use super::service::QolipService;

#[tokio::test]
async fn assigned_blocks_include_direct_block_assignments() {
    let service = QolipService::new(std::sync::Arc::new(DirectBlockAssignmentStore));
    let blocks = service
        .assigned_blocks(&principal())
        .await
        .expect("assigned blocks");

    assert_eq!(
        blocks,
        vec![QolipBlock {
            name: "A".to_string(),
            warehouse: "Qolip ombor".to_string(),
        }]
    );
}

#[tokio::test]
async fn checkouts_without_block_return_full_open_debt_ledger() {
    let service = QolipService::new(std::sync::Arc::new(CheckoutLedgerStore));
    let checkouts = service
        .checkouts(&principal(), false, None, "open", 100)
        .await
        .expect("checkouts");

    assert_eq!(
        checkouts
            .iter()
            .map(|checkout| checkout.block.as_str())
            .collect::<Vec<_>>(),
        vec!["A", "B"]
    );
}

#[tokio::test]
async fn order_start_rejects_qolip_from_another_finished_product_group() {
    let store = std::sync::Arc::new(MemoryQolipStore::new());
    store
        .seed_products(vec![QolipProduct {
            code: "ITEM-ORDER".to_string(),
            name: "Order product".to_string(),
            item_group: "Tayyor mahsulot A".to_string(),
            qolip_code: String::new(),
            size: 0,
            has_qolip_spec: false,
        }])
        .await;
    let service = QolipService::new(store);
    service
        .upsert_product_spec(
            QolipProductSpecUpsert {
                item_code: "ITEM-ORDER".to_string(),
                item_name: "Order product".to_string(),
                item_group: "Tayyor mahsulot B".to_string(),
                qolip_code: "QOLIP-WRONG-GROUP".to_string(),
                size: 42,
            },
            &principal(),
        )
        .await
        .expect("save mismatched spec");
    service
        .upsert_location(
            QolipLocationUpsert {
                block: "A".to_string(),
                warehouse: "Qolip ombor".to_string(),
                item_code: "ITEM-ORDER".to_string(),
                item_name: "Order product".to_string(),
                item_group: "Tayyor mahsulot B".to_string(),
                qolip_code: "QOLIP-WRONG-GROUP".to_string(),
                size: 42,
                quantity: 1,
                row_letter: "A".to_string(),
                column_number: Some(1),
            },
            &principal(),
        )
        .await
        .expect("save location");

    let error = service
        .prepare_qolip_code_for_order_start(
            "QOLIP-WRONG-GROUP",
            "ITEM-ORDER",
            "Order product",
            "worker-1",
            "Worker",
            &principal(),
        )
        .await
        .expect_err("wrong product group must be rejected");

    assert_eq!(error, QolipError::QolipCodeMismatch);
}

#[tokio::test]
async fn order_start_rejects_another_products_qolip_from_same_group() {
    let store = std::sync::Arc::new(MemoryQolipStore::new());
    store
        .seed_products(vec![
            QolipProduct {
                code: "ITEM-ORDER".to_string(),
                name: "Order product".to_string(),
                item_group: "Tayyor mahsulot".to_string(),
                qolip_code: String::new(),
                size: 0,
                has_qolip_spec: false,
            },
            QolipProduct {
                code: "ITEM-OTHER".to_string(),
                name: "Other product".to_string(),
                item_group: "Tayyor mahsulot".to_string(),
                qolip_code: String::new(),
                size: 0,
                has_qolip_spec: false,
            },
        ])
        .await;
    let service = QolipService::new(store);
    service
        .upsert_product_spec(
            QolipProductSpecUpsert {
                item_code: "ITEM-OTHER".to_string(),
                item_name: "Other product".to_string(),
                item_group: "Tayyor mahsulot".to_string(),
                qolip_code: "QOLIP-OTHER".to_string(),
                size: 42,
            },
            &principal(),
        )
        .await
        .expect("save other product spec");
    service
        .upsert_location(
            QolipLocationUpsert {
                block: "A".to_string(),
                warehouse: "Qolip ombor".to_string(),
                item_code: "ITEM-OTHER".to_string(),
                item_name: "Other product".to_string(),
                item_group: "Tayyor mahsulot".to_string(),
                qolip_code: "QOLIP-OTHER".to_string(),
                size: 42,
                quantity: 1,
                row_letter: "A".to_string(),
                column_number: Some(2),
            },
            &principal(),
        )
        .await
        .expect("save other product location");

    let error = service
        .prepare_qolip_code_for_order_start(
            "QOLIP-OTHER",
            "ITEM-ORDER",
            "Order product",
            "worker-1",
            "Worker",
            &principal(),
        )
        .await
        .expect_err("another product's qolip must be rejected");

    assert_eq!(error, QolipError::QolipCodeMismatch);
}

fn principal() -> Principal {
    Principal {
        role: PrincipalRole::Qolipchi,
        display_name: "Qolipchi".to_string(),
        legal_name: "Qolipchi".to_string(),
        ref_: "qolipchi-1".to_string(),
        phone: "+998901112233".to_string(),
        avatar_url: String::new(),
    }
}

fn checkout(id: &str, block: &str) -> QolipCheckout {
    QolipCheckout {
        id: id.to_string(),
        location_id: format!("location-{id}"),
        block: block.to_string(),
        warehouse: "Qolip ombor".to_string(),
        item_code: format!("ITEM-{id}"),
        item_name: format!("Qolip {id}"),
        item_group: String::new(),
        qolip_code: format!("Q-{id}"),
        size: 42,
        quantity: 1,
        row_letter: "A".to_string(),
        column_number: Some(1),
        location_label: "A1".to_string(),
        issued_to_ref: "worker".to_string(),
        issued_to_name: "Worker".to_string(),
        status: "open".to_string(),
        issued_by_role: "qolipchi".to_string(),
        issued_by_ref: "qolipchi-1".to_string(),
        issued_by_name: "Qolipchi".to_string(),
        issued_at: "2026-06-22T09:00:00Z".to_string(),
    }
}

struct DirectBlockAssignmentStore;

struct CheckoutLedgerStore;

#[async_trait]
impl QolipStorePort for CheckoutLedgerStore {
    async fn assigned_warehouses(&self, _principal: &Principal) -> Result<Vec<String>, QolipError> {
        Ok(Vec::new())
    }

    async fn assigned_blocks(&self, _principal: &Principal) -> Result<Vec<QolipBlock>, QolipError> {
        Ok(vec![QolipBlock {
            name: "A".to_string(),
            warehouse: "Qolip ombor".to_string(),
        }])
    }

    async fn all_blocks(&self) -> Result<Vec<QolipBlock>, QolipError> {
        Ok(vec![
            QolipBlock {
                name: "A".to_string(),
                warehouse: "Qolip ombor".to_string(),
            },
            QolipBlock {
                name: "B".to_string(),
                warehouse: "Qolip ombor".to_string(),
            },
        ])
    }

    async fn products(
        &self,
        _query: &str,
        _limit: usize,
        _with_qolip_only: bool,
    ) -> Result<Vec<QolipProduct>, QolipError> {
        Ok(Vec::new())
    }

    async fn product_spec(&self, _item_code: &str) -> Result<Option<QolipProductSpec>, QolipError> {
        Ok(None)
    }

    async fn put_product_spec(
        &self,
        spec: QolipProductSpec,
    ) -> Result<QolipProductSpec, QolipError> {
        Ok(spec)
    }

    async fn locations(&self, _block: &str) -> Result<Vec<QolipLocation>, QolipError> {
        Ok(Vec::new())
    }

    async fn location_by_id(
        &self,
        _location_id: &str,
    ) -> Result<Option<QolipLocation>, QolipError> {
        Ok(None)
    }

    async fn put_location(&self, location: QolipLocation) -> Result<QolipLocation, QolipError> {
        Ok(location)
    }

    async fn get_or_create_cell_qr(&self, cell: QolipCellQr) -> Result<QolipCellQr, QolipError> {
        Ok(cell)
    }

    async fn issue_checkout(&self, checkout: QolipCheckout) -> Result<QolipCheckout, QolipError> {
        Ok(checkout)
    }

    async fn checkouts(
        &self,
        _block: Option<&str>,
        allowed_blocks: Option<&[String]>,
        status: &str,
        limit: usize,
    ) -> Result<Vec<QolipCheckout>, QolipError> {
        let mut items = vec![checkout("1", "A"), checkout("2", "B")]
            .into_iter()
            .filter(|checkout| checkout.status.eq_ignore_ascii_case(status))
            .filter(|checkout| {
                allowed_blocks.is_none_or(|allowed| {
                    allowed
                        .iter()
                        .any(|block| checkout.block.eq_ignore_ascii_case(block))
                })
            })
            .collect::<Vec<_>>();
        items.truncate(limit);
        Ok(items)
    }

    async fn checkout_by_id(
        &self,
        _checkout_id: &str,
    ) -> Result<Option<QolipCheckout>, QolipError> {
        Ok(None)
    }

    async fn return_checkout(
        &self,
        _checkout_id: &str,
        _row_letter: &str,
        _column_number: Option<i32>,
    ) -> Result<QolipCheckout, QolipError> {
        Err(QolipError::CheckoutNotFound)
    }

    async fn move_location(
        &self,
        _location_id: &str,
        _row_letter: &str,
        _column_number: i32,
        _quantity: i32,
    ) -> Result<QolipLocation, QolipError> {
        Err(QolipError::LocationNotFound)
    }

    async fn cell_qr_by_payload(
        &self,
        _qr_payload: &str,
    ) -> Result<Option<QolipCellQr>, QolipError> {
        Ok(None)
    }
}

#[async_trait]
impl QolipStorePort for DirectBlockAssignmentStore {
    async fn assigned_warehouses(&self, _principal: &Principal) -> Result<Vec<String>, QolipError> {
        Ok(vec!["A".to_string()])
    }

    async fn assigned_blocks(&self, _principal: &Principal) -> Result<Vec<QolipBlock>, QolipError> {
        Ok(Vec::new())
    }

    async fn all_blocks(&self) -> Result<Vec<QolipBlock>, QolipError> {
        Ok(vec![QolipBlock {
            name: "A".to_string(),
            warehouse: "Qolip ombor".to_string(),
        }])
    }

    async fn products(
        &self,
        _query: &str,
        _limit: usize,
        _with_qolip_only: bool,
    ) -> Result<Vec<QolipProduct>, QolipError> {
        Ok(Vec::new())
    }

    async fn product_spec(&self, _item_code: &str) -> Result<Option<QolipProductSpec>, QolipError> {
        Ok(None)
    }

    async fn put_product_spec(
        &self,
        spec: QolipProductSpec,
    ) -> Result<QolipProductSpec, QolipError> {
        Ok(spec)
    }

    async fn locations(&self, _block: &str) -> Result<Vec<QolipLocation>, QolipError> {
        Ok(Vec::new())
    }

    async fn location_by_id(
        &self,
        _location_id: &str,
    ) -> Result<Option<QolipLocation>, QolipError> {
        Ok(None)
    }

    async fn put_location(&self, location: QolipLocation) -> Result<QolipLocation, QolipError> {
        Ok(location)
    }

    async fn get_or_create_cell_qr(&self, cell: QolipCellQr) -> Result<QolipCellQr, QolipError> {
        Ok(cell)
    }

    async fn issue_checkout(&self, checkout: QolipCheckout) -> Result<QolipCheckout, QolipError> {
        Ok(checkout)
    }

    async fn checkouts(
        &self,
        _block: Option<&str>,
        _allowed_blocks: Option<&[String]>,
        _status: &str,
        _limit: usize,
    ) -> Result<Vec<QolipCheckout>, QolipError> {
        Ok(Vec::new())
    }

    async fn checkout_by_id(
        &self,
        _checkout_id: &str,
    ) -> Result<Option<QolipCheckout>, QolipError> {
        Ok(None)
    }

    async fn return_checkout(
        &self,
        _checkout_id: &str,
        _row_letter: &str,
        _column_number: Option<i32>,
    ) -> Result<QolipCheckout, QolipError> {
        Err(QolipError::CheckoutNotFound)
    }

    async fn move_location(
        &self,
        _location_id: &str,
        _row_letter: &str,
        _column_number: i32,
        _quantity: i32,
    ) -> Result<QolipLocation, QolipError> {
        Err(QolipError::LocationNotFound)
    }

    async fn cell_qr_by_payload(
        &self,
        _qr_payload: &str,
    ) -> Result<Option<QolipCellQr>, QolipError> {
        Ok(None)
    }
}
