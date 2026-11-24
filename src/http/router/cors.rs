use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, Method, Request, Response, StatusCode, header};
use axum::middleware::Next;

#[derive(Clone)]
pub(super) struct AllowedOrigins(Arc<[HeaderValue]>);

impl AllowedOrigins {
    pub(super) fn from_config(raw: &str) -> Self {
        let origins = raw.split(',').map(str::trim).filter_map(|origin| {
            let uri: axum::http::Uri = origin.parse().ok()?;
            let authority = uri.authority()?;
            let scheme = uri.scheme_str()?;
            if !matches!(scheme, "https" | "http") || authority.as_str().contains(['@', '*'])
                || origin != format!("{scheme}://{authority}")
            {
                return None;
            }
            HeaderValue::from_str(origin).ok()
        }).collect();
        Self(origins)
    }
}

pub(super) async fn cors_headers(
    State(allowed): State<AllowedOrigins>,
    req: Request<Body>,
    next: Next,
) -> Response<Body> {
    let mut origins = req.headers().get_all(header::ORIGIN).iter();
    let origin = origins.next().filter(|value| allowed.0.contains(value)).cloned();
    let origin = origin.filter(|_| origins.next().is_none());
    let mut response = if req.method() == Method::OPTIONS {
        let mut response = Response::new(Body::empty());
        *response.status_mut() = StatusCode::NO_CONTENT;
        response
    } else {
        next.run(req).await
    };
    let headers = response.headers_mut();
    headers.append(header::VARY, HeaderValue::from_static("Origin"));
    if let Some(origin) = origin {
        insert_cors_headers(headers, origin);
    }
    response
}

fn insert_cors_headers(headers: &mut HeaderMap, origin: HeaderValue) {
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET,POST,PUT,PATCH,DELETE,OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static(
            "authorization,content-length,content-range,content-type,idempotency-key,x-file-name,range,if-none-match,prefer",
        ),
    );
    headers.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static(
            "accept-ranges,content-length,content-range,content-type,location,etag,preference-applied,x-production-epoch,x-production-revision,x-preparation-scope,x-preparation-version",
        ),
    );
    headers.insert(
        header::ACCESS_CONTROL_MAX_AGE,
        HeaderValue::from_static("86400"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, middleware, routing::post};
    use tower::ServiceExt;

    #[tokio::test]
    async fn only_configured_origins_receive_permission_on_actual_and_preflight_responses() {
        let router = Router::new()
            .route("/sequence", post(|| async { StatusCode::OK }))
            .layer(middleware::from_fn_with_state(
                AllowedOrigins::from_config("*, null, https://good.example:8443"), cors_headers,
            ));
        for origin in [None, Some("null"), Some("https://good.example"), Some("https://good.example:8443")] {
            for method in [Method::POST, Method::OPTIONS] {
                let mut request = Request::builder().uri("/sequence").method(method);
                if let Some(value) = origin {
                    request = request.header(header::ORIGIN, value);
                }
                let res = router.clone().oneshot(request.body(Body::empty()).unwrap()).await.unwrap();
                assert_eq!(res.headers()[header::VARY], "Origin");
                assert_eq!(
                    res.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).and_then(|v| v.to_str().ok()),
                    origin.filter(|value| *value == "https://good.example:8443")
                );
            }
        }
    }

    #[tokio::test]
    async fn browser_minimal_ack_preflight_allows_prefer_and_exposes_confirmation() {
        let router = Router::new()
            .route("/sequence", post(|| async { StatusCode::OK }))
            .layer(middleware::from_fn_with_state(
                AllowedOrigins::from_config("https://accord.example"), cors_headers,
            ));
        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/sequence")
                    .header(header::ORIGIN, "https://accord.example")
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                    .header(
                        header::ACCESS_CONTROL_REQUEST_HEADERS,
                        "authorization,content-type,prefer",
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "https://accord.example");
        for (header, token) in [
            (header::ACCESS_CONTROL_ALLOW_HEADERS, "prefer"),
            (header::ACCESS_CONTROL_ALLOW_HEADERS, "if-none-match"),
            (header::ACCESS_CONTROL_EXPOSE_HEADERS, "etag"),
            (header::ACCESS_CONTROL_EXPOSE_HEADERS, "preference-applied"),
            (header::ACCESS_CONTROL_EXPOSE_HEADERS, "x-production-epoch"),
            (
                header::ACCESS_CONTROL_EXPOSE_HEADERS,
                "x-production-revision",
            ),
            (header::ACCESS_CONTROL_EXPOSE_HEADERS, "x-preparation-scope"),
            (
                header::ACCESS_CONTROL_EXPOSE_HEADERS,
                "x-preparation-version",
            ),
        ] {
            assert!(
                response.headers()[header]
                    .to_str()
                    .unwrap()
                    .split(',')
                    .any(|value| value.trim().eq_ignore_ascii_case(token))
            );
        }
    }
}
