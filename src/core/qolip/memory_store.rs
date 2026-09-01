use async_trait::async_trait;
use std::collections::{BTreeMap, BTreeSet};
use tokio::sync::RwLock;

use crate::core::auth::models::Principal;

use super::models::{
    QolipBlock, QolipCellQr, QolipCheckout, QolipError, QolipLocation, QolipLocationMove,
    QolipProduct, QolipProductSpec,
};
use super::normalize::{
    location_from_checkout, location_from_checkout_target, location_identity_matches,
    normalize_move_target, qolip_location_id,
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

    async fn product_item_group(&self, item_code: &str) -> Option<String> {
        self.products
            .read()
            .await
            .iter()
            .find(|product| product.code.trim().eq_ignore_ascii_case(item_code.trim()))
            .map(|product| product.item_group.clone())
            .filter(|group| !group.trim().is_empty())
    }

    fn legacy_spec(location: &QolipLocation, item_group: Option<&str>) -> QolipProductSpec {
        QolipProductSpec {
            item_code: location.item_code.clone(),
            item_name: location.item_name.clone(),
            item_group: item_group.unwrap_or_default().to_string(),
            qolip_code: location.qolip_code.clone(),
            size: location.size,
            color: String::new(),
            created_by_role: location.created_by_role.clone(),
            created_by_ref: location.created_by_ref.clone(),
            created_by_name: location.created_by_name.clone(),
        }
    }

    fn legacy_checkout_spec(
        checkout: &QolipCheckout,
        item_group: Option<&str>,
    ) -> QolipProductSpec {
        QolipProductSpec {
            item_code: checkout.item_code.clone(),
            item_name: checkout.item_name.clone(),
            item_group: item_group
                .map(str::to_string)
                .unwrap_or_else(|| checkout.item_group.clone()),
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
    let column_number = input.column_number.ok_or(QolipError::InvalidLocation)?;
    let target = normalize_move_target(
        &locations[source_index],
        &input.block,
        &input.warehouse,
        &input.row_letter,
        column_number,
        input.quantity,
    )?;
    let mut target_index = locations.iter().position(|item| item.id == target.id);
    if let Some(index) = target_index
        && !location_identity_matches(&locations[index], &target)
    {
        return Err(QolipError::LocationIdentityMismatch);
    }

    let remaining = locations[source_index].quantity - input.quantity;
    if remaining > 0 {
        locations[source_index].quantity = remaining;
    } else {
        locations.remove(source_index);
        if let Some(index) = &mut target_index
            && *index > source_index
        {
            *index -= 1;
        }
    }
    if let Some(index) = target_index {
        locations[index].quantity += target.quantity;
        return Ok(locations[index].clone());
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

fn memory_product_matches(product: &QolipProduct, query: &str) -> bool {
    query.is_empty()
        || contains_case_insensitive(&product.name, query)
        || contains_case_insensitive(&product.code, query)
        || contains_case_insensitive(&product.qolip_code, query)
        || product
            .customer_names
            .iter()
            .any(|customer| contains_case_insensitive(customer, query))
}

fn contains_case_insensitive(value: &str, lowercase_query: &str) -> bool {
    if value.is_ascii() && lowercase_query.is_ascii() {
        let query = lowercase_query.as_bytes();
        return value
            .as_bytes()
            .windows(query.len())
            .any(|window| window.eq_ignore_ascii_case(query));
    }
    value.to_lowercase().contains(lowercase_query)
}

include!("memory_store_inline_tests.rs");
