use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct MobileServerHandshakeResponse {
    pub service: &'static str,
    pub product: &'static str,
    pub api_contract: &'static str,
    pub version: &'static str,
}

pub async fn mobile_server_handshake() -> Json<MobileServerHandshakeResponse> {
    Json(MobileServerHandshakeResponse {
        service: "mini_rs_erp",
        product: "mini_rs_erp",
        api_contract: "v1",
        version: env!("CARGO_PKG_VERSION"),
    })
}
