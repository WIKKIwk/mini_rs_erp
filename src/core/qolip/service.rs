use std::sync::Arc;

use crate::core::auth::models::Principal;

use super::models::{
    QolipBlock, QolipCellQr, QolipCellQrInput, QolipCheckout, QolipCheckoutCreate,
    QolipCheckoutReturn, QolipError, QolipLocation, QolipLocationMove, QolipLocationUpsert,
    QolipProduct, QolipProductSpec, QolipProductSpecUpsert,
};
use super::normalize::{
    normalize_cell_qr, normalize_checkout, normalize_location, normalize_move_target,
    normalize_product_spec, resolve_cell_qr_from_payload,
};
use super::ports::QolipStorePort;

#[derive(Clone)]
pub struct QolipService {
    store: Arc<dyn QolipStorePort>,
}

impl QolipService {
    pub fn new(store: Arc<dyn QolipStorePort>) -> Self {
        Self { store }
    }

    pub async fn assigned_blocks(
        &self,
        principal: &Principal,
    ) -> Result<Vec<QolipBlock>, QolipError> {
        let mut blocks = self.store.assigned_blocks(principal).await?;
        let assigned_warehouses = self.store.assigned_warehouses(principal).await?;
        if assigned_warehouses.is_empty() {
            return Ok(blocks);
        }
        let assigned_keys = assigned_warehouses
            .iter()
            .map(|warehouse| warehouse.trim().to_lowercase())
            .filter(|warehouse| !warehouse.is_empty())
            .collect::<std::collections::BTreeSet<_>>();
        if assigned_keys.is_empty() {
            return Ok(blocks);
        }
        let mut seen = blocks
            .iter()
            .map(|block| block.name.trim().to_lowercase())
            .collect::<std::collections::BTreeSet<_>>();
        for block in self.store.all_blocks().await? {
            let name_key = block.name.trim().to_lowercase();
            if assigned_keys.contains(&name_key) && seen.insert(name_key) {
                blocks.push(block);
            }
        }
        blocks.sort_by_key(|block| block.name.to_lowercase());
        Ok(blocks)
    }

    pub async fn blocks_for_principal(
        &self,
        principal: &Principal,
        is_admin: bool,
    ) -> Result<Vec<QolipBlock>, QolipError> {
        if is_admin {
            self.store.all_blocks().await
        } else {
            self.assigned_blocks(principal).await
        }
    }

    pub async fn warehouses_for_principal(
        &self,
        principal: &Principal,
        is_admin: bool,
    ) -> Result<Vec<String>, QolipError> {
        if is_admin {
            let blocks = self.store.all_blocks().await?;
            let mut warehouses = blocks
                .into_iter()
                .map(|block| block.warehouse.trim().to_string())
                .filter(|warehouse| !warehouse.is_empty())
                .collect::<Vec<_>>();
            warehouses.sort_by_key(|warehouse| warehouse.to_lowercase());
            warehouses.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
            Ok(warehouses)
        } else {
            self.store.assigned_warehouses(principal).await
        }
    }

    pub async fn assigned_warehouses(
        &self,
        principal: &Principal,
    ) -> Result<Vec<String>, QolipError> {
        self.store.assigned_warehouses(principal).await
    }

    pub async fn products(
        &self,
        query: &str,
        limit: usize,
        with_qolip_only: bool,
    ) -> Result<Vec<QolipProduct>, QolipError> {
        self.store
            .products(query, limit.clamp(1, 100), with_qolip_only)
            .await
    }

    pub async fn upsert_product_spec(
        &self,
        input: QolipProductSpecUpsert,
        principal: &Principal,
    ) -> Result<QolipProductSpec, QolipError> {
        let normalized = normalize_product_spec(input, principal)?;
        self.store.put_product_spec(normalized).await
    }

    pub async fn locations(&self, block: &str) -> Result<Vec<QolipLocation>, QolipError> {
        let block = block.trim();
        if block.is_empty() {
            return Err(QolipError::MissingBlock);
        }
        self.store.locations(block).await
    }

    pub async fn upsert_location(
        &self,
        mut input: QolipLocationUpsert,
        principal: &Principal,
    ) -> Result<QolipLocation, QolipError> {
        if input.qolip_code.trim().is_empty() || input.size <= 0 {
            let spec = self
                .store
                .product_spec(&input.item_code)
                .await?
                .ok_or(QolipError::MissingQolipCode)?;
            if input.item_name.trim().is_empty() {
                input.item_name = spec.item_name.clone();
            }
            if input.item_group.trim().is_empty() {
                input.item_group = spec.item_group.clone();
            }
            input.qolip_code = spec.qolip_code;
            input.size = spec.size;
        }
        let normalized = normalize_location(input, principal)?;
        self.store.put_location(normalized).await
    }

    pub async fn cell_qr(
        &self,
        input: QolipCellQrInput,
        principal: &Principal,
    ) -> Result<QolipCellQr, QolipError> {
        let normalized = normalize_cell_qr(input, principal)?;
        self.store.get_or_create_cell_qr(normalized).await
    }

    pub async fn location_by_id(
        &self,
        location_id: &str,
    ) -> Result<Option<QolipLocation>, QolipError> {
        let location_id = location_id.trim();
        if location_id.is_empty() {
            return Err(QolipError::LocationNotFound);
        }
        self.store.location_by_id(location_id).await
    }

    pub async fn issue_checkout(
        &self,
        input: QolipCheckoutCreate,
        worker_id: &str,
        worker_name: &str,
        principal: &Principal,
    ) -> Result<QolipCheckout, QolipError> {
        let location = self
            .location_by_id(&input.location_id)
            .await?
            .ok_or(QolipError::LocationNotFound)?;
        self.issue_checkout_from_location(
            location,
            input.quantity,
            worker_id,
            worker_name,
            principal,
        )
        .await
    }

    pub async fn issue_checkout_from_location(
        &self,
        location: QolipLocation,
        quantity: i32,
        worker_id: &str,
        worker_name: &str,
        principal: &Principal,
    ) -> Result<QolipCheckout, QolipError> {
        let checkout = normalize_checkout(location, quantity, worker_id, worker_name, principal)?;
        self.store.issue_checkout(checkout).await
    }

    pub async fn checkouts(
        &self,
        _principal: &Principal,
        _is_admin: bool,
        block: Option<&str>,
        status: &str,
        limit: usize,
    ) -> Result<Vec<QolipCheckout>, QolipError> {
        let status = status.trim();
        let status = if status.is_empty() { "open" } else { status };
        let limit = limit.clamp(1, 200);
        if block.is_some() {
            return self.store.checkouts(block, None, status, limit).await;
        }
        self.store.checkouts(None, None, status, limit).await
    }

    pub async fn checkout_by_id(
        &self,
        checkout_id: &str,
    ) -> Result<Option<QolipCheckout>, QolipError> {
        let checkout_id = checkout_id.trim();
        if checkout_id.is_empty() {
            return Ok(None);
        }
        self.store.checkout_by_id(checkout_id).await
    }

    pub async fn return_checkout(
        &self,
        input: QolipCheckoutReturn,
    ) -> Result<QolipCheckout, QolipError> {
        let checkout_id = input.checkout_id.trim();
        if checkout_id.is_empty() {
            return Err(QolipError::CheckoutNotFound);
        }
        self.store
            .return_checkout(checkout_id, &input.row_letter, input.column_number)
            .await
    }

    pub async fn move_location(
        &self,
        input: QolipLocationMove,
        _principal: &Principal,
    ) -> Result<QolipLocation, QolipError> {
        let location_id = input.location_id.trim();
        if location_id.is_empty() {
            return Err(QolipError::LocationNotFound);
        }
        let source = self
            .location_by_id(location_id)
            .await?
            .ok_or(QolipError::LocationNotFound)?;
        let column_number = input.column_number.ok_or(QolipError::InvalidLocation)?;
        let _target =
            normalize_move_target(&source, &input.row_letter, column_number, input.quantity)?;
        self.store
            .move_location(
                location_id,
                &input.row_letter,
                column_number,
                input.quantity,
            )
            .await
    }

    pub async fn cell_qr_by_payload(
        &self,
        qr_payload: &str,
    ) -> Result<Option<QolipCellQr>, QolipError> {
        let qr_payload = qr_payload.trim();
        if qr_payload.is_empty() {
            return Ok(None);
        }
        self.store.cell_qr_by_payload(qr_payload).await
    }

    pub async fn resolve_cell_qr(
        &self,
        qr_payload: &str,
        principal: &Principal,
    ) -> Result<Option<QolipCellQr>, QolipError> {
        let qr_payload = qr_payload.trim();
        if qr_payload.is_empty() {
            return Ok(None);
        }
        if let Some(cell) = self.store.cell_qr_by_payload(qr_payload).await? {
            return Ok(Some(cell));
        }
        let mut blocks = self.store.assigned_blocks(principal).await?;
        if blocks.is_empty() {
            blocks = self.store.all_blocks().await?;
        }
        let Some(cell) = resolve_cell_qr_from_payload(qr_payload, &blocks, principal) else {
            return Ok(None);
        };
        Ok(Some(self.store.get_or_create_cell_qr(cell).await?))
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use crate::core::auth::models::{Principal, PrincipalRole};

    use super::super::models::{
        QolipBlock, QolipCellQr, QolipCheckout, QolipError, QolipLocation, QolipProduct,
        QolipProductSpec,
    };
    use super::super::ports::QolipStorePort;
    use super::QolipService;

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
        async fn assigned_warehouses(
            &self,
            _principal: &Principal,
        ) -> Result<Vec<String>, QolipError> {
            Ok(Vec::new())
        }

        async fn assigned_blocks(
            &self,
            _principal: &Principal,
        ) -> Result<Vec<QolipBlock>, QolipError> {
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

        async fn product_spec(
            &self,
            _item_code: &str,
        ) -> Result<Option<QolipProductSpec>, QolipError> {
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

        async fn get_or_create_cell_qr(
            &self,
            cell: QolipCellQr,
        ) -> Result<QolipCellQr, QolipError> {
            Ok(cell)
        }

        async fn issue_checkout(
            &self,
            checkout: QolipCheckout,
        ) -> Result<QolipCheckout, QolipError> {
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
        async fn assigned_warehouses(
            &self,
            _principal: &Principal,
        ) -> Result<Vec<String>, QolipError> {
            Ok(vec!["A".to_string()])
        }

        async fn assigned_blocks(
            &self,
            _principal: &Principal,
        ) -> Result<Vec<QolipBlock>, QolipError> {
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

        async fn product_spec(
            &self,
            _item_code: &str,
        ) -> Result<Option<QolipProductSpec>, QolipError> {
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

        async fn get_or_create_cell_qr(
            &self,
            cell: QolipCellQr,
        ) -> Result<QolipCellQr, QolipError> {
            Ok(cell)
        }

        async fn issue_checkout(
            &self,
            checkout: QolipCheckout,
        ) -> Result<QolipCheckout, QolipError> {
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
}
