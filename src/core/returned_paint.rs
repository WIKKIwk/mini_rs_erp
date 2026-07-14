use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::core::auth::models::{Principal, PrincipalRole};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReturnedPaintItem {
    pub usage: String,
    pub category: String,
    pub name: String,
    pub values: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReturnedPaintRequestCreate {
    pub order_id: String,
    #[serde(default)]
    pub order_code: String,
    #[serde(default)]
    pub order_name: String,
    pub apparatus: String,
    pub items: Vec<ReturnedPaintItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
        let request = self.prepare_request(
            input,
            sender,
            format!(
                "returned_paint_{}",
                data_encoding::HEXLOWER.encode(&bytes)
            ),
        )?;
        self.store
            .create(request)
            .await
    }

    pub fn prepare_request(
        &self,
        input: ReturnedPaintRequestCreate,
        sender: &Principal,
        id: String,
    ) -> Result<ReturnedPaintRequest, ReturnedPaintError> {
        let order_id = required_text(input.order_id, ReturnedPaintError::MissingOrderId)?;
        let apparatus = required_text(input.apparatus, ReturnedPaintError::MissingApparatus)?;
        let items = normalize_items(input.items)?;
        let sender_ref = sender.ref_.trim();
        let sender_display_name = sender.display_name.trim();
        if sender_ref.is_empty() || sender_display_name.is_empty() || id.trim().is_empty() {
            return Err(ReturnedPaintError::StoreFailed);
        }
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
}

pub fn completion_report_message(request: &ReturnedPaintRequest) -> String {
    let order_label = [request.order_code.trim(), request.order_name.trim()]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    let order_label = if order_label.is_empty() {
        request.order_id.trim().to_string()
    } else {
        format!("{} ({})", order_label, request.order_id.trim())
    };
    format!(
        "Operator {} {} orderini {} apparatida muvaffaqiyatli yopdi. Rasxot bo‘yoq sarfi va Astatka qolgan bo‘yoq miqdorlari, berilgan lak va erituvchi qiymatlari qayd etildi.",
        request.sender_display_name.trim(),
        order_label,
        request.apparatus.trim(),
    )
}

fn required_text(
    value: String,
    error: ReturnedPaintError,
) -> Result<String, ReturnedPaintError> {
    let value = value.trim();
    if value.is_empty() {
        Err(error)
    } else {
        Ok(value.to_string())
    }
}

fn normalize_items(
    items: Vec<ReturnedPaintItem>,
) -> Result<Vec<ReturnedPaintItem>, ReturnedPaintError> {
    if items.is_empty() {
        return Err(ReturnedPaintError::MissingItems);
    }
    items
        .into_iter()
        .map(|item| {
            let usage = match item.usage.trim().to_ascii_lowercase().as_str() {
                "rasxot" => "rasxot",
                "astatka" => "astatka",
                _ => return Err(ReturnedPaintError::InvalidUsage),
            };
            let category = match item.category.trim().to_ascii_lowercase().as_str() {
                "colors" => "colors",
                "lacquers" => "lacquers",
                "solvents" => "solvents",
                _ => return Err(ReturnedPaintError::InvalidCategory),
            };
            let name = item.name.trim();
            if name.is_empty() {
                return Err(ReturnedPaintError::MissingItemName);
            }
            let values = item
                .values
                .into_iter()
                .map(|(label, value)| {
                    let label = label.trim();
                    if label.is_empty() || !value.is_finite() || value < 0.0 {
                        return Err(ReturnedPaintError::InvalidValue);
                    }
                    Ok((label.to_string(), value))
                })
                .collect::<Result<BTreeMap<_, _>, _>>()?;
            if values.is_empty() {
                return Err(ReturnedPaintError::MissingValues);
            }
            Ok(ReturnedPaintItem {
                usage: usage.to_string(),
                category: category.to_string(),
                name: name.to_string(),
                values,
            })
        })
        .collect()
}

struct UnavailableReturnedPaintStore;

#[async_trait]
impl ReturnedPaintStorePort for UnavailableReturnedPaintStore {
    async fn create(
        &self,
        _request: ReturnedPaintRequest,
    ) -> Result<ReturnedPaintRequest, ReturnedPaintError> {
        Err(ReturnedPaintError::StoreFailed)
    }

    async fn list(
        &self,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<ReturnedPaintRequest>, ReturnedPaintError> {
        Err(ReturnedPaintError::StoreFailed)
    }
}

#[derive(Default)]
pub struct MemoryReturnedPaintStore {
    requests: RwLock<Vec<ReturnedPaintRequest>>,
}

impl MemoryReturnedPaintStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ReturnedPaintStorePort for MemoryReturnedPaintStore {
    async fn create(
        &self,
        request: ReturnedPaintRequest,
    ) -> Result<ReturnedPaintRequest, ReturnedPaintError> {
        self.requests.write().await.push(request.clone());
        Ok(request)
    }

    async fn list(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<ReturnedPaintRequest>, ReturnedPaintError> {
        let mut requests = self.requests.read().await.clone();
        requests.sort_by(|left, right| {
            right
                .created_at_unix
                .cmp(&left.created_at_unix)
                .then_with(|| right.id.cmp(&left.id))
        });
        Ok(requests.into_iter().skip(offset).take(limit).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rasxot_and_astatka_stay_separate() {
        let service = ReturnedPaintService::new(Arc::new(MemoryReturnedPaintStore::new()));
        let sender = Principal {
            role: PrincipalRole::Aparatchi,
            display_name: "Bosmachi".to_string(),
            legal_name: "Bosmachi".to_string(),
            ref_: "worker-1".to_string(),
            phone: String::new(),
            avatar_url: String::new(),
        };
        let request = service
            .create(
                ReturnedPaintRequestCreate {
                    order_id: "order-1".to_string(),
                    order_code: "1212".to_string(),
                    order_name: "Mahsulot".to_string(),
                    apparatus: "7 ta rangli bosma".to_string(),
                    items: vec![
                        ReturnedPaintItem {
                            usage: "rasxot".to_string(),
                            category: "colors".to_string(),
                            name: "Oq".to_string(),
                            values: BTreeMap::from([("Mix".to_string(), 3.0)]),
                        },
                        ReturnedPaintItem {
                            usage: "astatka".to_string(),
                            category: "colors".to_string(),
                            name: "Oq".to_string(),
                            values: BTreeMap::from([("Mix".to_string(), 1.0)]),
                        },
                    ],
                },
                &sender,
            )
            .await
            .expect("create request");

        assert_eq!(request.items[0].usage, "rasxot");
        assert_eq!(request.items[0].values["Mix"], 3.0);
        assert_eq!(request.items[1].usage, "astatka");
        assert_eq!(request.items[1].values["Mix"], 1.0);
    }
}
