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
    #[serde(default)]
    pub customer_names: Vec<String>,
    pub qolip_code: String,
    pub size: i32,
    pub has_qolip_spec: bool,
    #[serde(default)]
    pub is_in_use: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QolipProductSpec {
    pub item_code: String,
    pub item_name: String,
    pub item_group: String,
    pub qolip_code: String,
    pub size: i32,
    pub created_by_role: String,
    pub created_by_ref: String,
    pub created_by_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct QolipProductSpecUpsert {
    #[serde(default)]
    pub item_code: String,
    #[serde(default)]
    pub item_name: String,
    #[serde(default)]
    pub item_group: String,
    #[serde(default)]
    pub qolip_code: String,
    #[serde(default)]
    pub size: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct QolipProductSpecDelete {
    #[serde(default)]
    pub qolip_codes: Vec<String>,
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QolipCellQr {
    pub id: String,
    pub block: String,
    pub warehouse: String,
    pub row_letter: String,
    pub column_number: i32,
    pub location_label: String,
    pub qr_payload: String,
    pub created_by_role: String,
    pub created_by_ref: String,
    pub created_by_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct QolipCellQrInput {
    #[serde(default)]
    pub block: String,
    #[serde(default)]
    pub warehouse: String,
    #[serde(default)]
    pub row_letter: String,
    #[serde(default)]
    pub column_number: Option<i32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QolipCheckout {
    pub id: String,
    pub location_id: String,
    pub block: String,
    pub warehouse: String,
    pub item_code: String,
    pub item_name: String,
    #[serde(default)]
    pub item_group: String,
    pub qolip_code: String,
    pub size: i32,
    pub quantity: i32,
    pub row_letter: String,
    pub column_number: Option<i32>,
    pub location_label: String,
    pub issued_to_ref: String,
    pub issued_to_name: String,
    pub status: String,
    pub issued_by_role: String,
    pub issued_by_ref: String,
    pub issued_by_name: String,
    pub issued_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QolipOrderStartPreparation {
    pub spec: QolipProductSpec,
    pub checkout: Option<QolipCheckout>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct QolipCheckoutCreate {
    #[serde(default)]
    pub location_id: String,
    #[serde(default)]
    pub quantity: i32,
    #[serde(default)]
    pub worker_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct QolipCheckoutReturn {
    #[serde(default)]
    pub checkout_id: String,
    #[serde(default)]
    pub row_letter: String,
    #[serde(default)]
    pub column_number: Option<i32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct QolipLocationMove {
    #[serde(default)]
    pub location_id: String,
    #[serde(default)]
    pub quantity: i32,
    #[serde(default)]
    pub row_letter: String,
    #[serde(default)]
    pub column_number: Option<i32>,
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
    pub item_group: String,
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
    #[error("item group is required")]
    MissingItemGroup,
    #[error("qolip code is required")]
    MissingQolipCode,
    #[error("qolip code not found")]
    QolipCodeNotFound,
    #[error("qolip code does not match item")]
    QolipCodeMismatch,
    #[error("qolip is in use")]
    QolipInUse,
    #[error("size is required")]
    InvalidSize,
    #[error("quantity is required")]
    InvalidQuantity,
    #[error("location is invalid")]
    InvalidLocation,
    #[error("location not found")]
    LocationNotFound,
    #[error("worker is required")]
    MissingWorker,
    #[error("worker not found")]
    WorkerNotFound,
    #[error("insufficient stock")]
    InsufficientStock,
    #[error("checkout not found")]
    CheckoutNotFound,
    #[error("checkout not returnable")]
    CheckoutNotReturnable,
    #[error("cell qr not found")]
    CellQrNotFound,
    #[error("location identity mismatch")]
    LocationIdentityMismatch,
    #[error("qolip store failed")]
    StoreFailed,
}
