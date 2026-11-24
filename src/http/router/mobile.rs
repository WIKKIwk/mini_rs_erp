mod admin;
mod brigader;
mod core;
mod live_auth;
pub(crate) use live_auth::LiveSession;
mod sales;
mod rezka_master;

use axum::Router;
use tower_http::trace::TraceLayer;

use crate::app::AppState;

pub(super) fn routes(state: AppState) -> Router {
    Router::new()
        .route("/v1/mobile/auth/live-ticket", axum::routing::post(live_auth::issue))
        .merge(core::routes())
        .merge(admin::routes())
        .merge(rezka_master::routes())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            sales::enforce_scope,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            brigader::enforce_scope,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            live_auth::authenticate,
        ))
        // Trace all outcomes, including rejected upgrades, using only the URI path.
        .layer(TraceLayer::new_for_http().make_span_with(|request: &axum::extract::Request| {
            tracing::debug_span!("http.request", method = %request.method(), path = request.uri().path())
        }))
        .with_state(state)
}
