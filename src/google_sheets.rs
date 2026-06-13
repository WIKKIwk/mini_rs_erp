use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::core::calculate_orders::CalculateOrderTemplate;
use crate::core::formula::{CalculateRequest, LayerInput, calculate};
use crate::core::production_map::{ProductionMapDefinition, ProductionMapNodeKind};

const SHEETS_SCOPE: &str = "https://www.googleapis.com/auth/spreadsheets";
const DEFAULT_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";
const ORDER_SHEET_ID: i64 = 0;
const ORDER_SHEET_HEADER_RANGE: &str = "A1:P1";
const ORDER_SHEET_FORMAT_ROW_LIMIT: i64 = 1000;
const ORDER_SHEET_HEADERS: [&str; 16] = [
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

pub fn is_sheet_order_map(map: &ProductionMapDefinition) -> bool {
    let id = map.id.trim();
    let order_number = map.order_number.trim();
    id.starts_with("zakaz-")
        && order_number.len() == 4
        && order_number.chars().all(|ch| ch.is_ascii_digit())
}

fn order_sheet_row(
    map: &ProductionMapDefinition,
    template: &CalculateOrderTemplate,
) -> Option<Vec<Value>> {
    if !is_sheet_order_map(map) || template.kg <= 0.0 {
        return None;
    }
    let calculation = calculate(CalculateRequest {
        order_number: Some(map.order_number.trim().to_string()),
        product: Some(template.product.trim().to_string()),
        kg: Some(template.kg),
        width_mm: Some(template.width_mm),
        waste_percent: Some(template.waste_percent),
        roll_count: template.roll_count,
        first_layer: LayerInput::new(
            template.first_layer_material.trim(),
            template.first_layer_micron.trim(),
        ),
        second_layer: LayerInput::new(
            template.second_layer_material.trim(),
            template.second_layer_micron.trim(),
        ),
        third_layer: LayerInput::new(
            template.third_layer_material.trim(),
            template.third_layer_micron.trim(),
        ),
        ..CalculateRequest::default()
    })
    .ok()?;
    let first_result = calculation.results.first()?;
    let now = time::OffsetDateTime::now_utc()
        .to_offset(time::UtcOffset::from_hms(5, 0, 0).expect("valid tashkent offset"));
    Some(vec![
        Value::String(sheet_press_marker(map, template)),
        Value::String(format!("{:02}/{:02}", now.day(), u8::from(now.month()))),
        Value::String(format!("{:02}:{:02}", now.hour(), now.minute())),
        Value::String(sheet_order_code(map, template)),
        Value::String(first_non_empty([
            &template.product,
            &template.name,
            &map.title,
        ])),
        json_number(template.kg),
        Value::String(template.first_layer_material.trim().to_string()),
        Value::String(template.second_layer_material.trim().to_string()),
        Value::String(template.third_layer_material.trim().to_string()),
        json_number(template.width_mm),
        Value::String(template.first_layer_micron.trim().to_string()),
        Value::String(template.second_layer_micron.trim().to_string()),
        Value::String(template.third_layer_micron.trim().to_string()),
        json_number(first_result.rounded_length),
        template
            .roll_count
            .map(json_number)
            .unwrap_or_else(|| Value::String(String::new())),
        json_number(f64::from(calculation.rubber_size_mm)),
    ])
}

pub fn missing_order_rows(
    maps: &[ProductionMapDefinition],
    templates: &[CalculateOrderTemplate],
    existing_codes: &BTreeSet<String>,
) -> Vec<Vec<Value>> {
    let mut seen = existing_codes.clone();
    let mut rows = Vec::new();
    for map in maps {
        if !is_sheet_order_map(map) {
            continue;
        }
        let Some(template) = templates.iter().find(|template| {
            template.source_map_id.trim() == map.id.trim()
                || template.order_number.trim() == map.order_number.trim()
                || template.code.trim() == map.code.trim()
        }) else {
            continue;
        };
        let Some(row) = order_sheet_row(map, template) else {
            continue;
        };
        let code = row_code(&row);
        if seen.insert(code) {
            rows.push(row);
        }
    }
    rows
}

fn sheet_codes(values: Vec<Vec<Value>>) -> BTreeSet<String> {
    values
        .into_iter()
        .filter_map(|row| row.get(3).and_then(value_text).map(normalize_sheet_code))
        .filter(|code| !code.is_empty())
        .collect()
}

fn sheet_has_header(rows: &[Vec<Value>]) -> bool {
    let Some(row) = rows.first() else {
        return false;
    };
    row.first().and_then(value_text) == Some(ORDER_SHEET_HEADERS[0])
        && row.get(1).and_then(value_text) == Some(ORDER_SHEET_HEADERS[1])
        && row.get(3).and_then(value_text) == Some(ORDER_SHEET_HEADERS[3])
}

fn row_code(row: &[Value]) -> String {
    row.get(3)
        .and_then(value_text)
        .map(normalize_sheet_code)
        .unwrap_or_default()
}

fn value_text(value: &Value) -> Option<&str> {
    match value {
        Value::String(value) => Some(value.as_str()),
        _ => None,
    }
}

fn normalize_sheet_code(value: &str) -> String {
    value.trim().trim_start_matches('/').trim().to_string()
}

fn sheet_press_marker(map: &ProductionMapDefinition, template: &CalculateOrderTemplate) -> String {
    let product = format!("{} {}", template.product, template.name).to_lowercase();
    if product.contains("flex") || product.contains("fleks") || product.contains("flekso") {
        return "F".to_string();
    }
    let mut titles = Vec::new();
    for node in &map.nodes {
        if node.kind != ProductionMapNodeKind::Apparatus {
            continue;
        }
        let assigned = node.alternative_assigned_title.trim();
        if !assigned.is_empty() {
            titles.insert(0, assigned.to_string());
        }
        titles.push(node.title.trim().to_string());
    }
    for title in titles {
        let lower = title.to_lowercase();
        if lower.contains("flex") || lower.contains("fleks") || lower.contains("flekso") {
            return "F".to_string();
        }
        for marker in ["9", "8", "7"] {
            if lower.contains(&format!("{marker} ta rangli"))
                || lower.contains(&format!("{marker} rangli"))
            {
                return marker.to_string();
            }
        }
    }
    String::new()
}

fn sheet_order_code(map: &ProductionMapDefinition, template: &CalculateOrderTemplate) -> String {
    let code = first_non_empty([
        map.order_number.as_str(),
        map.code.as_str(),
        template.order_number.as_str(),
        template.code.as_str(),
    ]);
    if code.starts_with('/') {
        code
    } else {
        format!("/{code}")
    }
}

fn first_non_empty<const N: usize>(values: [&str; N]) -> String {
    values
        .into_iter()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .unwrap_or("")
        .to_string()
}

fn json_number(value: f64) -> Value {
    serde_json::Number::from_f64(value)
        .map(Value::Number)
        .unwrap_or_else(|| Value::String(String::new()))
}

#[derive(Debug, Clone, Deserialize)]
struct ServiceAccount {
    client_email: String,
    private_key: String,
    #[serde(default)]
    token_uri: String,
}

#[derive(Debug)]
struct ServiceAccountTokenProvider {
    account: ServiceAccount,
    cache: Mutex<Option<CachedAccessToken>>,
}

impl ServiceAccountTokenProvider {
    fn new(account: ServiceAccount) -> Self {
        Self {
            account,
            cache: Mutex::new(None),
        }
    }

    async fn access_token(&self, client: &reqwest::Client) -> Result<String, OrderSheetError> {
        let mut cache = self.cache.lock().await;
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        if let Some(cached) = cache.as_ref()
            && cached.expires_at > now + 60
        {
            return Ok(cached.access_token.clone());
        }

        let token_uri = self.token_uri();
        let assertion = self.signed_assertion(now, &token_uri)?;
        let form = format!(
            "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer&assertion={}",
            urlencoding::encode(&assertion)
        );
        let response = client
            .post(&token_uri)
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(form)
            .send()
            .await
            .map_err(|_| OrderSheetError::AuthFailed)?;
        if !response.status().is_success() {
            return Err(OrderSheetError::AuthFailed);
        }
        let token: OAuthTokenResponse = response
            .json()
            .await
            .map_err(|_| OrderSheetError::AuthFailed)?;
        let expires_at = now + token.expires_in.unwrap_or(3600);
        *cache = Some(CachedAccessToken {
            access_token: token.access_token.clone(),
            expires_at,
        });
        Ok(token.access_token)
    }

    fn token_uri(&self) -> String {
        let value = self.account.token_uri.trim();
        if value.is_empty() {
            DEFAULT_TOKEN_URI.to_string()
        } else {
            value.to_string()
        }
    }

    fn signed_assertion(&self, now: i64, token_uri: &str) -> Result<String, OrderSheetError> {
        let claims = JwtClaims {
            iss: self.account.client_email.trim(),
            scope: SHEETS_SCOPE,
            aud: token_uri,
            iat: now,
            exp: now + 3600,
        };
        let key = EncodingKey::from_rsa_pem(self.account.private_key.as_bytes())
            .map_err(|_| OrderSheetError::AuthFailed)?;
        encode(&Header::new(Algorithm::RS256), &claims, &key)
            .map_err(|_| OrderSheetError::AuthFailed)
    }
}

#[derive(Debug, Clone)]
struct CachedAccessToken {
    access_token: String,
    expires_at: i64,
}

#[derive(Debug, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    expires_in: Option<i64>,
}

#[derive(Serialize)]
struct JwtClaims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    iat: i64,
    exp: i64,
}

#[derive(Serialize)]
struct AppendValuesRequest {
    values: Vec<Vec<Value>>,
}

#[derive(Serialize)]
struct BatchUpdateRequest {
    requests: Vec<Value>,
}

fn json_insert_header_row() -> Value {
    serde_json::json!({
        "insertDimension": {
            "range": {
                "sheetId": ORDER_SHEET_ID,
                "dimension": "ROWS",
                "startIndex": 0,
                "endIndex": 1
            },
            "inheritFromBefore": false
        }
    })
}

fn sheet_format_requests() -> Vec<Value> {
    let full_range = serde_json::json!({
        "sheetId": ORDER_SHEET_ID,
        "startRowIndex": 0,
        "endRowIndex": ORDER_SHEET_FORMAT_ROW_LIMIT,
        "startColumnIndex": 0,
        "endColumnIndex": ORDER_SHEET_HEADERS.len()
    });
    let header_range = serde_json::json!({
        "sheetId": ORDER_SHEET_ID,
        "startRowIndex": 0,
        "endRowIndex": 1,
        "startColumnIndex": 0,
        "endColumnIndex": ORDER_SHEET_HEADERS.len()
    });
    let mut requests = vec![
        serde_json::json!({
            "updateSheetProperties": {
                "properties": {
                    "sheetId": ORDER_SHEET_ID,
                    "gridProperties": {
                        "frozenRowCount": 1
                    }
                },
                "fields": "gridProperties.frozenRowCount"
            }
        }),
        serde_json::json!({
            "repeatCell": {
                "range": full_range.clone(),
                "cell": {
                    "userEnteredFormat": {
                        "backgroundColor": {
                            "red": 0.61,
                            "green": 0.88,
                            "blue": 0.89
                        },
                        "textFormat": {
                            "fontFamily": "Arial",
                            "fontSize": 10,
                            "foregroundColor": {
                                "red": 0.0,
                                "green": 0.0,
                                "blue": 0.0
                            }
                        },
                        "horizontalAlignment": "CENTER",
                        "verticalAlignment": "MIDDLE",
                        "wrapStrategy": "WRAP"
                    }
                },
                "fields": "userEnteredFormat(backgroundColor,textFormat,horizontalAlignment,verticalAlignment,wrapStrategy)"
            }
        }),
        serde_json::json!({
            "repeatCell": {
                "range": header_range,
                "cell": {
                    "userEnteredFormat": {
                        "textFormat": {
                            "bold": true,
                            "fontFamily": "Arial",
                            "fontSize": 10
                        }
                    }
                },
                "fields": "userEnteredFormat.textFormat"
            }
        }),
        serde_json::json!({
            "repeatCell": {
                "range": {
                    "sheetId": ORDER_SHEET_ID,
                    "startRowIndex": 1,
                    "endRowIndex": ORDER_SHEET_FORMAT_ROW_LIMIT,
                    "startColumnIndex": 4,
                    "endColumnIndex": 5
                },
                "cell": {
                    "userEnteredFormat": {
                        "horizontalAlignment": "LEFT",
                        "textFormat": {
                            "bold": true,
                            "italic": false
                        }
                    }
                },
                "fields": "userEnteredFormat(horizontalAlignment,textFormat.bold,textFormat.italic)"
            }
        }),
        serde_json::json!({
            "repeatCell": {
                "range": {
                    "sheetId": ORDER_SHEET_ID,
                    "startRowIndex": 1,
                    "endRowIndex": ORDER_SHEET_FORMAT_ROW_LIMIT,
                    "startColumnIndex": 6,
                    "endColumnIndex": 14
                },
                "cell": {
                    "userEnteredFormat": {
                        "textFormat": {
                            "italic": true,
                            "bold": true
                        }
                    }
                },
                "fields": "userEnteredFormat.textFormat"
            }
        }),
        serde_json::json!({
            "updateBorders": {
                "range": full_range.clone(),
                "top": sheet_border(),
                "bottom": sheet_border(),
                "left": sheet_border(),
                "right": sheet_border(),
                "innerHorizontal": sheet_border(),
                "innerVertical": sheet_border()
            }
        }),
    ];
    for (column, width) in [
        (0, 34),
        (1, 86),
        (2, 86),
        (3, 86),
        (4, 360),
        (5, 84),
        (6, 92),
        (7, 92),
        (8, 92),
        (9, 106),
        (10, 106),
        (11, 106),
        (12, 106),
        (13, 100),
        (14, 90),
        (15, 110),
    ] {
        requests.push(serde_json::json!({
            "updateDimensionProperties": {
                "range": {
                    "sheetId": ORDER_SHEET_ID,
                    "dimension": "COLUMNS",
                    "startIndex": column,
                    "endIndex": column + 1
                },
                "properties": {
                    "pixelSize": width
                },
                "fields": "pixelSize"
            }
        }));
    }
    requests
}

fn sheet_border() -> Value {
    serde_json::json!({
        "style": "SOLID",
        "width": 1,
        "color": {
            "red": 0.0,
            "green": 0.0,
            "blue": 0.0
        }
    })
}

#[derive(Deserialize)]
struct SheetValuesResponse {
    #[serde(default)]
    values: Vec<Vec<Value>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::production_map::{
        ProductionMapEdge, ProductionMapNode, ProductionMapNodeKind,
    };

    #[test]
    fn order_sheet_row_matches_legacy_excel_columns() {
        let map = ProductionMapDefinition {
            id: "zakaz-7775".to_string(),
            product_code: "ITEM-1".to_string(),
            title: "fibre Mahsulot: XXL pack 70 sht".to_string(),
            code: "7775".to_string(),
            order_number: "7775".to_string(),
            roll_count: Some(8.0),
            width_mm: Some(735.0),
            nodes: vec![ProductionMapNode {
                id: "apparatus".to_string(),
                kind: ProductionMapNodeKind::Apparatus,
                title: "7 ta rangli pechat".to_string(),
                formula: None,
                role_code: String::new(),
                item_code: String::new(),
                qty_formula: String::new(),
                from_location: String::new(),
                to_location: String::new(),
                alternative_group_id: String::new(),
                alternative_group_label: String::new(),
                alternative_assigned_title: "8 ta rangli pechat".to_string(),
                x: 0.0,
                y: 0.0,
            }],
            edges: Vec::<ProductionMapEdge>::new(),
        };
        let template = CalculateOrderTemplate {
            name: "fibre Mahsulot: XXL pack 70 sht".to_string(),
            product: "fibre Mahsulot: XXL pack 70 sht".to_string(),
            width_mm: 735.0,
            waste_percent: 5.0,
            roll_count: Some(8.0),
            first_layer_material: "pet".to_string(),
            first_layer_micron: "12".to_string(),
            second_layer_material: "pe oq".to_string(),
            second_layer_micron: "55".to_string(),
            kg: 600.0,
            ..CalculateOrderTemplate::default()
        };

        let row = order_sheet_row(&map, &template).expect("row");

        assert_eq!(row.len(), 16);
        assert_eq!(row[0], Value::String("8".to_string()));
        assert_eq!(row[3], Value::String("/7775".to_string()));
        assert_eq!(
            row[4],
            Value::String("fibre Mahsulot: XXL pack 70 sht".to_string())
        );
        assert_eq!(row[6], Value::String("pet".to_string()));
        assert_eq!(row[7], Value::String("pe oq".to_string()));
        assert_eq!(row[9], json_number(735.0));
        assert_eq!(row[10], Value::String("12".to_string()));
        assert_eq!(row[11], Value::String("55".to_string()));
        assert_eq!(row[14], json_number(8.0));
        assert_eq!(row[15], json_number(750.0));
    }

    #[test]
    fn order_sheet_row_marks_flexo_orders_with_f() {
        let map = ProductionMapDefinition {
            id: "zakaz-1123".to_string(),
            product_code: "ITEM-F".to_string(),
            title: "fleksa lec Mahsulot".to_string(),
            code: String::new(),
            order_number: "1123".to_string(),
            roll_count: None,
            width_mm: Some(1190.0),
            nodes: Vec::new(),
            edges: Vec::new(),
        };
        let template = CalculateOrderTemplate {
            name: "fleksa lec Mahsulot".to_string(),
            product: "fleksa lec Mahsulot".to_string(),
            width_mm: 1190.0,
            waste_percent: 5.0,
            first_layer_material: "pe pr".to_string(),
            first_layer_micron: "50".to_string(),
            second_layer_material: "pe pr".to_string(),
            second_layer_micron: "30".to_string(),
            kg: 500.0,
            ..CalculateOrderTemplate::default()
        };

        let row = order_sheet_row(&map, &template).expect("row");

        assert_eq!(row[0], Value::String("F".to_string()));
        assert_eq!(row[3], Value::String("/1123".to_string()));
    }

    #[test]
    fn missing_order_rows_skips_existing_sheet_codes() {
        let maps = vec![
            test_map("zakaz-7775", "7775", "8 ta rangli pechat"),
            test_map("zakaz-7776", "7776", "7 ta rangli pechat"),
        ];
        let templates = vec![
            test_template("zakaz-7775", "7775"),
            test_template("zakaz-7776", "7776"),
        ];
        let existing = BTreeSet::from(["7775".to_string()]);

        let rows = missing_order_rows(&maps, &templates, &existing);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][3], Value::String("/7776".to_string()));
    }

    fn test_map(id: &str, order_number: &str, apparatus: &str) -> ProductionMapDefinition {
        ProductionMapDefinition {
            id: id.to_string(),
            product_code: "ITEM-1".to_string(),
            title: "Test order".to_string(),
            code: order_number.to_string(),
            order_number: order_number.to_string(),
            roll_count: Some(7.0),
            width_mm: Some(650.0),
            nodes: vec![ProductionMapNode {
                id: "apparatus".to_string(),
                kind: ProductionMapNodeKind::Apparatus,
                title: apparatus.to_string(),
                formula: None,
                role_code: String::new(),
                item_code: String::new(),
                qty_formula: String::new(),
                from_location: String::new(),
                to_location: String::new(),
                alternative_group_id: String::new(),
                alternative_group_label: String::new(),
                alternative_assigned_title: String::new(),
                x: 0.0,
                y: 0.0,
            }],
            edges: Vec::new(),
        }
    }

    fn test_template(source_map_id: &str, order_number: &str) -> CalculateOrderTemplate {
        CalculateOrderTemplate {
            code: order_number.to_string(),
            order_number: order_number.to_string(),
            source_map_id: source_map_id.to_string(),
            name: "Test order".to_string(),
            product: "Test order".to_string(),
            width_mm: 650.0,
            waste_percent: 5.0,
            roll_count: Some(7.0),
            first_layer_material: "pet".to_string(),
            first_layer_micron: "12".to_string(),
            second_layer_material: "pe oq".to_string(),
            second_layer_micron: "30".to_string(),
            kg: 500.0,
            ..CalculateOrderTemplate::default()
        }
    }
}
