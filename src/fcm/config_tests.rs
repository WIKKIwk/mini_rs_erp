use super::*;
use crate::{
    core::auth::models::PrincipalRole, core::push::service::PushService,
    store::push_token_store::PushTokenStore,
};
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::State,
    http::{Request, StatusCode, header},
    routing::post,
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tower::ServiceExt;

#[derive(Default)]
struct GoogleMock {
    fail: AtomicBool,
    auth_calls: AtomicUsize,
    messages: Mutex<Vec<Value>>,
}

async fn setup() -> (tempfile::TempDir, Arc<FcmConfigService>, Arc<GoogleMock>) {
    let dir = tempfile::tempdir().unwrap();
    let mock = Arc::new(GoogleMock::default());
    let app = Router::new()
        .route(
            "/oauth",
            post(|State(mock): State<Arc<GoogleMock>>| async move {
                mock.auth_calls.fetch_add(1, Ordering::SeqCst);
                Json(json!({"access_token":"test-access-token", "expires_in":3600}))
            }),
        )
        .route(
            "/send",
            post(
                |State(mock): State<Arc<GoogleMock>>, Json(payload): Json<Value>| async move {
                    mock.messages.lock().await.push(payload);
                    if mock.fail.load(Ordering::SeqCst) {
                        StatusCode::FORBIDDEN
                    } else {
                        StatusCode::OK
                    }
                },
            ),
        )
        .with_state(mock.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let store = Arc::new(PushTokenStore::new(dir.path().join("tokens.json")));
    let mut service = FcmConfigService::new(store, dir.path().join("config.enc"));
    service.cipher_override = Some(Arc::new(CodeCipher::from_key(&[42; 32]).unwrap()));
    service.endpoints = Some((format!("{base}/oauth"), format!("{base}/send")));
    (dir, Arc::new(service), mock)
}

fn account(project: &str) -> Value {
    json!({"type":"service_account", "project_id":project,
        "client_email":format!("push@{project}.iam.gserviceaccount.com"),
        "private_key":crate::fcm_tests::TEST_PRIVATE_KEY,
        "token_uri":"https://oauth2.googleapis.com/token"})
}

fn principal(role: PrincipalRole) -> Principal {
    Principal {
        role,
        ref_: "config-admin".into(),
        display_name: "Admin".into(),
        legal_name: String::new(),
        phone: String::new(),
        avatar_url: String::new(),
    }
}

#[tokio::test]
async fn fcm_config_saves_encrypted_restores_and_reloads_existing_push_workers() {
    let (_dir, service, mock) = setup().await;
    let push = PushService::new(service.store.clone()).with_sender(service.clone());
    let existing_worker = push.clone();
    service
        .store
        .move_token_to_key("qolipchi:q1", "device-token", "ios")
        .await
        .unwrap();
    assert!(
        existing_worker
            .send_to_key("qolipchi:q1", "Hello", "Body", HashMap::new())
            .await
            .is_err()
    );
    let first = service.save(account("project-one")).await.unwrap();
    assert!(first.configured);
    assert!(first.last_verified_at.is_some());
    let encrypted = std::fs::read_to_string(&service.path).unwrap();
    assert!(!encrypted.contains("PRIVATE KEY"));
    assert!(!encrypted.contains("project-one"));
    let public = serde_json::to_string(&first).unwrap();
    assert!(!public.contains("private_key"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&service.path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    assert_eq!(mock.messages.lock().await[0]["validate_only"], true);
    let old_sender = service.sender().unwrap();
    service.save(account("project-two")).await.unwrap();
    assert!(!Arc::ptr_eq(&old_sender, &service.sender().unwrap()));
    existing_worker
        .send_to_key("qolipchi:q1", "Hello", "Body", HashMap::new())
        .await
        .unwrap();
    assert_eq!(mock.auth_calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        mock.messages.lock().await.last().unwrap()["message"]["token"],
        "device-token"
    );
    let mut restored = FcmConfigService::new(service.store.clone(), service.path.clone());
    restored.cipher_override = service.cipher_override.clone();
    restored.endpoints = service.endpoints.clone();
    restored.restore();
    assert_eq!(restored.status().project_id, "project-two");
    restored.check().await.unwrap();
}

#[tokio::test]
async fn fcm_config_rejected_or_unwritable_replacement_preserves_working_credentials() {
    let (dir, service, mock) = setup().await;
    service.save(account("project-one")).await.unwrap();
    let before = std::fs::read(&service.path).unwrap();
    mock.fail.store(true, Ordering::SeqCst);
    assert!(matches!(
        service.save(account("project-two")).await,
        Err(FcmConfigError::GoogleRejected)
    ));
    assert_eq!(service.status().project_id, "project-one");
    assert_eq!(std::fs::read(&service.path).unwrap(), before);
    mock.fail.store(false, Ordering::SeqCst);
    let mut blocked = FcmConfigService::new(service.store.clone(), dir.path().join("folder"));
    std::fs::create_dir(&blocked.path).unwrap();
    blocked.cipher_override = service.cipher_override.clone();
    blocked.endpoints = service.endpoints.clone();
    assert!(matches!(
        blocked.save(account("project-three")).await,
        Err(FcmConfigError::SaveFailed)
    ));
    assert!(!blocked.status().configured);
}

#[tokio::test]
async fn fcm_config_rejects_invalid_keys_and_untrusted_oauth_addresses_before_network() {
    let (_dir, service, mock) = setup().await;
    for (field, value) in [
        ("token_uri", "http://127.0.0.1/secrets"),
        ("private_key", "fcm-device-token"),
        ("project_id", "project/../other"),
        ("type", "authorized_user"),
    ] {
        let mut document = account("project-one");
        document[field] = json!(value);
        assert!(matches!(
            service.save(document).await,
            Err(FcmConfigError::InvalidCredentials)
        ));
    }
    assert_eq!(mock.auth_calls.load(Ordering::SeqCst), 0);
    assert!(!service.path.exists());
}

#[tokio::test]
async fn fcm_config_test_only_targets_callers_registered_device_with_ios_and_android_payload() {
    let (_dir, service, mock) = setup().await;
    service.save(account("project-one")).await.unwrap();
    let admin = principal(PrincipalRole::Admin);
    service
        .store
        .move_token_to_key("admin:config-admin", "own-device", "ios")
        .await
        .unwrap();
    service
        .store
        .move_token_to_key("admin:other", "foreign-device", "android")
        .await
        .unwrap();
    assert!(matches!(
        service.test_device(&admin, "foreign-device").await,
        Err(FcmConfigError::DeviceNotRegistered)
    ));
    service.test_device(&admin, "own-device").await.unwrap();
    let messages = mock.messages.lock().await;
    assert_eq!(messages.len(), 2);
    let test = &messages[1]["message"];
    assert_eq!(test["token"], "own-device");
    assert_eq!(test["data"]["event_type"], "push.configuration.test");
    assert_eq!(test["android"]["notification"]["channel_id"], "accord_chat");
    assert_eq!(test["apns"]["payload"]["aps"]["sound"], "default");
}

#[tokio::test]
async fn fcm_config_http_requires_admin_and_never_returns_private_key() {
    let (_dir, config, _) = setup().await;
    let mut state = crate::app::AppState::new(crate::config::AppConfig {
        bind_addr: "127.0.0.1:8081".parse().expect("addr"),
        default_target_warehouse: String::new(),
        http_timeout: std::time::Duration::from_secs(15),
        session_store_path: "data/mobile_sessions.json".into(),
        profile_store_path: "data/mobile_profiles.json".into(),
        push_token_store_path: "data/mobile_push_tokens.json".into(),
        session_ttl_seconds: Some(3600),
        supplier_prefix: "10".to_string(),
        werka_prefix: "20".to_string(),
        werka_code: "20ABCDEF1234".to_string(),
        werka_name: "Werka".to_string(),
        werka_phone: "+99888862440".to_string(),
        material_taminotchi_code: String::new(),
        material_taminotchi_name: "Material taminotchisi".to_string(),
        material_taminotchi_phone: String::new(),
        admin_phone: "+998880000000".to_string(),
        admin_name: "Admin".to_string(),
        admin_code: "19621978".to_string(),
    });
    state.sessions = crate::core::session::manager::SessionManager::memory(Some(3600));
    state.fcm_config = config;
    let admin = state
        .sessions
        .create(principal(PrincipalRole::Admin))
        .await
        .unwrap();
    let worker = state
        .sessions
        .create(principal(PrincipalRole::Aparatchi))
        .await
        .unwrap();
    let request = |method: &str, suffix: &str, token: &str, data: Value| {
        Request::builder()
            .method(method)
            .uri(format!("/v1/mobile/admin/push-config{suffix}"))
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(data.to_string()))
            .unwrap()
    };
    for (method, suffix) in [
        ("GET", ""),
        ("PUT", ""),
        ("POST", "/check"),
        ("POST", "/test"),
        ("PUT", "/mobile"),
    ] {
        let response = crate::http::router::build_router(state.clone())
            .oneshot(request(method, suffix, &worker, json!({})))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    let mismatch =
        json!({"service_account": account("project-one"), "client_project_id":"project-two"});
    let response = crate::http::router::build_router(state.clone())
        .oneshot(request("PUT", "", &admin, mismatch))
        .await
        .unwrap();
    // The old phone project must not prevent migrating the server to a new project.
    assert_eq!(response.status(), StatusCode::OK);
    let response = crate::http::router::build_router(state.clone())
        .oneshot(request(
            "PUT",
            "",
            &admin,
            json!({"service_account":account("project-one")}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = crate::http::router::build_router(state.clone())
        .oneshot(request("GET", "", &admin, json!({})))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
    let public = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(!public.contains("private_key"));
    assert!(!public.contains("PRIVATE KEY"));
    assert!(public.contains("project-one"));
    let client = mobile_config("project-one", "ios");
    let response = crate::http::router::build_router(state.clone())
        .oneshot(request(
            "PUT",
            "/mobile",
            &admin,
            json!({"platform":"ios", "config":client}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    for (token, expected) in [
        (worker.as_str(), StatusCode::OK),
        (admin.as_str(), StatusCode::OK),
        ("invalid", StatusCode::UNAUTHORIZED),
    ] {
        let response = crate::http::router::build_router(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/v1/mobile/push-config")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        if expected == StatusCode::OK {
            let body = to_bytes(response.into_body(), 4096).await.unwrap();
            let json: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(json["ios"]["project_id"], "project-one");
            assert_eq!(json.as_object().unwrap().len(), 2);
            assert!(!String::from_utf8_lossy(&body).contains("private_key"));
            assert!(!String::from_utf8_lossy(&body).contains("client_email"));
        }
    }
}

fn mobile_config(project: &str, platform: &str) -> crate::fcm::mobile_config::FirebaseClientConfig {
    crate::fcm::mobile_config::FirebaseClientConfig {
        project_id: project.into(),
        app_id: format!("1:12345:{platform}:abc123"),
        messaging_sender_id: "12345".into(),
        api_key: "AIzaTestPublicKey".into(),
        application_id: if platform == "ios" {
            "com.example.accordMobileV2.mirsaid.uzkingshark"
        } else {
            "com.example.accord_mobile_v2"
        }
        .into(),
    }
}

#[tokio::test]
async fn fcm_mobile_configs_validate_persist_and_follow_server_project() {
    let (_dir, service, _) = setup().await;
    service.save(account("project-one")).await.unwrap();
    for platform in ["android", "ios"] {
        let config = mobile_config("project-one", platform);
        service.save_mobile_config(platform, config).await.unwrap();
    }
    assert!(service.status().mobile.android.is_some());
    assert!(service.status().mobile.ios.is_some());
    let before = std::fs::read(service.path.with_extension("clients.json")).unwrap();
    assert!(matches!(
        service
            .save_mobile_config("ios", mobile_config("wrong-project", "ios"))
            .await,
        Err(FcmConfigError::ProjectMismatch)
    ));
    let mut bad = mobile_config("project-one", "ios");
    bad.application_id = "other.app".into();
    assert!(matches!(
        service.save_mobile_config("ios", bad).await,
        Err(FcmConfigError::InvalidClientConfig)
    ));
    assert_eq!(
        std::fs::read(service.path.with_extension("clients.json")).unwrap(),
        before
    );
    let mut restored = FcmConfigService::new(service.store.clone(), service.path.clone());
    restored.cipher_override = service.cipher_override.clone();
    restored.endpoints = service.endpoints.clone();
    restored.restore();
    assert!(restored.mobile_configs().ios.is_some());
    assert!(restored.mobile_configs().android.is_some());
    service.save(account("project-two")).await.unwrap();
    assert!(service.mobile_configs().ios.is_none());
    assert!(service.mobile_configs().android.is_none());
}
