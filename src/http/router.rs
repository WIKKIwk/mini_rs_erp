use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, Method, Request, Response, StatusCode, header};
use axum::middleware::{self, Next};
use axum::routing::any;
use tower_http::trace::TraceLayer;

use crate::app::AppState;
use crate::http::handlers::{
    admin, auth, calculate, customer, gscale, notifications, profile, push, rezka, rps_batch,
    stock_entry, supplier, werka,
};

pub fn build_router(state: AppState) -> Router {
    let health_routes = Router::new().route("/healthz", any(healthz));

    let api_routes = Router::new()
        .route("/v1/mobile/auth/login", any(auth::login))
        .route("/v1/mobile/auth/logout", any(auth::logout))
        .route("/v1/mobile/me", any(auth::me))
        .route("/v1/mobile/calculate", any(calculate::calculate_route))
        .route(
            "/v1/mobile/calculate/orders",
            any(calculate::calculate_orders_route),
        )
        .route(
            "/v1/mobile/calculate/orders/delete",
            any(calculate::calculate_order_delete_route),
        )
        .route(
            "/v1/mobile/calculate/orders/image",
            any(calculate::calculate_order_image_upload_route),
        )
        .route(
            "/v1/mobile/calculate/orders/image/view",
            any(calculate::calculate_order_image_view_route),
        )
        .route("/v1/mobile/profile", any(profile::profile))
        .route("/v1/mobile/profile/avatar", any(profile::avatar_upload))
        .route("/v1/mobile/push/token", any(push::token))
        .route("/v1/mobile/gscale/items", any(gscale::items))
        .route(
            "/v1/mobile/gscale/material-receipt/print",
            any(gscale::material_receipt_print),
        )
        .route("/v1/mobile/rps/batch/start", any(rps_batch::start))
        .route("/v1/mobile/rps/batch/state", any(rps_batch::state))
        .route("/v1/mobile/rps/batch/stop", any(rps_batch::stop))
        .route("/v1/mobile/rps/batch/print", any(rps_batch::print))
        .route("/v1/mobile/rezka/source", any(rezka::source))
        .route("/v1/mobile/rezka/split", any(rezka::split))
        .route("/v1/mobile/stock-entry/lookup", any(stock_entry::lookup))
        .route("/v1/mobile/customer/summary", any(customer::summary))
        .route("/v1/mobile/customer/history", any(customer::history))
        .route(
            "/v1/mobile/customer/status-details",
            any(customer::status_details),
        )
        .route("/v1/mobile/customer/detail", any(customer::detail))
        .route("/v1/mobile/customer/respond", any(customer::respond))
        .route(
            "/v1/mobile/notifications/detail",
            any(notifications::detail),
        )
        .route(
            "/v1/mobile/notifications/comments",
            any(notifications::comment),
        )
        .route("/v1/mobile/profile/avatar/view", any(profile::avatar_view))
        .route(
            "/v1/mobile/supplier/dispatch",
            any(supplier::create_dispatch),
        )
        .route("/v1/mobile/supplier/history", any(supplier::history))
        .route("/v1/mobile/supplier/items", any(supplier::items))
        .route(
            "/v1/mobile/supplier/status-breakdown",
            any(supplier::status_breakdown),
        )
        .route(
            "/v1/mobile/supplier/status-details",
            any(supplier::status_details),
        )
        .route("/v1/mobile/supplier/summary", any(supplier::summary))
        .route(
            "/v1/mobile/supplier/unannounced/respond",
            any(supplier::unannounced_respond),
        )
        .route("/v1/mobile/werka/archive", any(werka::archive))
        .route("/v1/mobile/werka/archive/pdf", any(werka::archive_pdf))
        .route(
            "/v1/mobile/werka/ai-search-suggestion",
            any(werka::ai_search_suggestion),
        )
        .route("/v1/mobile/werka/confirm", any(werka::confirm))
        .route(
            "/v1/mobile/werka/customer-issue/create",
            any(werka::customer_issue_create),
        )
        .route(
            "/v1/mobile/werka/customer-issue/batch-create",
            any(werka::customer_issue_batch_create),
        )
        .route(
            "/v1/mobile/werka/unannounced/create",
            any(werka::unannounced_create),
        )
        .route("/v1/mobile/werka/history", any(werka::history))
        .route("/v1/mobile/werka/notifications", any(werka::history))
        .route("/v1/mobile/werka/pending", any(werka::pending))
        .route(
            "/v1/mobile/werka/status-breakdown",
            any(werka::status_breakdown),
        )
        .route(
            "/v1/mobile/werka/status-details",
            any(werka::status_details),
        )
        .route("/v1/mobile/werka/summary", any(werka::summary))
        .route(
            "/v1/mobile/werka/customer-item-options",
            any(werka::customer_item_options),
        )
        .route(
            "/v1/mobile/werka/customer-items",
            any(werka::customer_items),
        )
        .route(
            "/v1/mobile/werka/supplier-items",
            any(werka::supplier_items),
        )
        .route("/v1/mobile/werka/customers", any(werka::customers))
        .route("/v1/mobile/werka/suppliers", any(werka::suppliers))
        .route("/v1/mobile/werka/home", any(werka::home))
        .route("/v1/mobile/admin/settings", any(admin::settings))
        .route(
            "/v1/mobile/admin/apparatus-groups",
            any(admin::apparatus_groups),
        )
        .route("/v1/mobile/admin/capabilities", any(admin::capabilities))
        .route("/v1/mobile/admin/roles", any(admin::roles))
        .route("/v1/mobile/admin/workers", any(admin::workers))
        .route("/v1/mobile/admin/worker-groups", any(admin::worker_groups))
        .route(
            "/v1/mobile/admin/production-maps",
            any(admin::production_maps),
        )
        .route(
            "/v1/mobile/admin/production-maps/run",
            any(admin::production_map_run),
        )
        .route(
            "/v1/mobile/admin/production-maps/with-order",
            any(admin::production_map_save_with_order),
        )
        .route(
            "/v1/mobile/admin/production-maps/move",
            any(admin::production_map_move),
        )
        .route(
            "/v1/mobile/admin/production-maps/move-batch",
            any(admin::production_map_move_batch),
        )
        .route(
            "/v1/mobile/admin/production-maps/sequence",
            any(admin::production_map_sequence),
        )
        .route(
            "/v1/mobile/admin/production-maps/live",
            any(admin::production_map_live),
        )
        .route(
            "/v1/mobile/admin/production-maps/queue-action",
            any(admin::production_map_queue_action),
        )
        .route(
            "/v1/mobile/admin/role-assignments",
            any(admin::role_assignments),
        )
        .route("/v1/mobile/admin/suppliers", any(admin::suppliers))
        .route("/v1/mobile/admin/suppliers/list", any(admin::supplier_list))
        .route(
            "/v1/mobile/admin/suppliers/summary",
            any(admin::supplier_summary),
        )
        .route(
            "/v1/mobile/admin/suppliers/detail",
            any(admin::supplier_detail),
        )
        .route(
            "/v1/mobile/admin/suppliers/inactive",
            any(admin::inactive_suppliers),
        )
        .route(
            "/v1/mobile/admin/suppliers/items/assigned",
            any(admin::assigned_supplier_items),
        )
        .route(
            "/v1/mobile/admin/suppliers/status",
            any(admin::supplier_status),
        )
        .route(
            "/v1/mobile/admin/suppliers/phone",
            any(admin::supplier_phone),
        )
        .route(
            "/v1/mobile/admin/suppliers/items",
            any(admin::supplier_items),
        )
        .route(
            "/v1/mobile/admin/suppliers/items/add",
            any(admin::supplier_item_add),
        )
        .route(
            "/v1/mobile/admin/suppliers/items/remove",
            any(admin::supplier_item_remove),
        )
        .route(
            "/v1/mobile/admin/suppliers/code/regenerate",
            any(admin::supplier_code_regenerate),
        )
        .route(
            "/v1/mobile/admin/suppliers/remove",
            any(admin::supplier_remove),
        )
        .route(
            "/v1/mobile/admin/suppliers/restore",
            any(admin::supplier_restore),
        )
        .route("/v1/mobile/admin/customers", any(admin::customers))
        .route("/v1/mobile/admin/customers/list", any(admin::customer_list))
        .route(
            "/v1/mobile/admin/customers/detail",
            any(admin::customer_detail),
        )
        .route(
            "/v1/mobile/admin/customers/phone",
            any(admin::customer_phone),
        )
        .route(
            "/v1/mobile/admin/customers/code/regenerate",
            any(admin::customer_code_regenerate),
        )
        .route(
            "/v1/mobile/admin/customers/items/add",
            any(admin::customer_item_add),
        )
        .route(
            "/v1/mobile/admin/customers/items/remove",
            any(admin::customer_item_remove),
        )
        .route(
            "/v1/mobile/admin/customers/remove",
            any(admin::customer_remove),
        )
        .route("/v1/mobile/admin/items", any(admin::items))
        .route("/v1/mobile/admin/apparatus", any(admin::apparatus_create))
        .route("/v1/mobile/admin/warehouses", any(admin::warehouses))
        .route(
            "/v1/mobile/admin/items/bulk-move-group",
            any(admin::items_bulk_move_group),
        )
        .route(
            "/v1/mobile/admin/item-groups/tree",
            any(admin::item_group_tree),
        )
        .route("/v1/mobile/admin/item-groups", any(admin::item_groups))
        .route("/v1/mobile/admin/activity", any(admin::activity))
        .route(
            "/v1/mobile/admin/werka/code/regenerate",
            any(admin::werka_code_regenerate),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    health_routes
        .merge(api_routes)
        .layer(middleware::from_fn(cors_headers))
}

const HEALTHZ_BODY: &str = r#"{"ok":true}"#;

async fn healthz() -> Response<Body> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(HEALTHZ_BODY))
        .expect("static health response is valid")
}

async fn cors_headers(req: Request<Body>, next: Next) -> Response<Body> {
    if req.method() == Method::OPTIONS {
        let mut response = Response::new(Body::empty());
        *response.status_mut() = StatusCode::NO_CONTENT;
        insert_cors_headers(response.headers_mut());
        return response;
    }

    let mut response = next.run(req).await;
    insert_cors_headers(response.headers_mut());
    response
}

fn insert_cors_headers(headers: &mut HeaderMap) {
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET,POST,PUT,PATCH,DELETE,OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("authorization,content-type"),
    );
    headers.insert(
        header::ACCESS_CONTROL_MAX_AGE,
        HeaderValue::from_static("86400"),
    );
}
