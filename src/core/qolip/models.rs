use serde::{Deserialize, Serialize};

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
