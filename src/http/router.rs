mod cors;
mod health;
mod mobile;

use axum::Router;
use axum::middleware;
use axum::http::{Extensions, HeaderMap, StatusCode, Version, header};
use tower_http::compression::{CompressionLayer, CompressionLevel, DefaultPredicate, Predicate};

use crate::app::AppState;

pub fn build_router(state: AppState) -> Router {
    let gzip_level = crate::config::http_gzip_level_from_env()
        .expect("HTTP gzip level must be validated before building the router");
    health::routes()
        .merge(mobile::routes(state))
        .layer(CompressionLayer::new()
            .quality(CompressionLevel::Precise(gzip_level))
            .compress_when(DefaultPredicate::new().and(compress_json)))
        .layer(middleware::from_fn(cors::cors_headers))
}

fn compress_json(_: StatusCode, _: Version, headers: &HeaderMap, _: &Extensions) -> bool {
    headers.get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(';').next().is_some_and(|mime| {
            let mime = mime.trim();
            mime == "application/json" || mime.ends_with("+json")
        }))
}
