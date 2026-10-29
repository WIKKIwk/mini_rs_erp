//! Brigaders reuse administrative components within the requested workspace.
//! Keep its closed endpoint boundary alongside the existing capability checks.
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
    // WebSocket clients authenticate through the query token. Checking both
    // credentials also closes mixed-header/query requests with distinct actors.
    let query_token = axum::extract::Query::<WorkspaceAuthQuery>::try_from_uri(request.uri())
        .ok()
        .and_then(|query| query.0.token);
    for token in [bearer_token(request.headers()), query_token]
        .into_iter()
        .flatten()
    {
        if let Ok(principal) = state.sessions.get(token.trim()).await {
            if principal.role == PrincipalRole::Brigader
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

#[derive(serde::Deserialize)]
struct WorkspaceAuthQuery {
    token: Option<String>,
}

fn under(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|tail| tail.starts_with('/'))
}

fn allows(method: &Method, path: &str) -> bool {
    if *method == Method::OPTIONS {
        return true;
    }
    let Some(path) = path.strip_prefix("/v1/mobile/") else {
        return path == "/v1/telegram/start";
    };
    // Ordinary account, communication and the dependencies of the shared pages.
    if [
        "auth",
        "profile",
        "chat",
        "notifications",
        "push",
        "calculate",
        "returned-paint",
        "rps/batch",
        "material-link-requests",
        "worker-material-use",
        "stock-entry",
    ]
    .iter()
    .any(|root| under(path, root))
    {
        return true;
    }
    if [
        "admin/production-maps",
        "admin/pending-orders",
        "admin/combos",
        "admin/suppliers",
        "admin/customers",
        "admin/material-taminotchilar",
        "admin/system-users",
        "admin/workers",
        "admin/worker-groups",
        "admin/items",
        "admin/item-groups",
        "admin/apparatus",
        "admin/apparatus-collections",
        "admin/factory-locations",
        "admin/warehouses",
        "admin/inventory",
        "admin/calculate-materials",
        "admin/raw-material-rules",
        "admin/raw-material-assignments",
        "admin/raw-material-start-receipt",
        "admin/raw-material-start-requirements",
        "admin/raw-material-intake",
        "admin/raw-material-intake-candidates",
        "admin/raw-material-stock",
        "admin/raw-material-history",
        "admin/app-update/settings",
        "admin/werka/code/regenerate",
    ]
    .iter()
    .any(|root| under(path, root))
    {
        return true;
    }
    if path == "admin/role-assignments" {
        return matches!(*method, Method::GET | Method::PUT);
    }
    if matches!(*method, Method::GET | Method::HEAD) {
        return matches!(
            path,
            "me" | "server/handshake"
                | "iroh-ticket"
                | "push-config"
                | "admin/users/list"
                | "admin/activity"
                | "admin/roles"
                | "admin/capabilities"
                | "admin/profile/avatar/view"
        ) || under(path, "app-update/android")
            || under(path, "qolip")
            || matches!(path, "gscale/items" | "gscale/material-tasks");
    }
    matches!(
        path,
        "gscale/material-receipt/print" | "gscale/material-receipt/simple"
    ) && *method == Method::POST
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brigader_reuses_requested_admin_operations_and_closes_system_administration() {
        for path in [
            "admin/production-maps/queue-action",
            "admin/production-maps/move",
            "admin/production-maps/paddons/management-settings",
            "admin/app-update/settings",
            "admin/workers",
            "admin/system-users",
            "admin/items",
            "admin/item-groups",
            "admin/apparatus/new/aasx",
            "admin/factory-locations/state",
            "admin/warehouses",
            "admin/raw-material-rules",
            "admin/calculate-materials",
            "calculate/orders",
        ] {
            assert!(
                allows(&Method::POST, &format!("/v1/mobile/{path}")),
                "{path}"
            );
        }
        for path in [
            "admin/settings",
            "admin/roles",
            "admin/telegram/settings",
            "admin/push-config",
            "admin/system/monitor",
            "admin/system/backups",
            "admin/emergency-reset/orders",
            "admin/training/restart",
            "admin/items-forged",
        ] {
            assert!(
                !allows(&Method::POST, &format!("/v1/mobile/{path}")),
                "{path}"
            );
        }
        assert!(allows(&Method::GET, "/v1/mobile/admin/roles"));
        assert!(!allows(&Method::PUT, "/v1/mobile/admin/roles"));
    }
}
