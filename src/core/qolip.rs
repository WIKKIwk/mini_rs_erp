use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::core::auth::models::{Principal, PrincipalRole};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QolipBlock {
    pub name: String,
    pub warehouse: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QolipProduct {
    pub code: String,
    pub name: String,
    pub item_group: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QolipLocation {
    pub id: String,
    pub block: String,
    pub warehouse: String,
    pub item_code: String,
    pub item_name: String,
    pub qolip_code: String,
    pub size: i32,
    pub quantity: i32,
    pub row_letter: String,
    pub column_number: Option<i32>,
    pub location_label: String,
    pub created_by_role: String,
    pub created_by_ref: String,
    pub created_by_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct QolipLocationUpsert {
    #[serde(default)]
    pub block: String,
    #[serde(default)]
    pub warehouse: String,
    #[serde(default)]
    pub item_code: String,
    #[serde(default)]
    pub item_name: String,
    #[serde(default)]
    pub qolip_code: String,
    #[serde(default)]
    pub size: i32,
    #[serde(default)]
    pub quantity: i32,
    #[serde(default)]
    pub row_letter: String,
    #[serde(default)]
    pub column_number: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum QolipError {
    #[error("block is required")]
    MissingBlock,
    #[error("item is required")]
    MissingItem,
    #[error("qolip code is required")]
    MissingQolipCode,
    #[error("size is required")]
    InvalidSize,
    #[error("quantity is required")]
    InvalidQuantity,
    #[error("location is invalid")]
    InvalidLocation,
    #[error("qolip store failed")]
    StoreFailed,
}

#[async_trait]
pub trait QolipStorePort: Send + Sync {
    async fn assigned_blocks(&self, principal: &Principal) -> Result<Vec<QolipBlock>, QolipError>;
    async fn products(&self, query: &str, limit: usize) -> Result<Vec<QolipProduct>, QolipError>;
    async fn locations(&self, block: &str) -> Result<Vec<QolipLocation>, QolipError>;
    async fn put_location(&self, location: QolipLocation) -> Result<QolipLocation, QolipError>;
}

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
        self.store.assigned_blocks(principal).await
    }

    pub async fn products(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<QolipProduct>, QolipError> {
        self.store.products(query, limit.clamp(1, 100)).await
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
        input: QolipLocationUpsert,
        principal: &Principal,
    ) -> Result<QolipLocation, QolipError> {
        let normalized = normalize_location(input, principal)?;
        self.store.put_location(normalized).await
    }
}

fn normalize_location(
    input: QolipLocationUpsert,
    principal: &Principal,
) -> Result<QolipLocation, QolipError> {
    let block = input.block.trim().to_string();
    if block.is_empty() {
        return Err(QolipError::MissingBlock);
    }
    let item_code = input.item_code.trim().to_string();
    let item_name = input.item_name.trim().to_string();
    if item_code.is_empty() || item_name.is_empty() {
        return Err(QolipError::MissingItem);
    }
    let qolip_code = input.qolip_code.trim().to_string();
    if qolip_code.is_empty() {
        return Err(QolipError::MissingQolipCode);
    }
    if input.size <= 0 {
        return Err(QolipError::InvalidSize);
    }
    if input.quantity <= 0 {
        return Err(QolipError::InvalidQuantity);
    }
    let row_letter = normalize_row_letter(&input.row_letter)?;
    let column_number = normalize_column_number(input.column_number, row_letter.as_deref())?;
    let location_label = match (row_letter.as_deref(), column_number) {
        (Some(row), Some(column)) => format!("{row}{column}"),
        _ => String::new(),
    };
    let role = role_code(&principal.role).to_string();
    let warehouse = input.warehouse.trim().to_string();
    let id = qolip_location_id(
        &block,
        &item_code,
        &qolip_code,
        input.size,
        row_letter.as_deref().unwrap_or(""),
        column_number,
    );
    Ok(QolipLocation {
        id,
        block,
        warehouse,
        item_code,
        item_name,
        qolip_code,
        size: input.size,
        quantity: input.quantity,
        row_letter: row_letter.unwrap_or_default(),
        column_number,
        location_label,
        created_by_role: role,
        created_by_ref: principal.ref_.trim().to_string(),
        created_by_name: principal.display_name.trim().to_string(),
    })
}

fn normalize_row_letter(value: &str) -> Result<Option<String>, QolipError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let mut chars = trimmed.chars();
    let Some(ch) = chars.next() else {
        return Ok(None);
    };
    if chars.next().is_some() || !ch.is_ascii_alphabetic() {
        return Err(QolipError::InvalidLocation);
    }
    Ok(Some(ch.to_ascii_uppercase().to_string()))
}

fn normalize_column_number(
    value: Option<i32>,
    row_letter: Option<&str>,
) -> Result<Option<i32>, QolipError> {
    match (row_letter, value) {
        (None, None) => Ok(None),
        (Some(_), Some(column)) if (1..=9).contains(&column) => Ok(Some(column)),
        _ => Err(QolipError::InvalidLocation),
    }
}

fn qolip_location_id(
    block: &str,
    item_code: &str,
    qolip_code: &str,
    size: i32,
    row_letter: &str,
    column_number: Option<i32>,
) -> String {
    format!(
        "qolip:{}:{}:{}:{}:{}:{}",
        compact_key(block),
        compact_key(item_code),
        compact_key(qolip_code),
        size,
        compact_key(row_letter),
        column_number.unwrap_or_default()
    )
}

fn compact_key(value: &str) -> String {
    let mut key = value
        .trim()
        .to_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>();
    while key.contains("__") {
        key = key.replace("__", "_");
    }
    key.trim_matches('_').to_string()
}

pub fn role_code(role: &PrincipalRole) -> &'static str {
    match role {
        PrincipalRole::Supplier => "supplier",
        PrincipalRole::Werka => "werka",
        PrincipalRole::Customer => "customer",
        PrincipalRole::Aparatchi => "aparatchi",
        PrincipalRole::Qolipchi => "qolipchi",
        PrincipalRole::Admin => "admin",
    }
}

#[derive(Default)]
pub struct MemoryQolipStore {
    blocks: RwLock<Vec<QolipBlock>>,
    products: RwLock<Vec<QolipProduct>>,
    locations: RwLock<Vec<QolipLocation>>,
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
    async fn assigned_blocks(&self, _principal: &Principal) -> Result<Vec<QolipBlock>, QolipError> {
        Ok(self.blocks.read().await.clone())
    }

    async fn products(&self, query: &str, limit: usize) -> Result<Vec<QolipProduct>, QolipError> {
        let query = query.trim().to_lowercase();
        Ok(self
            .products
            .read()
            .await
            .iter()
            .filter(|product| {
                query.is_empty()
                    || product.name.to_lowercase().contains(&query)
                    || product.code.to_lowercase().contains(&query)
            })
            .take(limit.max(1))
            .cloned()
            .collect())
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
            locations[index] = location.clone();
        } else {
            locations.push(location.clone());
        }
        locations.sort_by(|left, right| {
            left.row_letter
                .cmp(&right.row_letter)
                .then_with(|| left.column_number.cmp(&right.column_number))
                .then_with(|| left.item_name.cmp(&right.item_name))
        });
        Ok(location)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn principal() -> Principal {
        Principal {
            role: PrincipalRole::Qolipchi,
            display_name: "Ali".to_string(),
            legal_name: "Ali".to_string(),
            ref_: "worker-1".to_string(),
            phone: "+998901234567".to_string(),
            avatar_url: String::new(),
        }
    }

    #[test]
    fn normalize_location_requires_numeric_size_and_column_range() {
        let base = QolipLocationUpsert {
            block: "A".to_string(),
            item_code: "VELONA".to_string(),
            item_name: "Velona".to_string(),
            qolip_code: "Q-1".to_string(),
            size: 12,
            quantity: 9,
            row_letter: "a".to_string(),
            column_number: Some(1),
            ..QolipLocationUpsert::default()
        };
        let normalized = normalize_location(base.clone(), &principal()).expect("valid location");
        assert_eq!(normalized.row_letter, "A");
        assert_eq!(normalized.location_label, "A1");

        let invalid = QolipLocationUpsert {
            column_number: Some(10),
            ..base
        };
        assert_eq!(
            normalize_location(invalid, &principal()),
            Err(QolipError::InvalidLocation)
        );
    }
}
