use async_trait::async_trait;
use std::collections::BTreeMap;
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
        let specs = self.product_specs.read().await;
        let products = self.products.read().await;
        Ok(products
            .iter()
            .filter_map(|product| {
                let spec = specs.get(&product.code.trim().to_lowercase());
                if with_qolip_only && spec.is_none() {
                    return None;
                }
                let matches = query.is_empty()
                    || product.name.to_lowercase().contains(&query)
                    || product.code.to_lowercase().contains(&query);
                if !matches {
                    return None;
                }
                let mut product = product.clone();
                if let Some(spec) = spec {
                    product.qolip_code = spec.qolip_code.clone();
                    product.size = spec.size;
                    product.has_qolip_spec = true;
                }
                Some(product)
            })
            .take(limit.max(1))
            .collect())
    }

    async fn product_spec(&self, item_code: &str) -> Result<Option<QolipProductSpec>, QolipError> {
        Ok(self
            .product_specs
            .read()
            .await
            .get(&item_code.trim().to_lowercase())
            .cloned())
    }

    async fn put_product_spec(
        &self,
        spec: QolipProductSpec,
    ) -> Result<QolipProductSpec, QolipError> {
        self.product_specs
            .write()
            .await
            .insert(spec.item_code.trim().to_lowercase(), spec.clone());
        Ok(spec)
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
        if let Some(index) = locations.iter().position(|item| item.id == location.id) {
            let existing = &locations[index];
            if !location_identity_matches(existing, &location) {
                return Err(QolipError::LocationIdentityMismatch);
            }
            let mut merged = location.clone();
            merged.quantity += existing.quantity;
            locations[index] = merged.clone();
            locations.sort_by(|left, right| {
                left.row_letter
                    .cmp(&right.row_letter)
                    .then_with(|| left.column_number.cmp(&right.column_number))
                    .then_with(|| left.item_name.cmp(&right.item_name))
            });
            return Ok(merged);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn location(id: &str, item_code: &str, quantity: i32) -> QolipLocation {
        QolipLocation {
            id: id.to_string(),
            block: "A".to_string(),
            warehouse: "Qolip ombor".to_string(),
            item_code: item_code.to_string(),
            item_name: item_code.to_string(),
            qolip_code: "Q-1".to_string(),
            size: 40,
            quantity,
            row_letter: "C".to_string(),
            column_number: Some(2),
            location_label: "C2".to_string(),
            created_by_role: "admin".to_string(),
            created_by_ref: "admin".to_string(),
            created_by_name: "Admin".to_string(),
        }
    }

    fn checkout(id: &str, location_id: &str, item_code: &str, status: &str) -> QolipCheckout {
        QolipCheckout {
            id: id.to_string(),
            location_id: location_id.to_string(),
            block: "A".to_string(),
            warehouse: "Qolip ombor".to_string(),
            item_code: item_code.to_string(),
            item_name: item_code.to_string(),
            qolip_code: "Q-1".to_string(),
            size: 40,
            quantity: 2,
            row_letter: "C".to_string(),
            column_number: Some(2),
            location_label: "C2".to_string(),
            issued_to_ref: "worker".to_string(),
            issued_to_name: "Worker".to_string(),
            status: status.to_string(),
            issued_by_role: "admin".to_string(),
            issued_by_ref: "admin".to_string(),
            issued_by_name: "Admin".to_string(),
            issued_at: "1970-01-01T00:00:00Z".to_string(),
        }
    }

    #[tokio::test]
    async fn issue_checkout_rejects_location_identity_mismatch() {
        let store = MemoryQolipStore::default();
        store
            .locations
            .write()
            .await
            .push(location("loc-1", "ITEM-A", 5));

        let result = store
            .issue_checkout(checkout("checkout-1", "loc-1", "ITEM-B", "open"))
            .await;

        assert!(matches!(result, Err(QolipError::LocationIdentityMismatch)));
    }

    #[tokio::test]
    async fn return_checkout_rejects_location_identity_mismatch() {
        let store = MemoryQolipStore::default();
        store
            .checkouts
            .write()
            .await
            .push(checkout("checkout-1", "loc-1", "ITEM-A", "open"));
        store
            .locations
            .write()
            .await
            .push(location("qolip:a:item_a:q_1:40:c:2", "ITEM-B", 5));

        let result = store.return_checkout("checkout-1", "", None).await;

        assert!(matches!(result, Err(QolipError::LocationIdentityMismatch)));
        let saved = store
            .checkout_by_id("checkout-1")
            .await
            .expect("checkout lookup")
            .expect("checkout");
        assert_eq!(saved.status, "open");
    }

    #[tokio::test]
    async fn move_location_rejects_target_identity_mismatch() {
        let store = MemoryQolipStore::default();
        store.locations.write().await.extend([
            location("source", "ITEM-A", 5),
            location("qolip:a:item_a:q_1:40:d:3", "ITEM-B", 4),
        ]);

        let result = store.move_location("source", "D", 3, 2).await;

        assert!(matches!(result, Err(QolipError::LocationIdentityMismatch)));
    }
}
