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
