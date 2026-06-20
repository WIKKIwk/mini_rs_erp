mod admin;
mod core;

use axum::Router;
use tower_http::trace::TraceLayer;

use crate::app::AppState;

pub(super) fn routes(state: AppState) -> Router {
    Router::new()
        .merge(core::routes())
        .merge(admin::routes())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
