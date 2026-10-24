mod cors;
mod health;
mod mobile;

use axum::Router;
use axum::middleware;
use axum::http::{Extensions, HeaderMap, StatusCode, Version, header};
use tower_http::compression::{CompressionLayer, CompressionLevel, DefaultPredicate, Predicate};

use crate::app::AppState;

/// A response-local transport experiment. It never changes payload identity,
/// worker defaults, WebSocket compression, or the global gzip configuration.
#[derive(Clone, Copy, Debug)]
pub(crate) struct IdentityHttpResponse;

pub(crate) fn identity_response(mut response: axum::response::Response) -> axum::response::Response {
    response.extensions_mut().insert(IdentityHttpResponse);
    response
}

pub fn build_router(state: AppState) -> Router {
    let gzip_level = crate::config::http_gzip_level_from_env()
        .expect("HTTP gzip level must be validated before building the router");
    health::routes(state.clone())
        .merge(mobile::routes(state))
        .layer(CompressionLayer::new()
            .quality(CompressionLevel::Precise(gzip_level))
            .compress_when(DefaultPredicate::new().and(compress_json)))
        .layer(middleware::from_fn(cors::cors_headers))
}

fn compress_json(_: StatusCode, _: Version, headers: &HeaderMap, extensions: &Extensions) -> bool {
    if extensions.get::<IdentityHttpResponse>().is_some() {
        return false;
    }
    headers.get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(';').next().is_some_and(|mime| {
            let mime = mime.trim();
            mime == "application/json" || mime.ends_with("+json")
        }))
}
