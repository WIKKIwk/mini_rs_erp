use super::*;

const READS: &[&str] = &[
    "/v1/mobile/qolip/paddons",
    "/v1/mobile/qolip/paddons/detail?code=QP-00001",
    "/v1/mobile/qolip/paddons/molds",
    "/v1/mobile/qolip/archive",
];
const WRITES: &[&str] = &[
    "/v1/mobile/qolip/paddons/qr/print",
    "/v1/mobile/qolip/paddons/action",
    "/v1/mobile/qolip/archive/action",
];

#[tokio::test]
async fn qolip_paddon_and_archive_routes_require_authentication() {
    let router = build_router(test_state());
    for uri in READS {
        let response = router.clone().oneshot(request("GET", uri, "")).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{uri}");
    }
    for uri in WRITES {
        let response = router.clone().oneshot(request_with_body("POST", uri, "", "{}")).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{uri}");
    }
}

#[tokio::test]
async fn qolip_paddon_and_archive_routes_deny_unrelated_roles() {
    let state = test_state();
    let supplier = session(&state, PrincipalRole::Supplier).await;
    let worker = session(&state, PrincipalRole::Aparatchi).await;
    let router = build_router(state);
    for token in [&supplier, &worker] {
        for uri in READS {
            let response = router.clone().oneshot(request("GET", uri, token)).await.unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{uri}");
        }
        for uri in WRITES {
            let response = router.clone().oneshot(request_with_body("POST", uri, token,
                r#"{"action":"restore","request_id":"forbidden","entry_ids":["other-owner"]}"#)).await.unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{uri}");
        }
    }
}

#[tokio::test]
async fn qolip_paddon_and_archive_routes_reject_wrong_methods_and_malformed_json() {
    let state = test_state();
    let admin = session(&state, PrincipalRole::Admin).await;
    let router = build_router(state);
    for uri in READS {
        let response = router.clone().oneshot(request_with_body("DELETE", uri, &admin, "{}")).await.unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED, "{uri}");
    }
    for uri in WRITES {
        let response = router.clone().oneshot(request("GET", uri, &admin)).await.unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED, "{uri}");
        let response = router.clone().oneshot(request_with_body("POST", uri, &admin, "{not-json")).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{uri}");
    }
}
