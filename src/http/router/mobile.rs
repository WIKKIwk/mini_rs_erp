mod admin;
mod brigader;
mod core;
mod sales;

use axum::Router;
use tower_http::trace::TraceLayer;

use crate::app::AppState;

pub(super) fn routes(state: AppState) -> Router {
    Router::new()
        .merge(core::routes())
        .merge(admin::routes())
        .layer(TraceLayer::new_for_http())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            sales::enforce_scope,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            brigader::enforce_scope,
        ))
        .with_state(state)
}
