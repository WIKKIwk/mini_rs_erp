use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, Method, Request, Response, StatusCode, header};
use axum::middleware::Next;

pub(super) async fn cors_headers(req: Request<Body>, next: Next) -> Response<Body> {
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
    async fn browser_minimal_ack_preflight_allows_prefer_and_exposes_confirmation() {
        let router = Router::new()
            .route("/sequence", post(|| async { StatusCode::OK }))
            .layer(middleware::from_fn(cors_headers));
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
        for (header, token) in [
            (header::ACCESS_CONTROL_ALLOW_HEADERS, "prefer"),
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
