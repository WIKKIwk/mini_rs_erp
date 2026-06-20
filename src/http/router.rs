mod cors;
mod health;
mod mobile;

use axum::Router;
use axum::middleware;

use crate::app::AppState;

pub fn build_router(state: AppState) -> Router {
    health::routes()
        .merge(mobile::routes(state))
        .layer(middleware::from_fn(cors::cors_headers))
}
