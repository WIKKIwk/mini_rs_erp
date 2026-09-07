use super::*;

const RECEIVE: &str = "/v1/mobile/werka/paddons/receive";
const PREVIEW: &str = "/v1/mobile/werka/paddons/preview?code=00001";
const BODY: &str = r#"{"code":"00001","warehouse":"WH-2","expected_batch_ids":["roll-1"],"snapshot_token":"token"}"#;

#[tokio::test]
async fn werka_paddon_receipt_requires_auth_and_exact_role() {
    let state = test_state();
    for token in [
        String::new(),
        session(&state, PrincipalRole::Admin).await,
        session(&state, PrincipalRole::Aparatchi).await,
    ] {
        for req in [
            request("GET", PREVIEW, &token),
            request_with_body("POST", RECEIVE, &token, BODY),
        ] {
            let response = build_router(state.clone()).oneshot(req).await.unwrap();
            assert!(
                matches!(
                    response.status(),
                    StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
                ),
                "{}",
                response.status()
            );
        }
    }
}

#[tokio::test]
async fn werka_paddon_receipt_rejects_unassigned_warehouse_before_lookup() {
    let state = test_state();
    let token = session_for(&state, PrincipalRole::Werka, "paddon-keeper").await;
    let response = build_router(state.clone())
        .oneshot(request("GET", PREVIEW, &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assign_warehouse_to_principal(&state, PrincipalRole::Werka, "paddon-keeper", "WH-1").await;
    let response = build_router(state.clone())
        .oneshot(request_with_body("POST", RECEIVE, &token, BODY))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    // Assigned Werka passes authorization and reaches pallet lookup.
    let response = build_router(state)
        .oneshot(request("GET", PREVIEW, &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
