use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use serde::de::Error as _;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::core::auth::models::{Principal, PrincipalRole};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReturnedPaintItem {
    pub usage: String,
    pub category: String,
    pub name: String,
    #[serde(deserialize_with = "deserialize_decimal_values")]
    pub values: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReturnedPaintCalculation {
    pub rasxot_mix_total: String,
    pub astatka_mix_total: String,
    pub rasxot_alcohol: String,
    pub astatka_alcohol: String,
    pub final_used_alcohol: String,
    pub rasxot_pure_paint: String,
    pub astatka_pure_paint: String,
    pub final_used_paint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReturnedPaintStatus {
    WaitingForBoyoqchiInput,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReturnedPaintImage {
    pub image_id: String,
    pub image_name: String,
    pub image_mime: String,
    pub image_size_bytes: u64,
    pub image_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnedPaintStoredImage {
    pub image: ReturnedPaintImage,
    pub order_id: String,
    pub apparatus: String,
    pub owner_ref: String,
    pub body: Vec<u8>,
}

fn deserialize_decimal_values<'de, D>(deserializer: D) -> Result<BTreeMap<String, String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let values = BTreeMap::<String, serde_json::Value>::deserialize(deserializer)?;
    values
        .into_iter()
        .map(|(label, value)| {
            let value = match value {
                serde_json::Value::Number(value) => value.to_string(),
                serde_json::Value::String(value) => value,
                _ => return Err(D::Error::custom("returned paint value must be decimal")),
            };
            Ok((label, value))
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReturnedPaintRequestCreate {
    pub order_id: String,
    #[serde(default)]
    pub order_code: String,
    #[serde(default)]
    pub order_name: String,
    pub apparatus: String,
    #[serde(default)]
    pub image_id: String,
    #[serde(default)]
    pub items: Vec<ReturnedPaintItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReturnedPaintRequestComplete {
    pub request_id: String,
    pub items: Vec<ReturnedPaintItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReturnedPaintRequest {
    pub id: String,
    pub order_id: String,
    pub order_code: String,
    pub order_name: String,
    pub apparatus: String,
    pub sender_role: PrincipalRole,
    pub sender_ref: String,
    pub sender_display_name: String,
    pub items: Vec<ReturnedPaintItem>,
    pub status: ReturnedPaintStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<ReturnedPaintImage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calculation: Option<ReturnedPaintCalculation>,
    #[serde(default)]
    pub message: String,
    pub created_at_unix: i64,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ReturnedPaintError {
    #[error("order id is required")]
    MissingOrderId,
    #[error("apparatus is required")]
    MissingApparatus,
    #[error("at least one returned paint value is required")]
    MissingItems,
    #[error("at least three returned paint fields are required in both tabs")]
    InsufficientValues,
    #[error("returned paint request was not found")]
    RequestNotFound,
    #[error("returned paint image was not found")]
    ImageNotFound,
    #[error("returned paint image does not belong to this order")]
    ImageMismatch,
    #[error("returned paint image cannot be removed")]
    ImageDeleteNotAllowed,
    #[error("returned paint usage is invalid")]
    InvalidUsage,
    #[error("returned paint category is invalid")]
    InvalidCategory,
    #[error("returned paint item name is required")]
    MissingItemName,
    #[error("returned paint item values are required")]
    MissingValues,
    #[error("returned paint value is invalid")]
    InvalidValue,
    #[error("astatka cannot exceed rasxot")]
    NegativeFinalValue,
    #[error("returned paint store failed")]
    StoreFailed,
}

#[async_trait]
pub trait ReturnedPaintStorePort: Send + Sync {
    async fn create(
        &self,
        request: ReturnedPaintRequest,
    ) -> Result<ReturnedPaintRequest, ReturnedPaintError>;

    async fn list(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<ReturnedPaintRequest>, ReturnedPaintError>;

    async fn complete(
        &self,
        request_id: &str,
        items: Vec<ReturnedPaintItem>,
        calculation: ReturnedPaintCalculation,
    ) -> Result<ReturnedPaintRequest, ReturnedPaintError>;

    async fn save_image(
        &self,
        image: ReturnedPaintStoredImage,
    ) -> Result<ReturnedPaintStoredImage, ReturnedPaintError>;

    async fn image(
        &self,
        image_id: &str,
    ) -> Result<Option<ReturnedPaintStoredImage>, ReturnedPaintError>;

    async fn delete_image(
        &self,
        image_id: &str,
        owner_ref: &str,
    ) -> Result<bool, ReturnedPaintError>;
}

#[derive(Clone)]
pub struct ReturnedPaintService {
    store: Arc<dyn ReturnedPaintStorePort>,
}

impl ReturnedPaintService {
    pub fn new(store: Arc<dyn ReturnedPaintStorePort>) -> Self {
        Self { store }
    }

    pub fn unavailable() -> Self {
        Self::new(Arc::new(UnavailableReturnedPaintStore))
    }

    pub async fn create(
        &self,
        input: ReturnedPaintRequestCreate,
        sender: &Principal,
    ) -> Result<ReturnedPaintRequest, ReturnedPaintError> {
        let bytes: [u8; 12] = rand::random();
        let request = self
            .prepare_request(
                input,
                sender,
                format!("returned_paint_{}", data_encoding::HEXLOWER.encode(&bytes)),
            )
            .await?;
        self.store.create(request).await
    }

    pub async fn prepare_request(
        &self,
        input: ReturnedPaintRequestCreate,
        sender: &Principal,
        id: String,
    ) -> Result<ReturnedPaintRequest, ReturnedPaintError> {
        let order_id = required_text(input.order_id, ReturnedPaintError::MissingOrderId)?;
        let apparatus = required_text(input.apparatus, ReturnedPaintError::MissingApparatus)?;
        let sender_ref = sender.ref_.trim();
        let sender_display_name = sender.display_name.trim();
        if sender_ref.is_empty() || sender_display_name.is_empty() || id.trim().is_empty() {
            return Err(ReturnedPaintError::StoreFailed);
        }
        let image = if input.image_id.trim().is_empty() {
            None
        } else {
            let stored = self
                .store
                .image(input.image_id.trim())
                .await?
                .ok_or(ReturnedPaintError::ImageNotFound)?;
            if stored.owner_ref.trim() != sender_ref
                || stored.order_id.trim() != order_id
                || !stored.apparatus.trim().eq_ignore_ascii_case(&apparatus)
            {
                return Err(ReturnedPaintError::ImageMismatch);
            }
            Some(stored.image)
        };
        let (items, status, calculation) = if input.items.is_empty() {
            if image.is_none() {
                return Err(ReturnedPaintError::MissingItems);
            }
            (
                Vec::new(),
                ReturnedPaintStatus::WaitingForBoyoqchiInput,
                None,
            )
        } else {
            let items = normalize_items(input.items)?;
            if !returned_paint_has_minimum_values_per_usage(&items) {
                return Err(ReturnedPaintError::InsufficientValues);
            }
            let calculation = calculate_returned_paint(&items)?;
            (items, ReturnedPaintStatus::Completed, Some(calculation))
        };
        let mut request = ReturnedPaintRequest {
            id,
            order_id,
            order_code: input.order_code.trim().to_string(),
            order_name: input.order_name.trim().to_string(),
            apparatus,
            sender_role: sender.role.clone(),
            sender_ref: sender_ref.to_string(),
            sender_display_name: sender_display_name.to_string(),
            items,
            status,
            image,
            calculation,
            message: String::new(),
            created_at_unix: time::OffsetDateTime::now_utc().unix_timestamp(),
        };
        request.message = completion_report_message(&request);
        Ok(request)
    }

    pub async fn list(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<ReturnedPaintRequest>, ReturnedPaintError> {
        self.store.list(limit.clamp(1, 101), offset).await
    }

    pub async fn complete(
        &self,
        input: ReturnedPaintRequestComplete,
    ) -> Result<ReturnedPaintRequest, ReturnedPaintError> {
        let request_id = input.request_id.trim();
        if request_id.is_empty() {
            return Err(ReturnedPaintError::RequestNotFound);
        }
        let items = normalize_items(input.items)?;
        if !returned_paint_has_minimum_values_per_usage(&items) {
            return Err(ReturnedPaintError::InsufficientValues);
        }
        let calculation = calculate_returned_paint(&items)?;
        self.store.complete(request_id, items, calculation).await
    }

    pub async fn save_image(
        &self,
        order_id: String,
        apparatus: String,
        image_name: String,
        image_mime: String,
        body: Vec<u8>,
        owner: &Principal,
    ) -> Result<ReturnedPaintImage, ReturnedPaintError> {
        let order_id = required_text(order_id, ReturnedPaintError::MissingOrderId)?;
        let apparatus = required_text(apparatus, ReturnedPaintError::MissingApparatus)?;
        let owner_ref = owner.ref_.trim();
        let image_name = image_name.trim();
        let image_mime = image_mime.trim().to_ascii_lowercase();
        if owner_ref.is_empty()
            || image_name.is_empty()
            || body.is_empty()
            || body.len() > 6 * 1024 * 1024
            || !matches!(
                image_mime.as_str(),
                "image/jpeg" | "image/png" | "image/webp" | "image/heic" | "image/heif"
            )
        {
            return Err(ReturnedPaintError::InvalidValue);
        }
        let bytes: [u8; 12] = rand::random();
        let image_id = format!(
            "returned_paint_image_{}",
            data_encoding::HEXLOWER.encode(&bytes)
        );
        let image = ReturnedPaintStoredImage {
            image: ReturnedPaintImage {
                image_url: returned_paint_image_url(&image_id),
                image_id,
                image_name: image_name.to_string(),
                image_mime,
                image_size_bytes: body.len() as u64,
            },
            order_id,
            apparatus,
            owner_ref: owner_ref.to_string(),
            body,
        };
        Ok(self.store.save_image(image).await?.image)
    }

    pub async fn image(
        &self,
        image_id: &str,
    ) -> Result<ReturnedPaintStoredImage, ReturnedPaintError> {
        self.store
            .image(image_id.trim())
            .await?
            .ok_or(ReturnedPaintError::ImageNotFound)
    }

    pub async fn delete_image(
        &self,
        image_id: &str,
        owner: &Principal,
    ) -> Result<(), ReturnedPaintError> {
        let image_id = image_id.trim();
        if image_id.is_empty() {
            return Err(ReturnedPaintError::ImageNotFound);
        }
        if self.store.delete_image(image_id, owner.ref_.trim()).await? {
            Ok(())
        } else {
            Err(ReturnedPaintError::ImageDeleteNotAllowed)
        }
    }
}

include!("returned_paint_calculations.rs");

include!("returned_paint_stores.rs");

include!("returned_paint_inline_tests.rs");
