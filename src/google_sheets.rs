use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core::calculate_orders::CalculateOrderTemplate;
use crate::core::production_map::ProductionMapDefinition;

mod auth;
mod layout;
mod rows;
#[cfg(test)]
mod tests;

use self::auth::{ServiceAccount, ServiceAccountTokenProvider};
use self::layout::{json_insert_header_row, sheet_format_requests, sheet_has_header};
pub use self::rows::is_sheet_order_map;
use self::rows::{missing_order_rows, order_sheet_row, row_code, sheet_codes};

pub(super) const SHEETS_SCOPE: &str = "https://www.googleapis.com/auth/spreadsheets";
pub(super) const DEFAULT_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";
pub(super) const ORDER_SHEET_ID: i64 = 0;
const ORDER_SHEET_HEADER_RANGE: &str = "A1:P1";
pub(super) const ORDER_SHEET_FORMAT_ROW_LIMIT: i64 = 1000;
pub(super) const ORDER_SHEET_HEADERS: [&str; 16] = [
    "pechat",
    "sana",
    "vaqt",
    "kod",
    "zakaz nomi",
    "zakaz kg",
    "1 qavat",
    "2 qavat",
    "3 qavat",
    "material razmer",
    "1 qavat mikron",
    "2 qavat mikron",
    "3 qavat mikron",
    "metr",
    "qolib soni",
    "rezina razmer",
];

#[async_trait]
pub trait OrderSheetSink: Send + Sync {
    fn enabled(&self) -> bool {
        false
    }

    async fn append_order(
        &self,
        map: &ProductionMapDefinition,
        template: &CalculateOrderTemplate,
    ) -> Result<(), OrderSheetError>;

    async fn sync_orders(
        &self,
        maps: &[ProductionMapDefinition],
        templates: &[CalculateOrderTemplate],
    ) -> Result<usize, OrderSheetError> {
        let _ = maps;
        let _ = templates;
        Ok(0)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct NoopOrderSheetSink;

#[async_trait]
impl OrderSheetSink for NoopOrderSheetSink {
    async fn append_order(
        &self,
        _map: &ProductionMapDefinition,
        _template: &CalculateOrderTemplate,
    ) -> Result<(), OrderSheetError> {
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OrderSheetError {
    #[error("order sheet row is not available")]
    NoRow,
    #[error("google sheets auth failed")]
    AuthFailed,
    #[error("google sheets append failed")]
    AppendFailed,
    #[error("google sheets read failed")]
    ReadFailed,
    #[error("google sheets format failed")]
    FormatFailed,
}

pub fn discover_order_sheet_sink() -> Arc<dyn OrderSheetSink> {
    #[cfg(test)]
    {
        return Arc::new(NoopOrderSheetSink);
    }

    #[allow(unreachable_code)]
    {
        let spreadsheet_id = std::env::var("GOOGLE_SHEETS_ORDER_SPREADSHEET_ID")
            .unwrap_or_default()
            .trim()
            .to_string();
        if spreadsheet_id.is_empty() {
            tracing::info!("order sheets disabled: GOOGLE_SHEETS_ORDER_SPREADSHEET_ID missing");
            return Arc::new(NoopOrderSheetSink);
        }
        let Some(path) = discover_service_account_path() else {
            tracing::warn!("order sheets disabled: service account json missing");
            return Arc::new(NoopOrderSheetSink);
        };
        let raw = match std::fs::read(&path) {
            Ok(raw) => raw,
            Err(error) => {
                tracing::warn!(%error, "order sheets disabled: read service account failed");
                return Arc::new(NoopOrderSheetSink);
            }
        };
        let account: ServiceAccount = match serde_json::from_slice(&raw) {
            Ok(account) => account,
            Err(error) => {
                tracing::warn!(%error, "order sheets disabled: parse service account failed");
                return Arc::new(NoopOrderSheetSink);
            }
        };
        let range = std::env::var("GOOGLE_SHEETS_ORDER_RANGE")
            .unwrap_or_else(|_| "A:P".to_string())
            .trim()
            .to_string();
        Arc::new(GoogleSheetsOrderSink::new(account, spreadsheet_id, range))
    }
}

fn discover_service_account_path() -> Option<std::path::PathBuf> {
    for key in [
        "GOOGLE_SHEETS_SERVICE_ACCOUNT_PATH",
        "GOOGLE_SERVICE_ACCOUNT_PATH",
        "FCM_SERVICE_ACCOUNT_PATH",
    ] {
        if let Ok(env) = std::env::var(key) {
            let path = std::path::PathBuf::from(env.trim());
            if !path.as_os_str().is_empty() && path.is_file() {
                return Some(path);
            }
        }
    }
    let fallback = std::path::PathBuf::from("service-account.json");
    fallback.is_file().then_some(fallback)
}

struct GoogleSheetsOrderSink {
    http_client: reqwest::Client,
    token_provider: ServiceAccountTokenProvider,
    append_endpoint: String,
    read_endpoint: String,
    update_header_endpoint: String,
    batch_update_endpoint: String,
}

impl GoogleSheetsOrderSink {
    fn new(account: ServiceAccount, spreadsheet_id: String, range: String) -> Self {
        let encoded_range = urlencoding::encode(range.trim());
        Self {
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .expect("reqwest client"),
            token_provider: ServiceAccountTokenProvider::new(account),
            append_endpoint: format!(
                "https://sheets.googleapis.com/v4/spreadsheets/{spreadsheet_id}/values/{encoded_range}:append?valueInputOption=USER_ENTERED&insertDataOption=INSERT_ROWS"
            ),
            read_endpoint: format!(
                "https://sheets.googleapis.com/v4/spreadsheets/{spreadsheet_id}/values/{encoded_range}"
            ),
            update_header_endpoint: format!(
                "https://sheets.googleapis.com/v4/spreadsheets/{spreadsheet_id}/values/{}?valueInputOption=USER_ENTERED",
                urlencoding::encode(ORDER_SHEET_HEADER_RANGE),
            ),
            batch_update_endpoint: format!(
                "https://sheets.googleapis.com/v4/spreadsheets/{spreadsheet_id}:batchUpdate"
            ),
        }
    }

    async fn read_rows(&self, access_token: &str) -> Result<Vec<Vec<Value>>, OrderSheetError> {
        let response = self
            .http_client
            .get(&self.read_endpoint)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|_| OrderSheetError::ReadFailed)?;
        if !response.status().is_success() {
            return Err(OrderSheetError::ReadFailed);
        }
        let values: SheetValuesResponse = response
            .json()
            .await
            .map_err(|_| OrderSheetError::ReadFailed)?;
        Ok(values.values)
    }

    async fn existing_codes(
        &self,
        access_token: &str,
    ) -> Result<BTreeSet<String>, OrderSheetError> {
        Ok(sheet_codes(self.read_rows(access_token).await?))
    }

    async fn ensure_layout(&self, access_token: &str) -> Result<(), OrderSheetError> {
        let rows = self.read_rows(access_token).await?;
        if !sheet_has_header(&rows) {
            self.insert_header_row(access_token).await?;
        }
        self.write_header(access_token).await?;
        self.apply_format(access_token).await
    }

    async fn insert_header_row(&self, access_token: &str) -> Result<(), OrderSheetError> {
        let payload = BatchUpdateRequest {
            requests: vec![json_insert_header_row()],
        };
        let response = self
            .http_client
            .post(&self.batch_update_endpoint)
            .bearer_auth(access_token)
            .json(&payload)
            .send()
            .await
            .map_err(|_| OrderSheetError::FormatFailed)?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(OrderSheetError::FormatFailed)
        }
    }

    async fn write_header(&self, access_token: &str) -> Result<(), OrderSheetError> {
        let payload = AppendValuesRequest {
            values: vec![
                ORDER_SHEET_HEADERS
                    .into_iter()
                    .map(|value| Value::String(value.to_string()))
                    .collect(),
            ],
        };
        let response = self
            .http_client
            .put(&self.update_header_endpoint)
            .bearer_auth(access_token)
            .json(&payload)
            .send()
            .await
            .map_err(|_| OrderSheetError::FormatFailed)?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(OrderSheetError::FormatFailed)
        }
    }

    async fn apply_format(&self, access_token: &str) -> Result<(), OrderSheetError> {
        let payload = BatchUpdateRequest {
            requests: sheet_format_requests(),
        };
        let response = self
            .http_client
            .post(&self.batch_update_endpoint)
            .bearer_auth(access_token)
            .json(&payload)
            .send()
            .await
            .map_err(|_| OrderSheetError::FormatFailed)?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(OrderSheetError::FormatFailed)
        }
    }

    async fn append_rows(
        &self,
        access_token: &str,
        rows: Vec<Vec<Value>>,
    ) -> Result<(), OrderSheetError> {
        if rows.is_empty() {
            return Ok(());
        }
        let payload = AppendValuesRequest { values: rows };
        let response = self
            .http_client
            .post(&self.append_endpoint)
            .bearer_auth(access_token)
            .json(&payload)
            .send()
            .await
            .map_err(|_| OrderSheetError::AppendFailed)?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(OrderSheetError::AppendFailed)
        }
    }
}

#[async_trait]
impl OrderSheetSink for GoogleSheetsOrderSink {
    fn enabled(&self) -> bool {
        true
    }

    async fn append_order(
        &self,
        map: &ProductionMapDefinition,
        template: &CalculateOrderTemplate,
    ) -> Result<(), OrderSheetError> {
        let row = order_sheet_row(map, template).ok_or(OrderSheetError::NoRow)?;
        let access_token = self.token_provider.access_token(&self.http_client).await?;
        self.ensure_layout(&access_token).await?;
        let existing_codes = self.existing_codes(&access_token).await?;
        let code = row_code(&row);
        if existing_codes.contains(&code) {
            return Ok(());
        }
        self.append_rows(&access_token, vec![row]).await
    }

    async fn sync_orders(
        &self,
        maps: &[ProductionMapDefinition],
        templates: &[CalculateOrderTemplate],
    ) -> Result<usize, OrderSheetError> {
        let access_token = self.token_provider.access_token(&self.http_client).await?;
        self.ensure_layout(&access_token).await?;
        let existing_codes = self.existing_codes(&access_token).await?;
        let rows = missing_order_rows(maps, templates, &existing_codes);
        let count = rows.len();
        self.append_rows(&access_token, rows).await?;
        Ok(count)
    }
}

#[derive(Serialize)]
struct AppendValuesRequest {
    values: Vec<Vec<Value>>,
}

#[derive(Serialize)]
struct BatchUpdateRequest {
    requests: Vec<Value>,
}

#[derive(Deserialize)]
struct SheetValuesResponse {
    #[serde(default)]
    values: Vec<Vec<Value>>,
}
