use super::*;

#[tokio::test]
async fn admin_creates_and_lists_custom_apparatus_collection_without_mutating_apparatus() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let router = build_router(state);

    let before = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/apparatus?limit=500",
            &token,
        ))
        .await
        .expect("apparatus before collection create");
    assert_eq!(before.status(), StatusCode::OK);
    let before_body = json_body(before).await;

    let created = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/apparatus-collections",
            &token,
            r#"{
                "name":" Bosma A liniyasi ",
                "apparatus_ids":[
                    "apparatus:default:bosma_7",
                    "apparatus:default:bosma_8",
                    "apparatus:default:bosma_7"
                ]
            }"#,
        ))
        .await
        .expect("collection create response");
    assert_eq!(created.status(), StatusCode::OK);
    let created_body = json_body(created).await;
    assert_eq!(created_body["name"], "Bosma A liniyasi");
    assert_eq!(created_body["revision"], 1);
    assert_eq!(
        created_body["apparatus_ids"],
        serde_json::json!(["apparatus:default:bosma_7", "apparatus:default:bosma_8"])
    );
    assert!(
        created_body["id"]
            .as_str()
            .expect("collection id")
            .starts_with("apparatus-collection:")
    );

    let listed = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/apparatus-collections",
            &token,
        ))
        .await
        .expect("collection list response");
    assert_eq!(listed.status(), StatusCode::OK);
    assert_eq!(json_body(listed).await, serde_json::json!([created_body]));

    let after = router
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/apparatus?limit=500",
            &token,
        ))
        .await
        .expect("apparatus after collection create");
    assert_eq!(after.status(), StatusCode::OK);
    assert_eq!(json_body(after).await, before_body);
}

#[tokio::test]
async fn collection_rejects_unknown_canonical_apparatus_and_duplicate_name() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let router = build_router(state);

    let unknown = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/apparatus-collections",
            &token,
            r#"{
                "name":"Noma'lum",
                "apparatus_ids":["apparatus:custom:does-not-exist"]
            }"#,
        ))
        .await
        .expect("unknown apparatus response");
    assert_eq!(unknown.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(unknown).await["error"], "apparatus id is invalid");

    let first = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/apparatus-collections",
            &token,
            r#"{"name":"Bosma liniyasi","apparatus_ids":[]}"#,
        ))
        .await
        .expect("first collection response");
    assert_eq!(first.status(), StatusCode::OK);

    let duplicate = router
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/apparatus-collections",
            &token,
            r#"{"name":"BOSMA LINIYASI","apparatus_ids":[]}"#,
        ))
        .await
        .expect("duplicate collection response");
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn collection_update_and_delete_require_current_revision() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let router = build_router(state);

    let created = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/apparatus-collections",
            &token,
            r#"{
                "name":"Bosma liniyasi",
                "apparatus_ids":["apparatus:default:bosma_7"]
            }"#,
        ))
        .await
        .expect("collection create response");
    assert_eq!(created.status(), StatusCode::OK);
    let created_body = json_body(created).await;
    let id = created_body["id"].as_str().expect("collection id");
    let detail_uri = format!("/v1/mobile/admin/apparatus-collections/{id}");

    let updated = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            &detail_uri,
            &token,
            r#"{
                "expected_revision":1,
                "name":"Aralash liniya",
                "apparatus_ids":[
                    "apparatus:default:bosma_8",
                    "apparatus:default:bosma_7"
                ]
            }"#,
        ))
        .await
        .expect("collection update response");
    assert_eq!(updated.status(), StatusCode::OK);
    let updated_body = json_body(updated).await;
    assert_eq!(updated_body["name"], "Aralash liniya");
    assert_eq!(updated_body["revision"], 2);
    assert_eq!(
        updated_body["apparatus_ids"],
        serde_json::json!(["apparatus:default:bosma_7", "apparatus:default:bosma_8"])
    );

    let stale_delete = router
        .clone()
        .oneshot(request_with_body(
            "DELETE",
            &detail_uri,
            &token,
            r#"{"expected_revision":1}"#,
        ))
        .await
        .expect("stale delete response");
    assert_eq!(stale_delete.status(), StatusCode::CONFLICT);

    let deleted = router
        .clone()
        .oneshot(request_with_body(
            "DELETE",
            &detail_uri,
            &token,
            r#"{"expected_revision":2}"#,
        ))
        .await
        .expect("collection delete response");
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);

    let listed = router
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/apparatus-collections",
            &token,
        ))
        .await
        .expect("collection list response");
    assert_eq!(listed.status(), StatusCode::OK);
    assert_eq!(json_body(listed).await, serde_json::json!([]));
}
