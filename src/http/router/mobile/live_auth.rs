use super::{brigader, rezka_master, sales};
use crate::app::AppState;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::authz::Capability;
use crate::core::session::scoped_ticket::{issue_ticket, TicketScope};
use crate::http::handlers::auth::bearer_token;
use crate::http::ws::AccessDenial;
use axum::{
    extract::{Query, Request, State},
    http::{header, HeaderMap, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

#[derive(Clone)]
pub struct LiveSession {
    pub(crate) principal: Principal,
    pub(crate) digest: [u8; 32],
}

impl LiveSession {
    /// Revalidate the exact issuing session and route privilege before data
    /// leaves this connection and while the connection is idle.
    pub(crate) fn protect<F, Fut>(
        &self,
        ws: crate::http::ws::WebSocketUpgrade,
        state: &AppState,
        check: F,
    ) -> crate::http::ws::WebSocketUpgrade
    where
        F: Fn(std::sync::Arc<AppState>, Principal) -> Fut + Clone + Send + Sync + 'static,
        Fut: std::future::Future<Output = bool> + Send + 'static,
    {
        let state = std::sync::Arc::new(state.clone());
        let role = self.principal.role;
        let principal_ref: std::sync::Arc<str> = self.principal.ref_.as_str().into();
        let digest = self.digest;
        ws.with_authorization(move || {
            let state = state.clone();
            let principal_ref = principal_ref.clone();
            let check = check.clone();
            async move {
                let actor = state.sessions.get_by_digest(&digest).await
                    .map_err(|_| AccessDenial::Unauthorized)?;
                if actor.role != role || actor.ref_ != principal_ref.as_ref() {
                    return Err(AccessDenial::Unauthorized);
                }
                if actor.role == PrincipalRole::RezkaMaster
                    && !state.admin.principal_has_capability(&actor, Capability::RezkaMasterAccess).await
                {
                    return Err(AccessDenial::Forbidden);
                }
                if check(state, actor).await { Ok(()) } else { Err(AccessDenial::Forbidden) }
            }
        })
    }
}

#[derive(Deserialize)]
pub(super) struct LiveTicketRequest {
    path: String,
}

#[derive(Deserialize)]
struct Credentials {
    ticket: Option<String>,
    token: Option<String>,
}

fn is_private_live_path(path: &str) -> bool {
    matches!(path,
        "/v1/mobile/admin/warehouses/live" | "/v1/mobile/admin/system/monitor/live"
        | "/v1/mobile/admin/inventory/live" | "/v1/mobile/admin/production-maps/live"
        | "/v1/mobile/production-tasks/live" | "/v1/mobile/preparation/orders/live")
}

async fn role_allows(state: &AppState, actor: &Principal, method: &Method, path: &str) -> bool {
    match actor.role {
        PrincipalRole::SalesManager => sales::allows(method, path),
        PrincipalRole::Brigader => brigader::allows(method, path),
        PrincipalRole::RezkaMaster => rezka_master::allows(method, path) && state.admin
            .principal_has_capability(actor, Capability::RezkaMasterAccess).await,
        _ => true,
    }
}

pub(super) async fn issue(
    State(state): State<AppState>, headers: HeaderMap, Json(input): Json<LiveTicketRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    let token = bearer_token(&headers).ok_or(StatusCode::UNAUTHORIZED)?;
    let actor = state.sessions.get(&token).await.map_err(|_| StatusCode::UNAUTHORIZED)?;
    if !is_private_live_path(&input.path) {
        return Err(StatusCode::BAD_REQUEST);
    }
    if !role_allows(&state, &actor, &Method::GET, &input.path).await {
        return Err(StatusCode::FORBIDDEN);
    }
    let ticket = issue_ticket(&token, TicketScope::Live { path: &input.path });
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(serde_json::json!({ "ticket": ticket }))))
}

pub(super) async fn authenticate(
    State(state): State<AppState>, mut req: Request, next: Next,
) -> Response {
    let path = req.uri().path();
    if !is_private_live_path(path) {
        return next.run(req).await;
    }
    let Ok(Query(query)) = Query::<Credentials>::try_from_uri(req.uri()) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    // Never let a second credential select a different principal downstream.
    let has_header = req.headers().contains_key(header::AUTHORIZATION);
    let legacy = query.token.is_some();
    let token = match (has_header, query.token, &query.ticket) {
        (true, None, None) => bearer_token(req.headers()),
        (false, Some(token), None) if state.legacy_ws_query_token_until
            .is_some_and(|until| time::OffsetDateTime::now_utc() < until) => Some(token),
        (false, None, Some(_)) => None,
        _ => return StatusCode::UNAUTHORIZED.into_response(),
    };
    let session = if let Some(token) = &token {
        let Ok(principal) = state.sessions.get(token).await else {
            return StatusCode::UNAUTHORIZED.into_response();
        };
        LiveSession { principal, digest: crate::core::session::session_digest(token) }
    } else if let Some(ticket) = query.ticket {
        let Ok((digest, principal)) = state.sessions
            .resolve_ticket(&ticket, TicketScope::Live { path }).await else {
            return StatusCode::UNAUTHORIZED.into_response();
        };
        LiveSession { principal, digest }
    } else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if !role_allows(&state, &session.principal, req.method(), path).await {
        return StatusCode::FORBIDDEN.into_response();
    }
    let migration = legacy.then(|| {
        let version = req.headers().get("x-app-version").map(|header| {
            header.to_str().ok().filter(|v| !v.is_empty() && v.len() <= 128
                && !v.bytes().any(|b| b.is_ascii_control() || b"?&=".contains(&b))
                && !token.as_deref().is_some_and(|secret| v.contains(secret)))
                .unwrap_or("[redacted]").to_owned()
        });
        (path.to_owned(), session.principal.role, version)
    });
    req.extensions_mut().insert(session);
    let response = next.run(req).await;
    if response.status() == StatusCode::SWITCHING_PROTOCOLS
        && let Some((route, role, app_version)) = migration
    {
        tracing::info!(%route, ?role, app_version = app_version.as_deref(), "legacy_ws_query_connected");
    }
    response
}
