use axum::Router;
use axum::body::Body;
use axum::http::{Response, StatusCode, header};
use axum::routing::any;

const HEALTHZ_BODY: &str = r#"{"ok":true}"#;

pub(super) fn routes() -> Router {
    Router::new().route("/healthz", any(healthz))
}

async fn healthz() -> Response<Body> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(HEALTHZ_BODY))
        .expect("static health response is valid")
}
