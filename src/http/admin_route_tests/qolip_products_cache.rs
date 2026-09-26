use super::*;
use crate::core::qolip::{MemoryQolipStore, QolipBlock, QolipService};

#[tokio::test]
async fn qolip_catalog_revalidates_data_and_authorization_before_304() {
    let mut state = test_state();
    let store = Arc::new(MemoryQolipStore::new());
    store
        .seed_blocks(vec![QolipBlock {
            name: "A".into(),
            warehouse: "Qolip ombor".into(),
        }])
        .await;
    state.qolip = QolipService::new(store.clone());
    let token = session_for(&state, PrincipalRole::Qolipchi, "catalog-cache").await;
    let forbidden_token = session_for(&state, PrincipalRole::Customer, "customer-cache").await;
    let router = build_router(state);
    let created = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/product-specs",
            &token,
            r#"{"warehouse":"Qolip ombor","item_code":"ITEM","item_name":"Product",
             "item_group":"Tayyor mahsulot","qolip_code":"Q-CACHE","size":40}"#,
        ))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::OK);
    let path = "/v1/mobile/qolip/products?with_qolip=true&limit=20000";
    let first = router
        .clone()
        .oneshot(request("GET", path, &token))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(first.headers()[header::CACHE_CONTROL], "private, no-cache");
    assert_eq!(first.headers()[header::VARY], "Authorization");
    assert!(
        first.headers()[header::ACCESS_CONTROL_EXPOSE_HEADERS]
            .to_str()
            .unwrap()
            .contains("etag")
    );
    let etag = first.headers()[header::ETAG].clone();
    assert_eq!(
        json_body(first).await["products"].as_array().unwrap().len(),
        1
    );

    let mut unchanged_request = request("GET", path, &token);
    unchanged_request.headers_mut().insert(
        header::IF_NONE_MATCH,
        format!("\"another-version\", W/{}", etag.to_str().unwrap())
            .parse()
            .unwrap(),
    );
    let unchanged = router.clone().oneshot(unchanged_request).await.unwrap();
    assert_eq!(unchanged.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(unchanged.headers()[header::ETAG], etag);
    assert!(
        axum::body::to_bytes(unchanged.into_body(), usize::MAX)
            .await
            .unwrap()
            .is_empty()
    );

    for (denied_token, expected_status) in [
        ("invalid-token", StatusCode::UNAUTHORIZED),
        (forbidden_token.as_str(), StatusCode::FORBIDDEN),
    ] {
        let mut denied_request = request("GET", path, denied_token);
        denied_request
            .headers_mut()
            .insert(header::IF_NONE_MATCH, etag.clone());
        let denied = router.clone().oneshot(denied_request).await.unwrap();
        assert_eq!(denied.status(), expected_status);
    }

    // Revoking the warehouse must produce a fresh empty catalog, never 304
    // against data that this principal can no longer access.
    store.seed_blocks(Vec::new()).await;
    let mut revoked_request = request("GET", path, &token);
    revoked_request
        .headers_mut()
        .insert(header::IF_NONE_MATCH, etag.clone());
    let revoked = router.clone().oneshot(revoked_request).await.unwrap();
    assert_eq!(revoked.status(), StatusCode::OK);
    assert_ne!(revoked.headers()[header::ETAG], etag);
    assert_eq!(json_body(revoked).await["products"], serde_json::json!([]));

    let options = router
        .oneshot(request("OPTIONS", path, &token))
        .await
        .unwrap();
    assert_eq!(options.status(), StatusCode::NO_CONTENT);
    assert!(
        options.headers()[header::ACCESS_CONTROL_ALLOW_HEADERS]
            .to_str()
            .unwrap()
            .contains("if-none-match")
    );
}
