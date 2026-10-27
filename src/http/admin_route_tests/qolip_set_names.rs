use super::*;
use crate::core::qolip::{
    MemoryQolipStore, QolipBlock, QolipService, QolipSetRename, QolipStorePort,
};

#[tokio::test]
async fn qolip_set_names_persist_and_enforce_unique_names_and_access() {
    let store = Arc::new(MemoryQolipStore::new());
    store
        .seed_blocks(vec![QolipBlock {
            name: "A".into(),
            warehouse: "Qolip ombor".into(),
        }])
        .await;
    let mut state = test_state();
    state.qolip = QolipService::new(store.clone());
    let clerk = session_for(&state, PrincipalRole::Qolipchi, "name-clerk").await;
    let worker = session_for(&state, PrincipalRole::Aparatchi, "name-worker").await;
    let router = build_router(state);
    let mut sets = Vec::new();
    for code in ["NAME-A", "NAME-B"] {
        let response = router
            .clone()
            .oneshot(request_with_body(
                "POST",
                "/v1/mobile/qolip/sets/create",
                &clerk,
                &serde_json::json!({"warehouse":"Qolip ombor", "members":[{
                    "qolip_code":code, "size":420, "color":"BLACK",
                }]})
                .to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let set = json_body(response).await["set"].clone();
        assert_eq!(set["name"], "");
        sets.push(set);
    }
    let body =
        serde_json::json!({"set_id":sets[0]["set_id"], "name":"  Hot   lunch  "}).to_string();
    for (token, status) in [
        (&worker, StatusCode::FORBIDDEN),
        (&String::new(), StatusCode::UNAUTHORIZED),
    ] {
        let response = router
            .clone()
            .oneshot(request_with_body(
                "POST",
                "/v1/mobile/qolip/sets/rename",
                token,
                &body,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), status);
    }
    let response = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/sets/rename",
            &clerk,
            &body,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let saved = json_body(response).await["set"].clone();
    assert_eq!(saved["name"], "Hot lunch");
    assert_eq!(saved["fingerprint"], sets[0]["fingerprint"]);
    assert_eq!(saved["members"], sets[0]["members"]);

    for (name, status, error) in [
        (
            "HOT LUNCH".to_string(),
            StatusCode::CONFLICT,
            "qolip_set_name_conflict",
        ),
        (
            "  ".to_string(),
            StatusCode::BAD_REQUEST,
            "qolip_set_name_invalid",
        ),
        (
            "a".repeat(161),
            StatusCode::BAD_REQUEST,
            "qolip_set_name_invalid",
        ),
    ] {
        let response = router
            .clone()
            .oneshot(request_with_body(
                "POST",
                "/v1/mobile/qolip/sets/rename",
                &clerk,
                &serde_json::json!({"set_id":sets[1]["set_id"], "name":name}).to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert_eq!(json_body(response).await["error"], error);
    }
    let response = router
        .clone()
        .oneshot(request_with_body(
            "GET",
            "/v1/mobile/qolip/sets",
            &clerk,
            "",
        ))
        .await
        .unwrap();
    let listed = json_body(response).await;
    assert!(
        listed["sets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|set| set["set_id"] == saved["set_id"] && set["name"] == "Hot lunch")
    );

    let attempts = sets
        .iter()
        .map(|set| {
            router.clone().oneshot(request_with_body(
                "POST",
                "/v1/mobile/qolip/sets/rename",
                &clerk,
                &serde_json::json!({"set_id":set["set_id"], "name":"Concurrent"}).to_string(),
            ))
        })
        .collect::<Vec<_>>();
    let mut attempts = attempts.into_iter();
    let (a, b) = tokio::join!(attempts.next().unwrap(), attempts.next().unwrap());
    let statuses = [a.unwrap().status(), b.unwrap().status()];
    assert_eq!(statuses.iter().filter(|&&s| s == StatusCode::OK).count(), 1);
    assert_eq!(
        statuses
            .iter()
            .filter(|&&s| s == StatusCode::CONFLICT)
            .count(),
        1
    );

    let principal = Principal {
        role: PrincipalRole::Qolipchi,
        ref_: "name-clerk".into(),
        display_name: String::new(),
        legal_name: String::new(),
        phone: String::new(),
        avatar_url: String::new(),
    };
    let denied = store
        .rename_set(
            &QolipSetRename {
                set_id: saved["set_id"].as_str().unwrap().into(),
                name: "Outside".into(),
            },
            &principal,
            Some(&["Another warehouse".into()]),
        )
        .await;
    assert!(matches!(
        denied,
        Err(crate::core::qolip::QolipError::AccessDenied)
    ));
}
