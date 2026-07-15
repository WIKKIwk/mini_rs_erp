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

fn deserialize_decimal_values<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, String>, D::Error>
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
        let request = self.prepare_request(
            input,
            sender,
            format!(
                "returned_paint_{}",
                data_encoding::HEXLOWER.encode(&bytes)
            ),
        )
        .await?;
        self.store
            .create(request)
            .await
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
            (
                items,
                ReturnedPaintStatus::Completed,
                Some(calculation),
            )
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

pub fn returned_paint_image_url(image_id: &str) -> String {
    format!(
        "/v1/mobile/returned-paint/images/view?id={}",
        image_id.trim()
    )
}

pub fn returned_paint_value_count(items: &[ReturnedPaintItem]) -> usize {
    items.iter().map(|item| item.values.len()).sum()
}

pub fn returned_paint_value_count_for_usage(
    items: &[ReturnedPaintItem],
    usage: &str,
) -> usize {
    items
        .iter()
        .filter(|item| item.usage.trim().eq_ignore_ascii_case(usage.trim()))
        .map(|item| item.values.len())
        .sum()
}

pub fn returned_paint_has_minimum_values_per_usage(
    items: &[ReturnedPaintItem],
) -> bool {
    returned_paint_value_count_for_usage(items, "rasxot") >= 3
        && returned_paint_value_count_for_usage(items, "astatka") >= 3
}

pub fn returned_paint_report_can_close(
    items: &[ReturnedPaintItem],
    has_image: bool,
) -> bool {
    returned_paint_has_minimum_values_per_usage(items)
        || (returned_paint_value_count(items) == 0 && has_image)
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
    match request.status {
        ReturnedPaintStatus::WaitingForBoyoqchiInput => format!(
            "Operator {} {} orderini {} apparatida rasm bilan yopdi. Qaytarilgan bo‘yoq qiymatlari Bo‘yoqchi tomonidan kiritilishi kutilmoqda.",
            request.sender_display_name.trim(),
            order_label,
            request.apparatus.trim(),
        ),
        ReturnedPaintStatus::Completed => format!(
            "Operator {} {} orderini {} apparatida muvaffaqiyatli yopdi. Rasxot bo‘yoq sarfi va Astatka qolgan bo‘yoq miqdorlari, berilgan lak va erituvchi qiymatlari qayd etildi.",
            request.sender_display_name.trim(),
            order_label,
            request.apparatus.trim(),
        ),
    }
}

pub fn returned_paint_astatka_total(
    items: &[ReturnedPaintItem],
) -> Result<f64, ReturnedPaintError> {
    let total = items
        .iter()
        .filter(|item| item.usage.trim().eq_ignore_ascii_case("astatka"))
        .flat_map(|item| item.values.values())
        .try_fold(DecimalAmount::ZERO, |total, value| {
            checked_add(total, DecimalAmount::parse_input(value)?)
        })?;
    total.to_f64()
}

pub fn calculate_returned_paint(
    items: &[ReturnedPaintItem],
) -> Result<ReturnedPaintCalculation, ReturnedPaintError> {
    let mut rasxot = PaintUsageTotals::default();
    let mut astatka = PaintUsageTotals::default();

    for item in items.iter().filter(|item| {
        item.category.trim().eq_ignore_ascii_case("colors")
            || item.category.trim().eq_ignore_ascii_case("solvents")
    }) {
        let totals = if item.usage.trim().eq_ignore_ascii_case("rasxot") {
            &mut rasxot
        } else if item.usage.trim().eq_ignore_ascii_case("astatka") {
            &mut astatka
        } else {
            return Err(ReturnedPaintError::InvalidUsage);
        };
        for (label, value) in &item.values {
            let value = DecimalAmount::parse_input(value)?;
            if item.category.trim().eq_ignore_ascii_case("solvents") {
                totals.direct_alcohol = checked_add(totals.direct_alcohol, value)?;
            } else if item.name.trim().eq_ignore_ascii_case("mix")
                || label.trim().eq_ignore_ascii_case("mix")
            {
                totals.mix = checked_add(totals.mix, value)?;
            } else {
                totals.direct_paint = checked_add(totals.direct_paint, value)?;
            }
        }
    }

    let rasxot_mix_paint = rasxot.mix.checked_percent(70)?;
    let astatka_mix_paint = astatka.mix.checked_percent(70)?;
    let rasxot_alcohol = checked_add(
        rasxot.direct_alcohol,
        rasxot.mix.checked_percent(30)?,
    )?;
    let astatka_alcohol = checked_add(
        astatka.direct_alcohol,
        astatka.mix.checked_percent(30)?,
    )?;
    let rasxot_pure_paint = checked_add(rasxot.direct_paint, rasxot_mix_paint)?;
    let astatka_pure_paint = checked_add(astatka.direct_paint, astatka_mix_paint)?;
    let final_used_alcohol =
        checked_non_negative_sub(rasxot_alcohol, astatka_alcohol)?;
    let final_used_paint = checked_non_negative_sub(rasxot_pure_paint, astatka_pure_paint)?;

    for value in [
        rasxot.mix,
        astatka.mix,
        rasxot_alcohol,
        astatka_alcohol,
        final_used_alcohol,
        rasxot_pure_paint,
        astatka_pure_paint,
        final_used_paint,
    ] {
        validate_storable_decimal(value)?;
    }
    Ok(ReturnedPaintCalculation {
        rasxot_mix_total: rasxot.mix.to_string(),
        astatka_mix_total: astatka.mix.to_string(),
        rasxot_alcohol: rasxot_alcohol.to_string(),
        astatka_alcohol: astatka_alcohol.to_string(),
        final_used_alcohol: final_used_alcohol.to_string(),
        rasxot_pure_paint: rasxot_pure_paint.to_string(),
        astatka_pure_paint: astatka_pure_paint.to_string(),
        final_used_paint: final_used_paint.to_string(),
    })
}

#[derive(Default)]
struct PaintUsageTotals {
    mix: DecimalAmount,
    direct_paint: DecimalAmount,
    direct_alcohol: DecimalAmount,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
struct DecimalAmount(i128);

impl DecimalAmount {
    const SCALE_DIGITS: usize = 12;
    const SCALE_FACTOR: i128 = 1_000_000_000_000;
    const MAX_STORED_UNITS: i128 =
        999_999_999_999_999_999 * Self::SCALE_FACTOR;
    const ZERO: Self = Self(0);

    fn parse_input(value: &str) -> Result<Self, ReturnedPaintError> {
        Self::parse(value, 11)
    }

    fn parse_stored(value: &str) -> Result<Self, ReturnedPaintError> {
        Self::parse(value, Self::SCALE_DIGITS)
    }

    fn parse(
        value: &str,
        max_fraction_digits: usize,
    ) -> Result<Self, ReturnedPaintError> {
        const MAX_SIGNIFICAND_DIGITS: usize = 64;

        let value = value.trim();
        if value.is_empty() || matches!(value.as_bytes().first(), Some(b'-' | b'+')) {
            return Err(ReturnedPaintError::InvalidValue);
        }
        let mut scientific_parts = value.split(|character| matches!(character, 'e' | 'E'));
        let mantissa = scientific_parts.next().unwrap_or_default();
        let exponent = match scientific_parts.next() {
            Some(value) => value
                .parse::<i32>()
                .map_err(|_| ReturnedPaintError::InvalidValue)?,
            None => 0,
        };
        if scientific_parts.next().is_some() {
            return Err(ReturnedPaintError::InvalidValue);
        }
        let mut parts = mantissa.split('.');
        let integer = parts.next().unwrap_or_default();
        let fraction = parts.next().unwrap_or_default();
        if (integer.is_empty() && fraction.is_empty())
            || parts.next().is_some()
            || (!integer.is_empty() && !integer.bytes().all(|byte| byte.is_ascii_digit()))
            || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(ReturnedPaintError::InvalidValue);
        }
        if integer.len() + fraction.len() > MAX_SIGNIFICAND_DIGITS {
            return Err(ReturnedPaintError::InvalidValue);
        }
        let mut digits = format!("{integer}{fraction}");
        let mut fraction_digits = i64::try_from(fraction.len())
            .ok()
            .and_then(|length| length.checked_sub(i64::from(exponent)))
            .ok_or(ReturnedPaintError::InvalidValue)?;
        while fraction_digits > 0 && digits.ends_with('0') {
            digits.pop();
            fraction_digits -= 1;
        }
        let digits = digits.trim_start_matches('0');
        if digits.is_empty() {
            return Ok(Self::ZERO);
        }
        if fraction_digits > max_fraction_digits as i64 {
            return Err(ReturnedPaintError::InvalidValue);
        }
        let significand = digits
            .parse::<i128>()
            .map_err(|_| ReturnedPaintError::InvalidValue)?;
        let units = if fraction_digits >= 0 {
            let scale_power = i64::try_from(Self::SCALE_DIGITS)
                .ok()
                .and_then(|scale| scale.checked_sub(fraction_digits))
                .and_then(|power| u32::try_from(power).ok())
                .ok_or(ReturnedPaintError::InvalidValue)?;
            significand
                .checked_mul(
                    10_i128
                        .checked_pow(scale_power)
                        .ok_or(ReturnedPaintError::InvalidValue)?,
                )
                .ok_or(ReturnedPaintError::InvalidValue)?
        } else {
            let integer_power = fraction_digits
                .checked_neg()
                .and_then(|power| u32::try_from(power).ok())
                .ok_or(ReturnedPaintError::InvalidValue)?;
            significand
                .checked_mul(
                    10_i128
                        .checked_pow(integer_power)
                        .ok_or(ReturnedPaintError::InvalidValue)?,
                )
                .and_then(|value| value.checked_mul(Self::SCALE_FACTOR))
                .ok_or(ReturnedPaintError::InvalidValue)?
        };
        let amount = Self(units);
        validate_storable_decimal(amount)?;
        Ok(amount)
    }

    fn checked_percent(self, percent: i128) -> Result<Self, ReturnedPaintError> {
        let multiplied = self
            .0
            .checked_mul(percent)
            .ok_or(ReturnedPaintError::InvalidValue)?;
        if multiplied % 100 != 0 {
            return Err(ReturnedPaintError::InvalidValue);
        }
        Ok(Self(multiplied / 100))
    }

    fn to_f64(self) -> Result<f64, ReturnedPaintError> {
        let value = self
            .to_string()
            .parse::<f64>()
            .map_err(|_| ReturnedPaintError::InvalidValue)?;
        if value.is_finite() {
            Ok(value)
        } else {
            Err(ReturnedPaintError::InvalidValue)
        }
    }
}

impl fmt::Display for DecimalAmount {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let integer = self.0 / Self::SCALE_FACTOR;
        let fraction = self.0 % Self::SCALE_FACTOR;
        if fraction == 0 {
            return write!(formatter, "{integer}");
        }
        let fraction = format!("{fraction:0width$}", width = Self::SCALE_DIGITS)
            .trim_end_matches('0')
            .to_string();
        write!(formatter, "{integer}.{fraction}")
    }
}

fn checked_add(
    left: DecimalAmount,
    right: DecimalAmount,
) -> Result<DecimalAmount, ReturnedPaintError> {
    left.0
        .checked_add(right.0)
        .map(DecimalAmount)
        .ok_or(ReturnedPaintError::InvalidValue)
}

fn checked_non_negative_sub(
    left: DecimalAmount,
    right: DecimalAmount,
) -> Result<DecimalAmount, ReturnedPaintError> {
    if left < right {
        Err(ReturnedPaintError::NegativeFinalValue)
    } else {
        left.0
            .checked_sub(right.0)
            .map(DecimalAmount)
            .ok_or(ReturnedPaintError::InvalidValue)
    }
}

fn validate_storable_decimal(value: DecimalAmount) -> Result<(), ReturnedPaintError> {
    if value < DecimalAmount::ZERO || value.0 > DecimalAmount::MAX_STORED_UNITS {
        Err(ReturnedPaintError::InvalidValue)
    } else {
        Ok(())
    }
}

pub(crate) fn normalize_returned_paint_stored_decimal(
    value: &str,
) -> Result<String, ReturnedPaintError> {
    Ok(DecimalAmount::parse_stored(value)?.to_string())
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
                    if label.is_empty() {
                        return Err(ReturnedPaintError::InvalidValue);
                    }
                    let value = DecimalAmount::parse_input(&value)?;
                    Ok((label.to_string(), value.to_string()))
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

    async fn complete(
        &self,
        _request_id: &str,
        _items: Vec<ReturnedPaintItem>,
        _calculation: ReturnedPaintCalculation,
    ) -> Result<ReturnedPaintRequest, ReturnedPaintError> {
        Err(ReturnedPaintError::StoreFailed)
    }

    async fn save_image(
        &self,
        _image: ReturnedPaintStoredImage,
    ) -> Result<ReturnedPaintStoredImage, ReturnedPaintError> {
        Err(ReturnedPaintError::StoreFailed)
    }

    async fn image(
        &self,
        _image_id: &str,
    ) -> Result<Option<ReturnedPaintStoredImage>, ReturnedPaintError> {
        Err(ReturnedPaintError::StoreFailed)
    }

    async fn delete_image(
        &self,
        _image_id: &str,
        _owner_ref: &str,
    ) -> Result<bool, ReturnedPaintError> {
        Err(ReturnedPaintError::StoreFailed)
    }
}

#[derive(Default)]
pub struct MemoryReturnedPaintStore {
    requests: RwLock<Vec<ReturnedPaintRequest>>,
    images: RwLock<BTreeMap<String, ReturnedPaintStoredImage>>,
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
        let mut requests = self.requests.write().await;
        if let Some(existing) = requests.iter().find(|existing| existing.id == request.id) {
            return Ok(existing.clone());
        }
        requests.push(request.clone());
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

    async fn complete(
        &self,
        request_id: &str,
        items: Vec<ReturnedPaintItem>,
        calculation: ReturnedPaintCalculation,
    ) -> Result<ReturnedPaintRequest, ReturnedPaintError> {
        let mut requests = self.requests.write().await;
        let request = requests
            .iter_mut()
            .find(|request| request.id == request_id)
            .ok_or(ReturnedPaintError::RequestNotFound)?;
        if request.status == ReturnedPaintStatus::Completed {
            return Ok(request.clone());
        }
        request.items = items;
        request.calculation = Some(calculation);
        request.status = ReturnedPaintStatus::Completed;
        request.message = completion_report_message(request);
        Ok(request.clone())
    }

    async fn save_image(
        &self,
        image: ReturnedPaintStoredImage,
    ) -> Result<ReturnedPaintStoredImage, ReturnedPaintError> {
        self.images
            .write()
            .await
            .insert(image.image.image_id.clone(), image.clone());
        Ok(image)
    }

    async fn image(
        &self,
        image_id: &str,
    ) -> Result<Option<ReturnedPaintStoredImage>, ReturnedPaintError> {
        Ok(self.images.read().await.get(image_id).cloned())
    }

    async fn delete_image(
        &self,
        image_id: &str,
        owner_ref: &str,
    ) -> Result<bool, ReturnedPaintError> {
        if self
            .requests
            .read()
            .await
            .iter()
            .any(|request| request.image.as_ref().is_some_and(|image| image.image_id == image_id))
        {
            return Ok(false);
        }
        let mut images = self.images.write().await;
        if images
            .get(image_id)
            .map_or(true, |image| image.owner_ref != owner_ref)
        {
            return Ok(false);
        }
        Ok(images.remove(image_id).is_some())
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
                    image_id: String::new(),
                    items: vec![
                        ReturnedPaintItem {
                            usage: "rasxot".to_string(),
                            category: "colors".to_string(),
                            name: "Oq".to_string(),
                            values: BTreeMap::from([
                                ("Mix".to_string(), "3".to_string()),
                                ("Oq".to_string(), "1".to_string()),
                                ("Qora".to_string(), "0".to_string()),
                            ]),
                        },
                        ReturnedPaintItem {
                            usage: "astatka".to_string(),
                            category: "colors".to_string(),
                            name: "Oq".to_string(),
                            values: BTreeMap::from([
                                ("Mix".to_string(), "1".to_string()),
                                ("Oq".to_string(), "0".to_string()),
                                ("Qora".to_string(), "0".to_string()),
                            ]),
                        },
                    ],
                },
                &sender,
            )
            .await
            .expect("create request");

        assert_eq!(request.items[0].usage, "rasxot");
        assert_eq!(request.items[0].values["Mix"], "3");
        assert_eq!(request.items[1].usage, "astatka");
        assert_eq!(request.items[1].values["Mix"], "1");
        assert_eq!(returned_paint_astatka_total(&request.items), Ok(1.0));
    }

    #[test]
    fn minimum_returned_paint_fields_are_checked_per_usage() {
        let rasxot_only = vec![item(
            "rasxot",
            "colors",
            [("Mix", "1"), ("Oq", "1"), ("Qora", "0")],
        )];
        let both_usages = vec![
            item(
                "rasxot",
                "colors",
                [("Mix", "1"), ("Oq", "1"), ("Qora", "0")],
            ),
            item(
                "astatka",
                "colors",
                [("Mix", "0.5"), ("Oq", "0"), ("Qora", "0")],
            ),
        ];

        assert_eq!(
            returned_paint_value_count_for_usage(&rasxot_only, "rasxot"),
            3
        );
        assert_eq!(
            returned_paint_value_count_for_usage(&rasxot_only, "astatka"),
            0
        );
        assert!(!returned_paint_report_can_close(&rasxot_only, false));
        assert!(returned_paint_report_can_close(&both_usages, false));
        assert!(returned_paint_report_can_close(&[], true));
    }

    #[test]
    fn calculates_color_mix_and_all_solvent_fields_as_pure_alcohol() {
        let items = vec![
            item(
                "rasxot",
                "colors",
                [("Mix", "10"), ("Oq", "2"), ("Spirt", "1")],
            ),
            item("rasxot", "colors", [("Mix", "2.5"), ("Qora", "0.5")]),
            item("astatka", "colors", [("Mix", "4"), ("Oq", "1")]),
            item("astatka", "colors", [("Mix", "1"), ("Qora", "0.25")]),
            item("rasxot", "lacquers", [("OPV lak", "100")]),
            item(
                "rasxot",
                "solvents",
                [("Etil", "10"), ("Metoxil", "2"), ("Rasvavitel", "0.5")],
            ),
            item(
                "astatka",
                "solvents",
                [("Etil", "1"), ("Aralashmalar", "0.25")],
            ),
        ];

        let result = calculate_returned_paint(&items).expect("calculation");

        assert_eq!(result.rasxot_mix_total, "12.5");
        assert_eq!(result.astatka_mix_total, "5");
        assert_eq!(result.rasxot_alcohol, "16.25");
        assert_eq!(result.astatka_alcohol, "2.75");
        assert_eq!(result.final_used_alcohol, "13.5");
        assert_eq!(result.rasxot_pure_paint, "12.25");
        assert_eq!(result.astatka_pure_paint, "4.75");
        assert_eq!(result.final_used_paint, "7.5");
    }

    #[test]
    fn treats_named_values_inside_mix_item_as_mix() {
        let items = vec![
            item(
                "rasxot",
                "colors",
                [("Batch A", "10"), ("Blue recipe", "2")],
            ),
            item("astatka", "colors", [("Batch A", "4")]),
        ];

        let result = calculate_returned_paint(
            &items
                .into_iter()
                .map(|mut value| {
                    value.name = "Mix".to_string();
                    value
                })
                .collect::<Vec<_>>(),
        )
        .expect("named mix calculation");

        assert_eq!(result.rasxot_mix_total, "12");
        assert_eq!(result.astatka_mix_total, "4");
        assert_eq!(result.rasxot_alcohol, "3.6");
        assert_eq!(result.astatka_alcohol, "1.2");
        assert_eq!(result.final_used_paint, "5.6");
    }

    #[test]
    fn rejects_astatka_that_would_make_a_final_value_negative() {
        let alcohol_negative = vec![
            item("rasxot", "colors", [("Mix", "1")]),
            item("astatka", "colors", [("Mix", "2")]),
        ];
        let solvent_negative = vec![
            item("rasxot", "solvents", [("Etil", "1")]),
            item("astatka", "solvents", [("Metoxil", "2")]),
        ];
        let paint_negative = vec![
            item("rasxot", "colors", [("Mix", "1")]),
            item("astatka", "colors", [("Oq", "1")]),
        ];

        assert_eq!(
            calculate_returned_paint(&alcohol_negative),
            Err(ReturnedPaintError::NegativeFinalValue)
        );
        assert_eq!(
            calculate_returned_paint(&solvent_negative),
            Err(ReturnedPaintError::NegativeFinalValue)
        );
        assert_eq!(
            calculate_returned_paint(&paint_negative),
            Err(ReturnedPaintError::NegativeFinalValue)
        );
    }

    #[test]
    fn keeps_eleven_digit_input_precision_without_floating_point_rounding() {
        let items = serde_json::from_str::<Vec<ReturnedPaintItem>>(
            r#"[
                {"usage":"rasxot","category":"colors","name":"Oq","values":{"Mix":3e-11}},
                {"usage":"astatka","category":"colors","name":"Oq","values":{"Mix":1e-11}}
            ]"#,
        )
        .expect("decimal JSON");
        let items = normalize_items(items).expect("normalized items");

        let result = calculate_returned_paint(&items).expect("calculation");

        assert_eq!(result.rasxot_alcohol, "0.000000000009");
        assert_eq!(result.astatka_alcohol, "0.000000000003");
        assert_eq!(result.final_used_alcohol, "0.000000000006");
        assert_eq!(result.final_used_paint, "0.000000000014");
    }

    fn item<const N: usize>(
        usage: &str,
        category: &str,
        values: [(&str, &str); N],
    ) -> ReturnedPaintItem {
        ReturnedPaintItem {
            usage: usage.to_string(),
            category: category.to_string(),
            name: "card".to_string(),
            values: values
                .into_iter()
                .map(|(label, value)| (label.to_string(), value.to_string()))
                .collect(),
        }
    }
}
