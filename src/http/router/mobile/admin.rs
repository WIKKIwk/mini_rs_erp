use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{any, get, post};

use crate::app::AppState;
use crate::http::handlers::admin;
pub(super) fn routes() -> Router<AppState> {
    identity_routes()
        .merge(production_routes())
        .merge(material_routes())
        .merge(catalog_routes())
        .merge(inventory_and_system_routes())
}

include!("admin_route_parts/identity.rs");
include!("admin_route_parts/production.rs");
include!("admin_route_parts/material.rs");
include!("admin_route_parts/catalog.rs");
include!("admin_route_parts/inventory_and_system.rs");
