use crate::{app::AppState, http::handlers::admin::rezka_master};
use axum::{
    Router,
    http::Method,
    routing::{get, put},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/mobile/rezka-master/workers",
            get(rezka_master::workers).put(rezka_master::assign_worker),
        )
        .route(
            "/v1/mobile/rezka-master/order-settings",
            put(rezka_master::order_settings),
        )
}

pub(super) fn allows(method: &Method, path: &str) -> bool {
    if *method == Method::OPTIONS {
        return true;
    }
    let Some(path) = path.strip_prefix("/v1/mobile/") else {
        return false;
    };
    if path == "me"
        || path == "profile"
        || path == "server/handshake"
        || path == "app-update/android"
        || [
            "auth/",
            "profile/",
            "chat/",
            "notifications/",
            "app-update/android/",
        ]
        .iter()
        .any(|prefix| path.starts_with(prefix))
    {
        return true;
    }
    if path == "rezka-master/workers" {
        return matches!(*method, Method::GET | Method::PUT);
    }
    if path == "rezka-master/order-settings" {
        return *method == Method::PUT;
    }
    if path == "admin/apparatus" {
        return *method == Method::GET;
    }
    if matches!(
        path,
        "admin/raw-material-start-requirements"
            | "admin/raw-material-assignments"
            | "admin/raw-material-intake-candidates"
    ) {
        return *method == Method::GET;
    }
    if path == "admin/raw-material-intake" {
        return *method == Method::POST;
    }
    let root = "admin/production-maps";
    if path == root {
        return *method == Method::GET;
    }
    let Some(path) = path
        .strip_prefix(root)
        .and_then(|tail| tail.strip_prefix('/'))
    else {
        return false;
    };
    match *method {
        Method::GET => matches!(
            path,
            "live"
                | "sequence"
                | "order-scan-bootstrap"
                | "order-image/view"
                | "completed-orders"
                | "wip-batches"
                | "output-stats"
                | "rezka-output-report"
                | "rezka-astatka"
                | "completion-request-decisions"
                | "scanned-wips"
                | "progress-qr/history"
                | "paddons"
                | "paddons/detail"
                | "paddons/qr/report"
                | "paddons/management-settings"
                | "paddons/active"
        ),
        Method::PUT => matches!(path, "paddons/management-settings" | "paddons/active"),
        Method::POST => matches!(
            path,
            "queue-action"
                | "rezka-astatka"
                | "progress-qr/lookup"
                | "progress-qr/reprint"
                | "progress-qr/report"
                | "progress-qr/correct"
                | "cancel-start"
                | "temporary-pause"
                | "re-pause"
                | "scanned-wips/return"
                | "order-alert"
                | "reactivation"
                | "reactivation/preview"
                | "reactivation/reclose"
                | "paddons/create"
                | "paddons/delete"
                | "paddons/unlock"
                | "paddons/active/next"
                | "paddons/qr/print"
                | "paddons/qr/confirm"
                | "paddons/items/transfer"
                | "paddons/items/add"
                | "paddons/items/add-batch"
                | "paddons/items/remove"
                | "paddons/items/remove-batch"
        ),
        _ => false,
    }
}
