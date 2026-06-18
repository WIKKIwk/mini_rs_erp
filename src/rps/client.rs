use std::time::Duration;

use async_trait::async_trait;
use reqwest::Url;
use serde::Serialize;

use crate::core::gscale::models::{ScaleDriverPrintRequest, ScaleDriverPrintResponse};
use crate::core::gscale::ports::{GscalePortError, ScaleDriverPort};

#[derive(Clone)]
pub struct RpsDriverClient {
    http: reqwest::Client,
    default_driver_url: String,
}

impl RpsDriverClient {
    pub fn new(timeout: Duration, default_driver_url: String) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(timeout)
                .build()
                .expect("reqwest client"),
            default_driver_url: default_driver_url.trim().trim_end_matches('/').to_string(),
        }
    }
}

#[async_trait]
impl ScaleDriverPort for RpsDriverClient {
    async fn print_material_receipt(
        &self,
        request: ScaleDriverPrintRequest,
    ) -> Result<ScaleDriverPrintResponse, GscalePortError> {
        let url = self.print_url(&request.driver_url)?;
        let response = self
            .http
            .post(url)
            .json(&RpsPrintRequest::from(request))
            .send()
            .await
            .map_err(|error| GscalePortError::Driver(error.to_string()))?;
        decode_print_response(response).await
    }
}

impl RpsDriverClient {
    fn print_url(&self, request_driver_url: &str) -> Result<Url, GscalePortError> {
        let base = if request_driver_url.trim().is_empty() {
            self.default_driver_url.as_str()
        } else {
            request_driver_url.trim()
        };
        if base.is_empty() {
            return Err(GscalePortError::NotConfigured(
                "scale driver url is required".to_string(),
            ));
        }
        let base = base.trim_end_matches('/');
        let url = Url::parse(&format!("{base}/v1/driver/print"))
            .map_err(|error| GscalePortError::InvalidInput(error.to_string()))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(GscalePortError::InvalidInput(
                "scale driver url must be http or https".to_string(),
            ));
        }
        Ok(url)
    }
}

async fn decode_print_response(
    response: reqwest::Response,
) -> Result<ScaleDriverPrintResponse, GscalePortError> {
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| GscalePortError::Driver(error.to_string()))?;
    let parsed: ScaleDriverPrintResponse =
        serde_json::from_str(&body).unwrap_or_else(|_| ScaleDriverPrintResponse {
            status: status.as_str().to_string(),
            detail: body.trim().to_string(),
            ..ScaleDriverPrintResponse::default()
        });
    if status.is_success() {
        Ok(parsed)
    } else {
        Err(GscalePortError::Driver(print_error_detail(parsed, status)))
    }
}

fn print_error_detail(parsed: ScaleDriverPrintResponse, status: reqwest::StatusCode) -> String {
    if parsed.error.trim() == "driver_busy" {
        return "Printer band. Boshqa mobile print qilmoqda, keyin qayta urining.".to_string();
    }
    for value in [parsed.detail, parsed.error, parsed.status] {
        let value = value.trim().to_string();
        if !value.is_empty() {
            return value;
        }
    }
    format!("scale driver http {}", status.as_u16())
}

#[derive(Debug, Serialize)]
struct RpsPrintRequest {
    epc: String,
    item_code: String,
    item_name: String,
    warehouse: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    executor_name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    label_kind: String,
    printer: String,
    print_mode: String,
    gross_qty: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    qty: Option<f64>,
    unit: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    progress_unit: String,
    tare_enabled: bool,
    tare_kg: f64,
    print_count: u32,
}

impl From<ScaleDriverPrintRequest> for RpsPrintRequest {
    fn from(request: ScaleDriverPrintRequest) -> Self {
        Self {
            epc: request.epc,
            item_code: request.item_code,
            item_name: request.item_name,
            warehouse: request.warehouse,
            executor_name: request.executor_name,
            label_kind: request.label_kind,
            printer: request.printer,
            print_mode: request.print_mode,
            gross_qty: request.gross_qty,
            qty: request.qty,
            unit: request.unit,
            progress_unit: request.progress_unit,
            tare_enabled: request.tare_enabled,
            tare_kg: request.tare_kg,
            print_count: request.print_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_driver_print_url_from_request_or_default() {
        let client = RpsDriverClient::new(
            Duration::from_secs(1),
            "http://127.0.0.1:39117/".to_string(),
        );

        assert_eq!(
            client.print_url("").unwrap().as_str(),
            "http://127.0.0.1:39117/v1/driver/print"
        );
        assert_eq!(
            client
                .print_url("http://10.0.0.5:41257/base")
                .unwrap()
                .as_str(),
            "http://10.0.0.5:41257/base/v1/driver/print"
        );
    }

    #[test]
    fn rejects_missing_or_non_http_driver_url() {
        let client = RpsDriverClient::new(Duration::from_secs(1), String::new());

        assert!(matches!(
            client.print_url("").unwrap_err(),
            GscalePortError::NotConfigured(_)
        ));
        assert!(matches!(
            client.print_url("file:///tmp/rps").unwrap_err(),
            GscalePortError::InvalidInput(_)
        ));
    }

    #[test]
    fn print_error_detail_maps_driver_busy_to_operator_message() {
        let parsed = ScaleDriverPrintResponse {
            error: "driver_busy".to_string(),
            detail: "Printer server band. Boshqa mobile print yakunlagandan keyin qayta urining."
                .to_string(),
            ..ScaleDriverPrintResponse::default()
        };

        assert_eq!(
            print_error_detail(parsed, reqwest::StatusCode::CONFLICT),
            "Printer band. Boshqa mobile print qilmoqda, keyin qayta urining."
        );
    }
}
