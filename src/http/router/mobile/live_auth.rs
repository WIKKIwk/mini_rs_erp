use super::{brigader, rezka_master, sales};
use crate::app::AppState;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::authz::Capability;
use crate::core::session::scoped_ticket::{issue_ticket, TicketScope};
use crate::http::handlers::auth::bearer_token;
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
    if query.token.is_some() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    // Use exactly one credential source. Every workspace policy and handler
    // receives this same principal, regardless of how the socket authenticated.
    let session = match (req.headers().contains_key(header::AUTHORIZATION), query.ticket) {
        (true, None) => {
            let Some(token) = bearer_token(req.headers()) else {
                return StatusCode::UNAUTHORIZED.into_response();
            };
            let Ok(principal) = state.sessions.get(&token).await else {
                return StatusCode::UNAUTHORIZED.into_response();
            };
            LiveSession { principal, digest: crate::core::session::session_digest(&token) }
        }
        (false, Some(ticket)) => {
            let Ok((digest, principal)) = state.sessions
                .resolve_ticket(&ticket, TicketScope::Live { path }).await else {
                return StatusCode::UNAUTHORIZED.into_response();
            };
            LiveSession { principal, digest }
        }
        _ => return StatusCode::UNAUTHORIZED.into_response(),
    };
    if !role_allows(&state, &session.principal, req.method(), path).await {
        return StatusCode::FORBIDDEN.into_response();
    }
    req.extensions_mut().insert(session);
    next.run(req).await
}
