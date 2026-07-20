use async_trait::async_trait;
use std::collections::{BTreeMap, BTreeSet};
use tokio::sync::RwLock;

use crate::core::auth::models::Principal;

use super::models::{
    QolipBlock, QolipCellQr, QolipCheckout, QolipError, QolipLocation, QolipProduct,
    QolipProductSpec,
};
use super::normalize::{
    location_from_checkout, location_from_checkout_target, location_identity_matches,
    normalize_move_target,
};
use super::ports::QolipStorePort;

#[derive(Default)]
pub struct MemoryQolipStore {
    blocks: RwLock<Vec<QolipBlock>>,
    products: RwLock<Vec<QolipProduct>>,
    product_specs: RwLock<BTreeMap<String, QolipProductSpec>>,
    locations: RwLock<Vec<QolipLocation>>,
    cell_qrs: RwLock<BTreeMap<String, QolipCellQr>>,
    checkouts: RwLock<Vec<QolipCheckout>>,
}

impl MemoryQolipStore {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub async fn seed_blocks(&self, blocks: Vec<QolipBlock>) {
        *self.blocks.write().await = blocks;
    }

    #[cfg(test)]
    pub async fn seed_products(&self, products: Vec<QolipProduct>) {
        *self.products.write().await = products;
    }
}

#[async_trait]
impl QolipStorePort for MemoryQolipStore {
    async fn assigned_warehouses(&self, _principal: &Principal) -> Result<Vec<String>, QolipError> {
        let mut warehouses = self
            .blocks
            .read()
            .await
            .iter()
            .map(|block| block.warehouse.trim().to_string())
            .filter(|warehouse| !warehouse.is_empty())
            .collect::<Vec<_>>();
        warehouses.sort_by_key(|warehouse| warehouse.to_lowercase());
        warehouses.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
        Ok(warehouses)
    }

    async fn assigned_blocks(&self, _principal: &Principal) -> Result<Vec<QolipBlock>, QolipError> {
        Ok(self.blocks.read().await.clone())
    }

    async fn all_blocks(&self) -> Result<Vec<QolipBlock>, QolipError> {
        Ok(self.blocks.read().await.clone())
    }

    async fn products(
        &self,
        query: &str,
        limit: usize,
        with_qolip_only: bool,
    ) -> Result<Vec<QolipProduct>, QolipError> {
        let query = query.trim().to_lowercase();
        let in_use_codes = self
            .checkouts
            .read()
            .await
            .iter()
            .filter(|checkout| checkout.status.trim().eq_ignore_ascii_case("open"))
            .map(|checkout| checkout.qolip_code.trim().to_lowercase())
            .collect::<BTreeSet<_>>();
        let specs = self.product_specs.read().await;
        let products = self.products.read().await;
        let mut items = Vec::new();
        for product in products.iter() {
            let product_specs = specs
                .values()
                .filter(|spec| {
                    spec.item_code
                        .trim()
                        .eq_ignore_ascii_case(product.code.trim())
                })
                .collect::<Vec<_>>();
            if with_qolip_only && product_specs.is_empty() {
                continue;
            }
            if product_specs.is_empty() {
                let matches = query.is_empty()
                    || product.name.to_lowercase().contains(&query)
                    || product.code.to_lowercase().contains(&query);
                if matches {
                    let mut item = product.clone();
                    item.is_in_use = false;
                    items.push(item);
                }
                continue;
            }
            for spec in product_specs {
                let matches = query.is_empty()
                    || product.name.to_lowercase().contains(&query)
                    || product.code.to_lowercase().contains(&query)
                    || spec.qolip_code.to_lowercase().contains(&query);
                if matches {
                    let mut item = product.clone();
                    item.qolip_code = spec.qolip_code.clone();
                    item.size = spec.size;
                    item.has_qolip_spec = true;
                    item.is_in_use = in_use_codes.contains(&spec.qolip_code.trim().to_lowercase());
                    items.push(item);
                }
            }
        }
        items.truncate(limit.max(1));
        Ok(items)
    }

    async fn product_spec(&self, item_code: &str) -> Result<Option<QolipProductSpec>, QolipError> {
        Ok(self
            .product_specs
            .read()
            .await
            .values()
            .find(|spec| spec.item_code.trim().eq_ignore_ascii_case(item_code.trim()))
            .cloned())
    }

    async fn product_spec_by_qolip_code(
        &self,
        qolip_code: &str,
    ) -> Result<Option<QolipProductSpec>, QolipError> {
        let qolip_code = qolip_code.trim();
        Ok(self
            .product_specs
            .read()
            .await
            .values()
            .find(|spec| spec.qolip_code.trim().eq_ignore_ascii_case(qolip_code))
            .cloned())
    }

    async fn put_product_spec(
        &self,
        spec: QolipProductSpec,
    ) -> Result<QolipProductSpec, QolipError> {
        let mut products = self.products.write().await;
        if let Some(product) = products.iter_mut().find(|product| {
            product
                .code
                .trim()
                .eq_ignore_ascii_case(spec.item_code.trim())
        }) {
            product.qolip_code = spec.qolip_code.clone();
            product.size = spec.size;
            product.has_qolip_spec = true;
        } else {
            products.push(QolipProduct {
                code: spec.item_code.clone(),
                name: spec.item_name.clone(),
                item_group: spec.item_group.clone(),
                customer_names: Vec::new(),
                qolip_code: spec.qolip_code.clone(),
                size: spec.size,
                has_qolip_spec: true,
                is_in_use: false,
            });
        }
        drop(products);
        self.product_specs
            .write()
            .await
            .insert(spec.qolip_code.trim().to_lowercase(), spec.clone());
        Ok(spec)
    }

    async fn delete_product_specs(&self, qolip_codes: &[String]) -> Result<usize, QolipError> {
        let normalized = qolip_codes
            .iter()
            .map(|code| code.trim().to_lowercase())
            .filter(|code| !code.is_empty())
            .collect::<BTreeSet<_>>();
        if normalized.is_empty() {
            return Err(QolipError::MissingQolipCode);
        }
        if self.checkouts.read().await.iter().any(|checkout| {
            checkout.status.trim().eq_ignore_ascii_case("open")
                && normalized.contains(&checkout.qolip_code.trim().to_lowercase())
        }) {
            return Err(QolipError::QolipInUse);
        }
        let mut specs = self.product_specs.write().await;
        let before = specs.len();
        specs.retain(|code, _| !normalized.contains(&code.trim().to_lowercase()));
        let deleted = before - specs.len();
        drop(specs);
        self.locations
            .write()
            .await
            .retain(|location| !normalized.contains(&location.qolip_code.trim().to_lowercase()));
        Ok(deleted)
    }

    async fn locations(&self, block: &str) -> Result<Vec<QolipLocation>, QolipError> {
        let block = block.trim().to_lowercase();
        Ok(self
            .locations
            .read()
            .await
            .iter()
            .filter(|location| location.block.to_lowercase() == block)
            .cloned()
            .collect())
    }

    async fn put_location(&self, location: QolipLocation) -> Result<QolipLocation, QolipError> {
        let mut locations = self.locations.write().await;
        locations.retain(|item| {
            !item
                .qolip_code
                .trim()
                .eq_ignore_ascii_case(location.qolip_code.trim())
        });
        locations.push(location.clone());
        locations.sort_by(|left, right| {
            left.row_letter
                .cmp(&right.row_letter)
                .then_with(|| left.column_number.cmp(&right.column_number))
                .then_with(|| left.item_name.cmp(&right.item_name))
        });
        Ok(location)
    }

    async fn get_or_create_cell_qr(&self, cell: QolipCellQr) -> Result<QolipCellQr, QolipError> {
        let mut cell_qrs = self.cell_qrs.write().await;
        if let Some(existing) = cell_qrs.get(&cell.id) {
            return Ok(existing.clone());
        }
        cell_qrs.insert(cell.id.clone(), cell.clone());
        Ok(cell)
    }

    async fn location_by_id(&self, location_id: &str) -> Result<Option<QolipLocation>, QolipError> {
        let location_id = location_id.trim();
        Ok(self
            .locations
            .read()
            .await
            .iter()
            .find(|location| location.id == location_id)
            .cloned())
    }

    async fn location_by_qolip_code(
        &self,
        qolip_code: &str,
    ) -> Result<Option<QolipLocation>, QolipError> {
        let qolip_code = qolip_code.trim();
        Ok(self
            .locations
            .read()
            .await
            .iter()
            .find(|location| location.qolip_code.trim().eq_ignore_ascii_case(qolip_code))
            .cloned())
    }

    async fn issue_checkout(&self, checkout: QolipCheckout) -> Result<QolipCheckout, QolipError> {
        let mut locations = self.locations.write().await;
        let Some(index) = locations
            .iter()
            .position(|location| location.id == checkout.location_id)
        else {
            return Err(QolipError::LocationNotFound);
        };
        let expected = location_from_checkout(&checkout);
        if !location_identity_matches(&locations[index], &expected) {
            return Err(QolipError::LocationIdentityMismatch);
        }
        if checkout.quantity > locations[index].quantity {
            return Err(QolipError::InsufficientStock);
        }
        let remaining = locations[index].quantity - checkout.quantity;
        if remaining > 0 {
            locations[index].quantity = remaining;
        } else {
            locations.remove(index);
        }
        drop(locations);

        let mut saved = checkout.clone();
        if saved.issued_at.is_empty() {
            saved.issued_at = "1970-01-01T00:00:00Z".to_string();
        }
        self.checkouts.write().await.push(saved.clone());
        Ok(saved)
    }

    async fn checkouts(
        &self,
        block: Option<&str>,
        allowed_blocks: Option<&[String]>,
        status: &str,
        limit: usize,
    ) -> Result<Vec<QolipCheckout>, QolipError> {
        let status = status.trim().to_lowercase();
        let block = block.map(str::trim).filter(|value| !value.is_empty());
        let mut items = self
            .checkouts
            .read()
            .await
            .iter()
            .filter(|checkout| checkout.status.to_lowercase() == status)
            .filter(|checkout| {
                if let Some(block) = block {
                    checkout.block.eq_ignore_ascii_case(block)
                } else if let Some(allowed_blocks) = allowed_blocks {
                    allowed_blocks
                        .iter()
                        .any(|block| checkout.block.eq_ignore_ascii_case(block))
                } else {
                    true
                }
            })
            .cloned()
            .collect::<Vec<_>>();
        items.sort_by(|left, right| right.issued_at.cmp(&left.issued_at));
        Ok(items.into_iter().take(limit.max(1)).collect())
    }

    async fn checkout_by_id(&self, checkout_id: &str) -> Result<Option<QolipCheckout>, QolipError> {
        let checkout_id = checkout_id.trim();
        Ok(self
            .checkouts
            .read()
            .await
            .iter()
            .find(|checkout| checkout.id == checkout_id)
            .cloned())
    }

    async fn return_checkout(
        &self,
        checkout_id: &str,
        row_letter: &str,
        column_number: Option<i32>,
    ) -> Result<QolipCheckout, QolipError> {
        let checkout_id = checkout_id.trim();
        let mut checkouts = self.checkouts.write().await;
        let Some(index) = checkouts
            .iter()
            .position(|checkout| checkout.id == checkout_id)
        else {
            return Err(QolipError::CheckoutNotFound);
        };
        if !checkouts[index].status.eq_ignore_ascii_case("open") {
            return Err(QolipError::CheckoutNotReturnable);
        }
        let checkout = checkouts[index].clone();
        let restore = location_from_checkout_target(&checkout, row_letter, column_number)?;
        let mut locations = self.locations.write().await;
        if let Some(target_index) = locations.iter().position(|item| item.id == restore.id) {
            if !location_identity_matches(&locations[target_index], &restore) {
                return Err(QolipError::LocationIdentityMismatch);
            }
            locations[target_index].quantity += restore.quantity;
        } else {
            locations.push(restore);
        }
        checkouts[index].status = "returned".to_string();
        let checkout = checkouts[index].clone();
        locations.sort_by(|left, right| {
            left.row_letter
                .cmp(&right.row_letter)
                .then_with(|| left.column_number.cmp(&right.column_number))
                .then_with(|| left.item_name.cmp(&right.item_name))
        });
        Ok(checkout)
    }

    async fn move_location(
        &self,
        location_id: &str,
        row_letter: &str,
        column_number: i32,
        quantity: i32,
    ) -> Result<QolipLocation, QolipError> {
        let location_id = location_id.trim();
        let mut locations = self.locations.write().await;
        let Some(source_index) = locations.iter().position(|item| item.id == location_id) else {
            return Err(QolipError::LocationNotFound);
        };
        let source = locations[source_index].clone();
        let target = normalize_move_target(&source, row_letter, column_number, quantity)?;
        let remaining = source.quantity - quantity;
        if remaining > 0 {
            locations[source_index].quantity = remaining;
        } else {
            locations.remove(source_index);
        }
        if let Some(target_index) = locations.iter().position(|item| item.id == target.id) {
            if !location_identity_matches(&locations[target_index], &target) {
                return Err(QolipError::LocationIdentityMismatch);
            }
            locations[target_index].quantity += target.quantity;
            let saved = locations[target_index].clone();
            locations.sort_by(|left, right| {
                left.row_letter
                    .cmp(&right.row_letter)
                    .then_with(|| left.column_number.cmp(&right.column_number))
                    .then_with(|| left.item_name.cmp(&right.item_name))
            });
            return Ok(saved);
        }
        locations.push(target.clone());
        locations.sort_by(|left, right| {
            left.row_letter
                .cmp(&right.row_letter)
                .then_with(|| left.column_number.cmp(&right.column_number))
                .then_with(|| left.item_name.cmp(&right.item_name))
        });
        Ok(target)
    }

    async fn cell_qr_by_payload(
        &self,
        qr_payload: &str,
    ) -> Result<Option<QolipCellQr>, QolipError> {
        let qr_payload = qr_payload.trim();
        Ok(self
            .cell_qrs
            .read()
            .await
            .values()
            .find(|cell| cell.qr_payload.eq_ignore_ascii_case(qr_payload))
            .cloned())
    }
}

include!("memory_store_inline_tests.rs");
