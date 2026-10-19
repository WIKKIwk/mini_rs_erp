use super::*;
use crate::core::mobile_release::MobileReleaseStore;

const SETTINGS: &str = "/v1/mobile/admin/app-update/settings";
const METADATA: &str = "/v1/mobile/app-update/android";

async fn publish(directory: &std::path::Path, version: u64, mandatory: bool, minimum: u64) {
    tokio::fs::write(directory.join("accord.apk"), b"apk")
        .await
        .unwrap();
    tokio::fs::write(
        directory.join("android.json"),
        serde_json::to_vec(&serde_json::json!({
            "version_code": version,
            "version_name": format!("1.0.{version}"),
            "minimum_supported_version_code": minimum,
            "mandatory": mandatory,
            "apk_file": "accord.apk",
            "sha256": "a".repeat(64),
            "size_bytes": 3
        }))
        .unwrap(),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn app_update_settings_persist_and_control_public_manifest_across_releases() {
    let directory = tempfile::tempdir().unwrap();
    let mut state = test_state();
    state.mobile_releases = MobileReleaseStore::new(directory.path());
    let token = session(&state, PrincipalRole::Admin).await;
    let response = build_router(state.clone())
        .oneshot(request("GET", SETTINGS, &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["settings"]["mandatory_updates_enabled"],
        false
    );

    // Admin can set the policy before the first APK is published.
    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            SETTINGS,
            &token,
            r#"{"mandatory_updates_enabled":true}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["settings"]["mandatory_updates_enabled"],
        true
    );
    let response = build_router(state.clone())
        .oneshot(request("GET", METADATA, ""))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    publish(directory.path(), 6, false, 0).await;
    // A fresh store simulates a server restart. Re-publishing must not reset the switch.
    state.mobile_releases = MobileReleaseStore::new(directory.path());
    for version in [6, 7] {
        publish(directory.path(), version, false, 0).await;
        let response = build_router(state.clone())
            .oneshot(request("GET", METADATA, ""))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "no-store, max-age=0"
        );
        let metadata = json_body(response).await;
        assert_eq!(metadata["version_code"], version);
        assert_eq!(metadata["mandatory"], true);
    }

    // Turning off also clears a legacy per-release minimum requirement.
    publish(directory.path(), 8, true, 7).await;
    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            SETTINGS,
            &token,
            r#"{"mandatory_updates_enabled":false}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    state.mobile_releases = MobileReleaseStore::new(directory.path());
    let response = build_router(state.clone())
        .oneshot(request("GET", METADATA, ""))
        .await
        .unwrap();
    let metadata = json_body(response).await;
    assert_eq!(metadata["mandatory"], false);
    assert_eq!(metadata["minimum_supported_version_code"], 0);
    let response = build_router(state)
        .oneshot(request("GET", SETTINGS, &token))
        .await
        .unwrap();
    assert_eq!(
        json_body(response).await["settings"]["mandatory_updates_enabled"],
        false
    );
}

#[tokio::test]
async fn app_update_settings_require_admin_and_valid_boolean() {
    let directory = tempfile::tempdir().unwrap();
    let mut state = test_state();
    state.mobile_releases = MobileReleaseStore::new(directory.path());
    let admin = session(&state, PrincipalRole::Admin).await;
    for token in [
        String::new(),
        session(&state, PrincipalRole::Aparatchi).await,
        session(&state, PrincipalRole::Supplier).await,
    ] {
        for method in ["GET", "PUT"] {
            let response = build_router(state.clone())
                .oneshot(request_with_body(
                    method,
                    SETTINGS,
                    &token,
                    r#"{"mandatory_updates_enabled":true}"#,
                ))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                if token.is_empty() {
                    StatusCode::UNAUTHORIZED
                } else {
                    StatusCode::FORBIDDEN
                }
            );
        }
    }
    for body in [
        "{}",
        r#"{"mandatory_updates_enabled":"true"}"#,
        r#"{"mandatory_updates_enabled":true,"unexpected":true}"#,
    ] {
        let response = build_router(state.clone())
            .oneshot(request_with_body("PUT", SETTINGS, &admin, body))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    let response = build_router(state.clone())
        .oneshot(request("POST", SETTINGS, &admin))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert!(!directory.path().join("update-settings.json").exists());
}

#[tokio::test]
async fn app_update_settings_preserve_legacy_policy_until_admin_changes_it() {
    let directory = tempfile::tempdir().unwrap();
    publish(directory.path(), 8, false, 7).await;
    let mut state = test_state();
    state.mobile_releases = MobileReleaseStore::new(directory.path());
    let token = session(&state, PrincipalRole::Admin).await;
    let response = build_router(state.clone())
        .oneshot(request("GET", SETTINGS, &token))
        .await
        .unwrap();
    assert_eq!(
        json_body(response).await["settings"]["mandatory_updates_enabled"],
        true
    );
    let response = build_router(state)
        .oneshot(request("GET", METADATA, ""))
        .await
        .unwrap();
    let metadata = json_body(response).await;
    assert_eq!(metadata["mandatory"], false);
    assert_eq!(metadata["minimum_supported_version_code"], 7);
}

#[tokio::test]
async fn app_update_settings_failures_do_not_report_success_or_optional_updates() {
    let directory = tempfile::tempdir().unwrap();
    publish(directory.path(), 6, false, 0).await;
    tokio::fs::write(directory.path().join("update-settings.json"), b"invalid")
        .await
        .unwrap();
    let mut state = test_state();
    state.mobile_releases = MobileReleaseStore::new(directory.path());
    let token = session(&state, PrincipalRole::Admin).await;
    let response = build_router(state.clone())
        .oneshot(request("GET", SETTINGS, &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let response = build_router(state.clone())
        .oneshot(request("GET", METADATA, ""))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    // A directory in place of the settings file makes the atomic save fail.
    tokio::fs::remove_file(directory.path().join("update-settings.json"))
        .await
        .unwrap();
    tokio::fs::create_dir(directory.path().join("update-settings.json"))
        .await
        .unwrap();
    let response = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            SETTINGS,
            &token,
            r#"{"mandatory_updates_enabled":true}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}
