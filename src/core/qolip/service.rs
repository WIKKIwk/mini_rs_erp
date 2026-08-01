use std::sync::Arc;

use crate::core::auth::models::Principal;

use super::models::{
    QolipBlock, QolipCellQr, QolipCellQrInput, QolipCheckout, QolipCheckoutCreate,
    QolipCheckoutReturn, QolipError, QolipLocation, QolipLocationMove, QolipLocationUpsert,
    QolipOrderNote, QolipOrderStartPreparation, QolipProduct, QolipProductSpec,
    QolipProductSpecUpsert,
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

    pub async fn rename_block(
        &self,
        block: &str,
        new_block: &str,
        warehouse: &str,
    ) -> Result<QolipBlock, QolipError> {
        let block = block.trim();
        let new_block = new_block.trim();
        let warehouse = warehouse.trim();
        if block.is_empty() || new_block.is_empty() {
            return Err(QolipError::MissingBlock);
        }
        self.store.rename_block(block, new_block, warehouse).await
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
            .products(query, limit.clamp(1, 20_000), with_qolip_only)
            .await
    }

    pub async fn upsert_product_spec(
        &self,
        input: QolipProductSpecUpsert,
        principal: &Principal,
    ) -> Result<QolipProductSpec, QolipError> {
        let previous_qolip_code = input.previous_qolip_code.trim().to_string();
        let normalized = normalize_product_spec(input, principal)?;
        if !previous_qolip_code.is_empty() {
            let next_qolip_code = normalized.qolip_code.clone();
            return match self
                .store
                .rename_product_spec(&previous_qolip_code, normalized.clone())
                .await
            {
                Ok(spec) => Ok(spec),
                Err(QolipError::QolipCodeNotFound)
                    if previous_qolip_code.eq_ignore_ascii_case(&next_qolip_code) =>
                {
                    self.store.put_product_spec(normalized).await
                }
                Err(error) => Err(error),
            };
        }
        self.store.put_product_spec(normalized).await
    }

    pub async fn delete_product_specs(
        &self,
        qolip_codes: Vec<String>,
    ) -> Result<usize, QolipError> {
        let mut normalized = qolip_codes
            .into_iter()
            .map(|code| code.trim().to_string())
            .filter(|code| !code.is_empty())
            .collect::<Vec<_>>();
        normalized.sort_by_key(|code| code.to_lowercase());
        normalized.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
        if normalized.is_empty() {
            return Err(QolipError::MissingQolipCode);
        }
        self.store.delete_product_specs(&normalized).await
    }

    pub async fn product_spec_by_qolip_code(
        &self,
        qolip_code: &str,
    ) -> Result<Option<QolipProductSpec>, QolipError> {
        let qolip_code = qolip_code.trim();
        if qolip_code.is_empty() {
            return Err(QolipError::MissingQolipCode);
        }
        self.store.product_spec_by_qolip_code(qolip_code).await
    }

    pub async fn required_qolips_for_order(
        &self,
        item_code: &str,
        item_name: &str,
    ) -> Result<Vec<QolipProductSpec>, QolipError> {
        let expected_product = self
            .order_product(item_code, item_name)
            .await?
            .ok_or(QolipError::QolipCodeMismatch)?;
        let mut specs = self
            .store
            .product_specs(&expected_product.code)
            .await?
            .into_iter()
            .filter(|spec| qolip_spec_matches_order(spec, &expected_product))
            .filter(|spec| !spec.qolip_code.trim().is_empty())
            .collect::<Vec<_>>();
        specs.sort_by_key(|spec| spec.qolip_code.trim().to_lowercase());
        specs.dedup_by(|left, right| {
            left.qolip_code
                .trim()
                .eq_ignore_ascii_case(right.qolip_code.trim())
        });
        Ok(specs)
    }

    pub async fn order_notes(
        &self,
        principal: &Principal,
    ) -> Result<Vec<QolipOrderNote>, QolipError> {
        self.store.order_notes(principal).await
    }

    pub async fn order_note_qolip_codes_in_use(
        &self,
        principal: &Principal,
        order_id: &str,
    ) -> Result<Vec<String>, QolipError> {
        self.store
            .order_note_qolip_codes_in_use(principal, order_id)
            .await
    }

    pub async fn order_note(
        &self,
        principal: &Principal,
        order_id: &str,
    ) -> Result<Option<QolipOrderNote>, QolipError> {
        self.store.order_note(principal, order_id).await
    }

    pub async fn save_order_note(
        &self,
        mut note: QolipOrderNote,
        principal: &Principal,
    ) -> Result<QolipOrderNote, QolipError> {
        note.order_id = note.order_id.trim().to_string();
        note.item_code = note.item_code.trim().to_string();
        note.item_name = note.item_name.trim().to_string();
        note.status = note.status.trim().to_ascii_lowercase();
        let mut codes = Vec::new();
        for code in note.qolip_codes {
            let code = code.trim();
            if code.is_empty()
                || codes
                    .iter()
                    .any(|existing: &String| existing.eq_ignore_ascii_case(code))
            {
                continue;
            }
            codes.push(code.to_string());
        }
        codes.sort_by_key(|code| code.to_ascii_lowercase());
        if note.status == "given" && codes.is_empty() {
            return Err(QolipError::MissingQolipCode);
        }
        note.qolip_codes = codes;
        self.store.save_order_note(principal, note).await
    }

    pub async fn product_requires_qolip(&self, item_code: &str) -> Result<bool, QolipError> {
        let item_code = item_code.trim();
        if item_code.is_empty() {
            return Ok(false);
        }
        Ok(self.store.product_spec(item_code).await?.is_some())
    }

    pub async fn order_product_requires_qolip(
        &self,
        item_code: &str,
        item_name: &str,
    ) -> Result<bool, QolipError> {
        if self.product_requires_qolip(item_code).await? {
            return Ok(true);
        }
        let item_name = item_name.trim();
        if item_name.is_empty() {
            return Ok(false);
        }
        Ok(self
            .store
            .products(item_name, 50, true)
            .await?
            .into_iter()
            .any(|product| {
                product.code.trim().eq_ignore_ascii_case(item_code.trim())
                    || product.name.trim().eq_ignore_ascii_case(item_name)
            }))
    }

    pub async fn checkout_qolip_code_for_order_start(
        &self,
        qolip_code: &str,
        expected_item_code: &str,
        expected_item_name: &str,
        worker_id: &str,
        worker_name: &str,
        principal: &Principal,
    ) -> Result<QolipCheckout, QolipError> {
        let preparation = self
            .prepare_qolip_code_for_order_start(
                qolip_code,
                expected_item_code,
                expected_item_name,
                worker_id,
                worker_name,
                principal,
            )
            .await?;
        if let Some(checkout) = preparation.checkout {
            return self.issue_prepared_checkout(checkout).await;
        }
        self.store
            .open_checkout_by_qolip_code(qolip_code)
            .await?
            .ok_or(QolipError::CheckoutRequired)
    }

    pub async fn prepare_qolip_code_for_order_start(
        &self,
        qolip_code: &str,
        expected_item_code: &str,
        expected_item_name: &str,
        worker_id: &str,
        worker_name: &str,
        principal: &Principal,
    ) -> Result<QolipOrderStartPreparation, QolipError> {
        let qolip_code = qolip_code.trim();
        if qolip_code.is_empty() {
            return Err(QolipError::MissingQolipCode);
        }
        let spec = self
            .store
            .product_spec_by_qolip_code(qolip_code)
            .await?
            .ok_or(QolipError::QolipCodeNotFound)?;
        let expected_product = self
            .order_product(expected_item_code, expected_item_name)
            .await?
            .ok_or(QolipError::QolipCodeMismatch)?;
        if !qolip_spec_matches_order(&spec, &expected_product) {
            return Err(QolipError::QolipCodeMismatch);
        }
        let existing_checkout = self.store.open_checkout_by_qolip_code(qolip_code).await?;
        if let Some(checkout) = &existing_checkout {
            if !checkout
                .issued_to_ref
                .trim()
                .eq_ignore_ascii_case(worker_id.trim())
            {
                return Err(QolipError::CheckoutAssignedToAnotherWorker);
            }
            if self
                .store
                .location_by_qolip_code(qolip_code)
                .await?
                .is_some()
            {
                return Err(QolipError::QolipInUse);
            }
        }

        let checkout = match (
            existing_checkout,
            self.store.location_by_qolip_code(qolip_code).await?,
        ) {
            (Some(_), None) => None,
            (None, Some(location)) => {
                if !qolip_location_matches_spec(&location, &spec) {
                    return Err(QolipError::QolipCodeMismatch);
                }
                let mut checkout =
                    normalize_checkout(location, 1, worker_id, worker_name, principal)?;
                checkout.item_group = spec.item_group.clone();
                Some(checkout)
            }
            (Some(_), Some(_)) => return Err(QolipError::QolipInUse),
            (None, None) => None,
        };
        Ok(QolipOrderStartPreparation { spec, checkout })
    }

    pub async fn issue_prepared_checkout(
        &self,
        checkout: QolipCheckout,
    ) -> Result<QolipCheckout, QolipError> {
        self.store.issue_checkout(checkout).await
    }

    async fn order_product(
        &self,
        item_code: &str,
        item_name: &str,
    ) -> Result<Option<QolipProduct>, QolipError> {
        let item_code = item_code.trim();
        let item_name = item_name.trim();
        let query = if item_code.is_empty() {
            item_name
        } else {
            item_code
        };
        let products = self.store.products(query, 100, false).await?;
        if !item_code.is_empty() {
            return Ok(products
                .into_iter()
                .find(|product| product.code.trim().eq_ignore_ascii_case(item_code)));
        }
        Ok(products.into_iter().find(|product| {
            !item_name.is_empty() && product.name.trim().eq_ignore_ascii_case(item_name)
        }))
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

    pub async fn location_by_qolip_code(
        &self,
        qolip_code: &str,
    ) -> Result<Option<QolipLocation>, QolipError> {
        let qolip_code = qolip_code.trim();
        if qolip_code.is_empty() {
            return Err(QolipError::MissingQolipCode);
        }
        self.store.location_by_qolip_code(qolip_code).await
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

    pub async fn open_checkouts_for_worker(
        &self,
        worker_refs: &[String],
        worker_name: &str,
        limit: usize,
    ) -> Result<Vec<QolipCheckout>, QolipError> {
        self.store
            .open_checkouts_for_worker(worker_refs, worker_name, limit.clamp(1, 500))
            .await
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
        let target = normalize_move_target(
            &source,
            &input.block,
            &input.warehouse,
            &input.row_letter,
            column_number,
            input.quantity,
        )?;
        self.store
            .move_location(
                location_id,
                &target.block,
                &target.warehouse,
                &target.row_letter,
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

include!("service_matches.rs");
