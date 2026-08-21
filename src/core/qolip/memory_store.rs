use async_trait::async_trait;
use std::collections::{BTreeMap, BTreeSet};
use tokio::sync::RwLock;

use crate::core::auth::models::Principal;

use super::models::{
    QolipBlock, QolipCellQr, QolipCheckout, QolipError, QolipLocation, QolipLocationMove,
    QolipOrderNote, QolipProduct, QolipProductSpec,
};
use super::normalize::{
    location_from_checkout, location_from_checkout_target, location_identity_matches,
    normalize_move_target, qolip_location_id, role_code,
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
    order_notes: RwLock<BTreeMap<String, QolipOrderNote>>,
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

    async fn legacy_spec(&self, location: &QolipLocation) -> QolipProductSpec {
        let item_group = self
            .products
            .read()
            .await
            .iter()
            .find(|product| {
                product
                    .code
                    .trim()
                    .eq_ignore_ascii_case(location.item_code.trim())
            })
            .map(|product| product.item_group.clone())
            .unwrap_or_default();
        QolipProductSpec {
            item_code: location.item_code.clone(),
            item_name: location.item_name.clone(),
            item_group,
            qolip_code: location.qolip_code.clone(),
            size: location.size,
            color: String::new(),
            created_by_role: location.created_by_role.clone(),
            created_by_ref: location.created_by_ref.clone(),
            created_by_name: location.created_by_name.clone(),
        }
    }

    async fn legacy_checkout_spec(&self, checkout: &QolipCheckout) -> QolipProductSpec {
        let item_group = self
            .products
            .read()
            .await
            .iter()
            .find(|product| {
                product
                    .code
                    .trim()
                    .eq_ignore_ascii_case(checkout.item_code.trim())
            })
            .map(|product| product.item_group.clone())
            .filter(|group| !group.trim().is_empty())
            .unwrap_or_else(|| checkout.item_group.clone());
        QolipProductSpec {
            item_code: checkout.item_code.clone(),
            item_name: checkout.item_name.clone(),
            item_group,
            qolip_code: checkout.qolip_code.clone(),
            size: checkout.size,
            color: String::new(),
            created_by_role: checkout.issued_by_role.clone(),
            created_by_ref: checkout.issued_by_ref.clone(),
            created_by_name: checkout.issued_by_name.clone(),
        }
    }
}

include!("memory_store_impl_parts/part_01.rs");
include!("memory_store_impl_parts/part_02.rs");
include!("memory_store_impl_parts/part_03.rs");

include!("memory_store_trait_impl.rs");

fn apply_memory_location_move(
    locations: &mut Vec<QolipLocation>,
    input: &QolipLocationMove,
) -> Result<QolipLocation, QolipError> {
    let location_id = input.location_id.trim();
    let Some(source_index) = locations.iter().position(|item| item.id == location_id) else {
        return Err(QolipError::LocationNotFound);
    };
    let source = locations[source_index].clone();
    let column_number = input.column_number.ok_or(QolipError::InvalidLocation)?;
    let target = normalize_move_target(
        &source,
        &input.block,
        &input.warehouse,
        &input.row_letter,
        column_number,
        input.quantity,
    )?;
    if let Some(existing) = locations.iter().find(|item| item.id == target.id)
        && !location_identity_matches(existing, &target)
    {
        return Err(QolipError::LocationIdentityMismatch);
    }

    let remaining = source.quantity - input.quantity;
    if remaining > 0 {
        locations[source_index].quantity = remaining;
    } else {
        locations.remove(source_index);
    }
    if let Some(target_index) = locations.iter().position(|item| item.id == target.id) {
        locations[target_index].quantity += target.quantity;
        return Ok(locations[target_index].clone());
    }
    locations.push(target.clone());
    Ok(target)
}

fn sort_locations(locations: &mut [QolipLocation]) {
    locations.sort_by(|left, right| {
        left.row_letter
            .cmp(&right.row_letter)
            .then_with(|| left.column_number.cmp(&right.column_number))
            .then_with(|| left.item_name.cmp(&right.item_name))
    });
}

fn order_note_key_prefix(principal: &Principal) -> String {
    format!("{}:{}:", role_code(&principal.role), principal.ref_.trim())
}

fn order_note_key(principal: &Principal, order_id: &str) -> String {
    format!("{}{}", order_note_key_prefix(principal), order_id.trim())
}

fn normalize_order_note_codes(codes: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for code in codes {
        let code = code.trim();
        if code.is_empty()
            || normalized
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(code))
        {
            continue;
        }
        normalized.push(code.to_string());
    }
    normalized.sort_by_key(|code| code.to_ascii_lowercase());
    normalized
}

fn memory_product_matches(product: &QolipProduct, query: &str) -> bool {
    query.is_empty()
        || product.name.to_lowercase().contains(query)
        || product.code.to_lowercase().contains(query)
        || product.qolip_code.to_lowercase().contains(query)
        || product
            .customer_names
            .iter()
            .any(|customer| customer.to_lowercase().contains(query))
}

include!("memory_store_inline_tests.rs");
