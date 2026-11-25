use axum::Router;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use mini_rs_erp::db::postgres_qolip::PostgresQolipStore;
use serde_json::json;

use super::*;

fn target(code: &str) -> String {
    format!("qp1.{}", URL_SAFE_NO_PAD.encode(code.as_bytes()))
}

fn save(
    id: &str,
    token: &str,
    edit: serde_json::Value,
    file: Option<(&str, &[u8])>,
) -> Request<Body> {
    let mut body =
        format!("--fixture\r\nContent-Disposition: form-data; name=\"edit\"\r\n\r\n{edit}\r\n")
            .into_bytes();
    if let Some((name, bytes)) = file {
        body.extend_from_slice(
            format!("--fixture\r\nContent-Disposition: form-data; name=\"attachment\"; filename=\"{name}\"\r\n\r\n")
                .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(b"--fixture--\r\n");
    Request::builder()
        .method("PUT")
        .uri(format!("/v1/mobile/qolip/products/information?id={id}"))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(
            header::CONTENT_TYPE,
            "multipart/form-data; boundary=fixture",
        )
        .body(Body::from(body))
        .unwrap()
}

async fn read(app: &Router, id: &str, token: &str) -> axum::response::Response {
    app.clone()
        .oneshot(request(
            "GET",
            &format!("/v1/mobile/qolip/products/information?id={id}"),
            token,
        ))
        .await
        .unwrap()
}

async fn download(app: &Router, id: &str, token: &str, revision: u32) -> axum::response::Response {
    app.clone()
        .oneshot(request(
            "GET",
            &format!(
                "/v1/mobile/qolip/products/information/attachment?id={id}&revision={revision}"
            ),
            token,
        ))
        .await
        .unwrap()
}

pub(super) async fn verify(pool: &sqlx::PgPool) {
    let (mut state, _files) = test_state();
    state.qolip = QolipService::new(Arc::new(PostgresQolipStore::new(pool.clone())));
    let token = session_for(&state, PrincipalRole::Qolipchi, "owner-a").await;
    let hidden = session_for(&state, PrincipalRole::Qolipchi, "owner-b").await;
    let customer = session_for(&state, PrincipalRole::Customer, "catalog-customer").await;
    let app = build_router(state);
    let product = target("item-000");
    let duplicate = target("item-001");
    let file = ("ko‘k andoza.pdf", b"%PDF-test\0binary" as &[u8]);

    let response = app
        .clone()
        .oneshot(save(
            &product,
            &token,
            json!({"revision": 0, "details": "Qolip tafsiloti"}),
            Some(file),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["information"],
        json!({
            "revision": 1, "details": "Qolip tafsiloti",
            "attachment_name": file.0, "attachment_size": file.1.len()
        })
    );

    // The second product has the same label and shares this physical set.
    let response = read(&app, &duplicate, &token).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["information"]["revision"], 0);
    for (id, has_content) in [(&product, true), (&duplicate, false)] {
        let response = app
            .clone()
            .oneshot(request(
                "GET",
                &format!("/v1/mobile/qolip/products/detail?id={id}"),
                &token,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            json_body(response).await["information_status"]["has_content"],
            has_content
        );
    }

    for (actor, status) in [
        (hidden.as_str(), StatusCode::NOT_FOUND),
        (customer.as_str(), StatusCode::FORBIDDEN),
    ] {
        assert_eq!(read(&app, &product, actor).await.status(), status);
        assert_eq!(download(&app, &product, actor, 1).await.status(), status);
        let response = app
            .clone()
            .oneshot(save(
                &product,
                actor,
                json!({"revision": 1, "details": "Unscoped change"}),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), status);
    }

    let response = app
        .clone()
        .oneshot(save(
            &product,
            &token,
            json!({"revision": 0, "details": "Stale change"}),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let response = app
        .clone()
        .oneshot(save(
            &product,
            &token,
            json!({"revision": 1, "details": "Invalid replacement"}),
            Some(("../file.pdf", b"bad")),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(read(&app, &product, &token).await).await["information"]["details"],
        "Qolip tafsiloti"
    );

    let response = app
        .clone()
        .oneshot(save(
            &product,
            &token,
            json!({"revision": 1, "details": "Yangi tafsilot"}),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["information"]["attachment_name"],
        file.0
    );
    assert_eq!(
        download(&app, &product, &token, 1).await.status(),
        StatusCode::CONFLICT
    );
    let response = download(&app, &product, &token, 2).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "application/octet-stream"
    );
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
    assert_eq!(
        response.headers()[header::CONTENT_DISPOSITION],
        "attachment; filename*=UTF-8''ko%E2%80%98k%20andoza.pdf"
    );
    assert_eq!(
        to_bytes(response.into_body(), 1024).await.unwrap().as_ref(),
        file.1
    );

    let response = app
        .oneshot(save(
            &product,
            &token,
            json!({"revision": 2, "details": "", "remove_attachment": true}),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["information"],
        json!({"revision": 3, "details": "", "attachment_name": null, "attachment_size": null})
    );
}
