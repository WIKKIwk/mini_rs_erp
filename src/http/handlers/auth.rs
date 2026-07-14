use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::app::AppState;
use crate::core::auth::models::{LoginRequest, LoginResponse, Principal, PrincipalRole};
use crate::core::auth::service::AuthError;
use crate::core::authz::Capability;

pub async fn login(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<LoginResponse>, (StatusCode, Json<ErrorResponse>)> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let request: LoginRequest = parse_json(&body)?;
    let mut principal = state
        .auth
        .login(request.phone.trim(), request.code.trim())
        .await
        .map_err(login_error)?;
    principal = state.profiles.refresh(principal).await;
    if principal.role == PrincipalRole::Qolipchi
        && !state
            .admin
            .principal_has_capability(&principal, Capability::QolipManage)
            .await
    {
        return Err(login_error(AuthError::InvalidCredentials));
    }
    if principal.role == PrincipalRole::Boyoqchi
        && !state
            .admin
            .principal_has_capability(&principal, Capability::BoyoqchiAccess)
            .await
    {
        return Err(login_error(AuthError::InvalidCredentials));
    }
    let token = state
        .sessions
        .create(principal.clone())
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "session create failed",
                }),
            )
        })?;

    let werka_home = if principal.role == PrincipalRole::Werka {
        state.werka.home(20).await.ok().flatten()
    } else {
        None
    };
    let capabilities = state.admin.principal_capability_codes(&principal).await;
    let assigned_apparatus = state.admin.principal_assigned_apparatus(&principal).await;
    let assigned_item_groups = state.admin.principal_assigned_item_groups(&principal).await;
    let assigned_warehouses = state
        .warehouses
        .assigned_warehouse_names(&principal)
        .await
        .unwrap_or_default();

    Ok(Json(LoginResponse {
        profile: with_avatar_proxy(&headers, principal, &token),
        token,
        capabilities,
        assigned_apparatus,
        assigned_item_groups,
        assigned_warehouses,
        werka_home,
    }))
}

pub async fn logout(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Json<OkResponse>, (StatusCode, Json<ErrorResponse>)> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let token = bearer_token(&headers).ok_or_else(unauthorized)?;
    state.sessions.delete(&token).await;

    Ok(Json(OkResponse { ok: true }))
}

pub async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Principal>, (StatusCode, Json<ErrorResponse>)> {
    let token = bearer_token(&headers).ok_or_else(unauthorized)?;
    let mut principal = state
        .sessions
        .get(&token)
        .await
        .map_err(|_| unauthorized())?;
    principal = state.profiles.refresh(principal).await;
    state.sessions.update(&token, principal.clone()).await;

    Ok(Json(with_avatar_proxy(&headers, principal, &token)))
}

pub fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let raw = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let token = raw.strip_prefix("Bearer ")?.trim();

    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

pub(crate) fn with_avatar_proxy(
    headers: &HeaderMap,
    mut principal: Principal,
    token: &str,
) -> Principal {
    if principal.ref_.trim().is_empty() || principal.avatar_url.trim().is_empty() {
        return principal;
    }

    let Some(host) = headers
        .get(axum::http::header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return principal;
    };

    principal.avatar_url = format!(
        "{}://{}/v1/mobile/profile/avatar/view?token={}&v={}",
        request_scheme(headers),
        host,
        urlencoding::encode(token.trim()),
        urlencoding::encode(avatar_version(&principal.avatar_url))
    );
    principal
}

pub(crate) fn profile_avatar_proxy_url(
    headers: &HeaderMap,
    avatar_url: &str,
    role_key: &str,
    principal_ref: &str,
    token: &str,
) -> Option<String> {
    if avatar_url.trim().is_empty()
        || role_key.trim().is_empty()
        || principal_ref.trim().is_empty()
        || token.trim().is_empty()
    {
        return None;
    }
    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    Some(format!(
        "{}://{}/v1/mobile/profile/avatar/view?role={}&ref={}&token={}&v={}",
        request_scheme(headers),
        host,
        urlencoding::encode(role_key.trim()),
        urlencoding::encode(principal_ref.trim()),
        urlencoding::encode(token.trim()),
        urlencoding::encode(avatar_version(avatar_url))
    ))
}

fn avatar_version(avatar_url: &str) -> &str {
    avatar_url
        .trim()
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("avatar")
}

fn request_scheme(headers: &HeaderMap) -> &str {
    if headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| value.eq_ignore_ascii_case("https"))
        .is_some()
    {
        return "https";
    }

    if headers
        .get("cf-visitor")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_ascii_lowercase().contains("\"scheme\":\"https\""))
        .unwrap_or(false)
    {
        return "https";
    }

    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .unwrap_or_default();
    if is_public_host(host) {
        return "https";
    }

    "http"
}

fn is_public_host(host: &str) -> bool {
    let host = host.trim().trim_matches(['[', ']']).to_ascii_lowercase();
    if host.is_empty()
        || host == "localhost"
        || host.starts_with("localhost:")
        || host.starts_with("127.")
        || host.starts_with("10.")
        || host.starts_with("192.168.")
        || (16..=31).any(|octet| host.starts_with(&format!("172.{octet}.")))
    {
        return false;
    }

    host.parse::<std::net::IpAddr>().is_err()
}

fn unauthorized() -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(ErrorResponse {
            error: "unauthorized",
        }),
    )
}

fn method_not_allowed() -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(ErrorResponse {
            error: "method not allowed",
        }),
    )
}

fn bad_request(error: &'static str) -> (StatusCode, Json<ErrorResponse>) {
    (StatusCode::BAD_REQUEST, Json(ErrorResponse { error }))
}

fn parse_json<T: DeserializeOwned>(body: &[u8]) -> Result<T, (StatusCode, Json<ErrorResponse>)> {
    serde_json::from_slice(body).map_err(|_| bad_request("invalid json"))
}

fn login_error(error: AuthError) -> (StatusCode, Json<ErrorResponse>) {
    match error {
        AuthError::InvalidCredentials | AuthError::InvalidRole => (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "invalid credentials",
            }),
        ),
        AuthError::Internal => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "internal error",
            }),
        ),
    }
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: &'static str,
}

#[derive(Serialize)]
pub struct OkResponse {
    pub ok: bool,
}

#[allow(dead_code)]
fn _login_response_contract(_response: LoginResponse) {}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};

    use super::{is_public_host, profile_avatar_proxy_url, with_avatar_proxy};
    use crate::core::auth::models::{Principal, PrincipalRole};

    #[test]
    fn supplier_avatar_uses_token_proxy_url() {
        let mut headers = HeaderMap::new();
        headers.insert("host", HeaderValue::from_static("mobile.test"));

        let principal = with_avatar_proxy(
            &headers,
            Principal {
                role: PrincipalRole::Supplier,
                display_name: "Supplier".to_string(),
                legal_name: "Supplier".to_string(),
                ref_: "SUP-001".to_string(),
                phone: "+998901234567".to_string(),
                avatar_url: "http://files.test/files/avatar.png".to_string(),
            },
            "abc token",
        );

        assert_eq!(
            principal.avatar_url,
            "https://mobile.test/v1/mobile/profile/avatar/view?token=abc%20token&v=avatar.png"
        );
    }

    #[test]
    fn local_avatar_uses_http_proxy_url() {
        let mut headers = HeaderMap::new();
        headers.insert("host", HeaderValue::from_static("127.0.0.1:18081"));

        let principal = with_avatar_proxy(
            &headers,
            Principal {
                role: PrincipalRole::Admin,
                display_name: "Admin".to_string(),
                legal_name: "Admin".to_string(),
                ref_: "admin".to_string(),
                phone: "+998901234567".to_string(),
                avatar_url: "local://profile_avatars/admin/admin/avatar.jpg".to_string(),
            },
            "token",
        );

        assert_eq!(
            principal.avatar_url,
            "http://127.0.0.1:18081/v1/mobile/profile/avatar/view?token=token&v=avatar.jpg"
        );
    }

    #[test]
    fn another_profile_avatar_url_contains_vault_identity_and_version() {
        let mut headers = HeaderMap::new();
        headers.insert("host", HeaderValue::from_static("mobile.test"));

        let url = profile_avatar_proxy_url(
            &headers,
            "local://profile_avatars/aparatchi/worker_1/abc123.jpg",
            "aparatchi",
            "worker_1",
            "viewer token",
        )
        .expect("proxy url");

        assert_eq!(
            url,
            "https://mobile.test/v1/mobile/profile/avatar/view?role=aparatchi&ref=worker_1&token=viewer%20token&v=abc123.jpg"
        );
    }

    #[test]
    fn private_172_hosts_are_not_public() {
        assert!(!is_public_host("172.16.0.1:3000"));
        assert!(!is_public_host("172.31.255.255:8080"));
    }
}
