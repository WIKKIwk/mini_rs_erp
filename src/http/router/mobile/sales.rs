//! Sales reuses administrative APIs, with a closed business-operation boundary.
//! Keep this check in addition to capabilities: several legacy capabilities are
//! shared by customer creation and unrelated staff administration.
use crate::{app::AppState, core::auth::models::PrincipalRole, http::handlers::auth::bearer_token};
use axum::{
    Json,
    extract::{Request, State},
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

pub(super) async fn enforce_scope(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if let Some(token) = bearer_token(request.headers()) {
        if let Ok(principal) = state.sessions.get(&token).await {
            if principal.role == PrincipalRole::SalesManager
                && !allows(request.method(), request.uri().path())
            {
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({"error":"forbidden"})),
                )
                    .into_response();
            }
        }
    }
    next.run(request).await
}

fn allows(method: &Method, path: &str) -> bool {
    // The ordinary authenticated account and messaging APIs retain their own
    // validation; this boundary controls the reused business workspaces.
    if path.starts_with("/v1/mobile/chat/")
        || path.starts_with("/v1/mobile/profile")
        || path.starts_with("/v1/mobile/auth/")
        || path == "/v1/mobile/push/token"
    {
        return true;
    }
    if *method == Method::OPTIONS {
        return true;
    }
    let path = path.strip_prefix("/v1/mobile/").unwrap_or(path);
    if *method == Method::GET || *method == Method::HEAD {
        return matches!(
            path,
            "me" | "server/handshake"
                | "iroh-ticket"
                | "push-config"
                | "calculate/orders"
                | "calculate/orders/image/view"
                | "admin/customers"
                | "admin/customers/list"
                | "admin/customers/detail"
                | "admin/users/list"
                | "admin/profile/avatar/view"
                | "admin/items"
                | "admin/items/uoms"
                | "admin/items/detail"
                | "admin/item-groups"
                | "admin/item-groups/tree"
                | "admin/combos"
                | "admin/calculate-materials"
                | "admin/pending-orders"
                | "admin/pending-orders/image"
                | "admin/apparatus"
                | "admin/apparatus/options"
                | "admin/apparatus-collections"
                | "admin/production-maps"
                | "admin/production-maps/audit"
                | "admin/production-maps/capacity"
                | "admin/production-maps/order-page"
                | "admin/production-maps/closed-order"
                | "admin/production-maps/order-summary"
                | "admin/production-maps/order-edit"
                | "admin/production-maps/sequence"
                | "admin/production-maps/replay"
                | "admin/production-maps/queue-policies"
                | "admin/production-maps/live"
                | "admin/production-maps/order-image/view"
                | "admin/production-maps/completed-orders"
                | "admin/production-maps/closed-orders"
                | "admin/production-maps/completion-requests"
                | "admin/production-maps/completion-request-decisions"
                | "admin/production-maps/progress-qr/lookup"
                | "admin/production-maps/progress-qr/history"
                | "admin/production-maps/progress-qr/report"
                | "admin/production-maps/wip-batches"
                | "admin/production-maps/paddons"
                | "admin/production-maps/paddons/detail"
                | "admin/production-maps/paddons/qr/report"
                | "admin/production-maps/order-scan-bootstrap"
                | "admin/production-maps/rezka-output-report"
                | "admin/production-maps/output-stats"
                | "admin/production-maps/layer-remainders"
                | "admin/production-maps/lamination-settings"
                | "admin/production-maps/qolip-start-settings"
        ) || path.starts_with("admin/apparatus/")
            || path.starts_with("app-update/android");
    }
    matches!(
        (method.as_str(), path),
        (
            "POST",
            "calculate"
                | "calculate/orders"
                | "calculate/orders/image"
                | "admin/customers"
                | "admin/items"
                | "admin/combos"
                | "admin/production-maps/auto-open"
                | "admin/production-maps/progress-qr/lookup"
                | "admin/production-maps/progress-qr/report"
        ) | (
            "PUT",
            "admin/production-maps"
                | "admin/production-maps/with-order"
                | "admin/production-maps/order-edit"
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sales_scope_allows_planning_and_rejects_operational_methods() {
        assert!(allows(&Method::PUT, "/v1/mobile/admin/production-maps"));
        assert!(allows(
            &Method::GET,
            "/v1/mobile/admin/production-maps/sequence"
        ));
        for method in [Method::GET, Method::POST, Method::PUT, Method::DELETE] {
            for operation in [
                "queue-action",
                "move",
                "move-batch",
                "apparatus-transfer",
                "order-control",
                "temporary-pause",
                "re-pause",
                "reactivation",
                "paddons/create",
                "progress-qr/reprint",
            ] {
                assert!(
                    !allows(
                        &method,
                        &format!("/v1/mobile/admin/production-maps/{operation}")
                    ),
                    "{method} {operation}"
                );
            }
        }
        assert!(!allows(
            &Method::POST,
            "/v1/mobile/admin/material-taminotchilar"
        ));
        assert!(!allows(&Method::PUT, "/v1/mobile/admin/items/detail"));
        assert!(!allows(&Method::PUT, "/v1/mobile/admin/customers/phone"));
    }
}
