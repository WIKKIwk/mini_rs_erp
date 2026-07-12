use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use image::codecs::png::PngEncoder;
use image::{ColorType, GenericImageView, ImageEncoder, Rgba, RgbaImage};
use tower::ServiceExt;

use super::router::build_router;
use crate::app::AppState;
use crate::config::AppConfig;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::authz::RoleAssignmentUpsert;
use crate::core::profile::ports::{
    CustomerProfileRecord, DownloadedFile, ProfileAvatarStorage, ProfileLookup, ProfilePortError,
    ProfilePrefs, ProfileStoreError, ProfileStorePort, StoredProfileAvatar, SupplierProfileRecord,
};
use crate::core::profile::service::ProfileService;
use crate::core::session::manager::SessionManager;
use crate::core::warehouses::{WarehouseAssignmentUpsert, WarehouseUpsert};
use crate::store::profile_avatar_local::LocalProfileAvatarStorage;

fn test_state() -> AppState {
    let mut state = AppState::new(AppConfig {
        bind_addr: "127.0.0.1:8081".parse().expect("addr"),
        default_target_warehouse: String::new(),
        http_timeout: std::time::Duration::from_secs(15),
        session_store_path: "data/mobile_sessions.json".into(),
        profile_store_path: unique_profile_store_path(),
        push_token_store_path: "data/mobile_push_tokens.json".into(),
        session_ttl_seconds: Some(30 * 24 * 60 * 60),
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
    state.sessions = SessionManager::memory(Some(30 * 24 * 60 * 60));
    state
}

#[tokio::test]
async fn profile_avatar_upload_persists_worker_avatar_without_r2() {
    let mut state = test_state();
    state.profiles = ProfileService::new(String::new())
        .with_store(Arc::new(FakeProfileStore::default()))
        .with_avatar_storage(Arc::new(LocalProfileAvatarStorage::new(
            unique_profile_avatar_store_dir(),
        )));
    let token = state
        .sessions
        .create(Principal {
            role: PrincipalRole::Werka,
            display_name: "Werka".to_string(),
            legal_name: "Werka".to_string(),
            ref_: "werka_1".to_string(),
            phone: "+998901234567".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("session");
    let boundary = "BOUNDARY";

    let app = build_router(state);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/profile/avatar")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::HOST, "mobile.test")
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(multipart_avatar_body(boundary)))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert!(
        value["avatar_url"]
            .as_str()
            .unwrap_or_default()
            .starts_with("https://mobile.test/v1/mobile/profile/avatar/view?token=")
    );

    let view = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/mobile/profile/avatar/view?token={token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(view.status(), StatusCode::OK);
    assert_eq!(
        view.headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("image/jpeg")
    );
    let bytes = to_bytes(view.into_body(), usize::MAX).await.expect("body");
    let decoded = image::load_from_memory(&bytes).expect("canonical avatar");
    assert_eq!(decoded.dimensions(), (1000, 500));
}

#[tokio::test]
async fn authenticated_user_can_view_another_profile_avatar_by_vault_identity() {
    let mut state = test_state();
    let store = Arc::new(FakeProfileStore::default());
    state.profiles = ProfileService::new(String::new())
        .with_store(store)
        .with_avatar_storage(Arc::new(LocalProfileAvatarStorage::new(
            unique_profile_avatar_store_dir(),
        )));
    state
        .profiles
        .upload_avatar(
            Principal {
                role: PrincipalRole::Aparatchi,
                display_name: "Worker".to_string(),
                legal_name: "Worker".to_string(),
                ref_: "worker_001".to_string(),
                phone: "+998901112233".to_string(),
                avatar_url: String::new(),
            },
            "avatar.png",
            "image/png",
            test_png(160, 80),
        )
        .await
        .expect("store worker avatar");
    let viewer_token = state
        .sessions
        .create(Principal {
            role: PrincipalRole::Customer,
            display_name: "Viewer".to_string(),
            legal_name: "Viewer".to_string(),
            ref_: "customer_001".to_string(),
            phone: "+998909998877".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("viewer session");

    let response = build_router(state)
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/v1/mobile/profile/avatar/view?role=worker&ref=worker_001&token={viewer_token}"
                ))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("image/jpeg")
    );
}

#[tokio::test]
async fn profile_get_requires_auth_like_go() {
    let app = build_router(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/profile")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(json_body(response).await["error"], "unauthorized");
}

#[tokio::test]
async fn profile_put_updates_nickname_and_session_like_go() {
    let state = test_state();
    let token = supplier_session(&state).await;
    let app = build_router(state);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/v1/mobile/profile")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"nickname":"Alias"}"#))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["display_name"], "Alias");

    let me = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/me")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(json_body(me).await["display_name"], "Alias");
}

#[tokio::test]
async fn profile_get_returns_material_scope_and_capabilities() {
    let state = test_state();
    state
        .admin
        .upsert_role_assignment(RoleAssignmentUpsert {
            principal_role: PrincipalRole::MaterialTaminotchi,
            principal_ref: "material_taminotchi".to_string(),
            role_id: "material_taminotchi".to_string(),
            assigned_apparatus: Vec::new(),
            assigned_item_groups: vec!["Kraska".to_string(), "Kley".to_string()],
        })
        .await
        .expect("material role assignment");
    state
        .warehouses
        .upsert_warehouse(WarehouseUpsert {
            warehouse: "Kalidor".to_string(),
            company: "Company".to_string(),
            is_group: false,
            parent_warehouse: String::new(),
        })
        .await
        .expect("warehouse");
    state
        .warehouses
        .assign_warehouse(WarehouseAssignmentUpsert {
            warehouse: "Kalidor".to_string(),
            principal_role: PrincipalRole::MaterialTaminotchi,
            principal_ref: "material_taminotchi".to_string(),
            display_name: "Material taminotchisi".to_string(),
        })
        .await
        .expect("warehouse assignment");
    let token = state
        .sessions
        .create(Principal {
            role: PrincipalRole::MaterialTaminotchi,
            display_name: "Material taminotchisi".to_string(),
            legal_name: "Material taminotchisi".to_string(),
            ref_: "material_taminotchi".to_string(),
            phone: "+998901112233".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("session");

    let response = build_router(state)
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/profile")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["role"], "material_taminotchi");
    assert_eq!(
        value["assigned_item_groups"],
        serde_json::json!(["Kley", "Kraska"])
    );
    assert_eq!(
        value["assigned_warehouses"],
        serde_json::json!(["Kalidor"])
    );
    assert!(
        value["capabilities"]
            .as_array()
            .expect("capabilities")
            .iter()
            .any(|capability| capability == "gscale.print")
    );
    assert!(
        value["capabilities"]
            .as_array()
            .expect("capabilities")
            .iter()
            .any(|capability| capability == "raw_material.assign")
    );
}

#[tokio::test]
async fn profile_rejects_wrong_method_like_go() {
    let state = test_state();
    let token = supplier_session(&state).await;

    let response = build_router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/profile")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(json_body(response).await["error"], "method not allowed");
}

#[tokio::test]
async fn profile_avatar_rejects_non_post_like_go() {
    let state = test_state();
    let token = supplier_session(&state).await;

    let response = build_router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/mobile/profile/avatar")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(json_body(response).await["error"], "method not allowed");
}

#[tokio::test]
async fn profile_avatar_requires_multipart_like_go() {
    let state = test_state();
    let token = supplier_session(&state).await;

    let response = build_router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/profile/avatar")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from("not multipart"))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "invalid multipart");
}

#[tokio::test]
async fn profile_avatar_upload_returns_proxied_supplier_avatar_like_go() {
    let mut state = test_state();
    state.profiles = ProfileService::new("http://files.test".to_string())
        .with_profile_lookup(Arc::new(FakeLookup));
    let token = supplier_session(&state).await;
    let boundary = "BOUNDARY";
    let body = multipart_avatar_body(boundary);

    let response = build_router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/profile/avatar")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::HOST, "mobile.test")
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["avatar_url"],
        format!("https://mobile.test/v1/mobile/profile/avatar/view?token={token}&v=uploaded.png")
    );
}

#[tokio::test]
async fn profile_avatar_upload_returns_worker_storage_avatar() {
    let mut state = test_state();
    state.profiles = ProfileService::new(String::new())
        .with_store(Arc::new(FakeProfileStore::default()))
        .with_avatar_storage(Arc::new(FakeAvatarStorage));
    let token = state
        .sessions
        .create(Principal {
            role: PrincipalRole::Werka,
            display_name: "Werka".to_string(),
            legal_name: "Werka".to_string(),
            ref_: "werka_1".to_string(),
            phone: "+998901234567".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("session");
    let boundary = "BOUNDARY";
    let body = multipart_avatar_body(boundary);

    let response = build_router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/profile/avatar")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["avatar_url"],
        "https://cdn.test/profile_avatars/werka/werka_1/avatar.jpg"
    );
}

#[tokio::test]
async fn avatar_view_requires_auth() {
    let app = build_router(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/profile/avatar/view")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn avatar_view_returns_not_found_without_uploaded_avatar() {
    let state = test_state();
    let token = state
        .sessions
        .create(Principal {
            role: PrincipalRole::Customer,
            display_name: "Customer".to_string(),
            legal_name: "Customer".to_string(),
            ref_: "CUST-001".to_string(),
            phone: "+998901234567".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("session");
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/profile/avatar/view")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn avatar_view_accepts_token_query_with_any_method_like_go() {
    let mut state = test_state();
    state.profiles = ProfileService::new("http://files.test".to_string())
        .with_profile_lookup(Arc::new(FakeLookup));
    let token = state
        .sessions
        .create(Principal {
            role: PrincipalRole::Supplier,
            display_name: "Supplier".to_string(),
            legal_name: "Supplier".to_string(),
            ref_: "SUP-001".to_string(),
            phone: "+998901234567".to_string(),
            avatar_url: "http://files.test/files/uploaded.png".to_string(),
        })
        .await
        .expect("session");

    let response = build_router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/mobile/profile/avatar/view?token={token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("image/png")
    );
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    assert_eq!(&bytes[..], b"png");
}

async fn supplier_session(state: &AppState) -> String {
    state
        .sessions
        .create(Principal {
            role: PrincipalRole::Supplier,
            display_name: "Supplier".to_string(),
            legal_name: "Supplier".to_string(),
            ref_: "SUP-001".to_string(),
            phone: "+998901234567".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("session")
}

async fn json_body(response: axum::response::Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json")
}

fn unique_profile_store_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "accord-profile-route-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ))
}

fn unique_profile_avatar_store_dir() -> PathBuf {
    std::env::temp_dir().join(format!(
        "accord-profile-avatars-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ))
}

fn multipart_avatar_body(boundary: &str) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"avatar\"; filename=\"avatar.png\"\r\n",
    );
    body.extend_from_slice(b"Content-Type: image/png\r\n\r\n");
    body.extend_from_slice(&test_png(1600, 800));
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

fn test_png(width: u32, height: u32) -> Vec<u8> {
    let image = RgbaImage::from_pixel(width, height, Rgba([120, 80, 40, 255]));
    let mut bytes = Vec::new();
    PngEncoder::new(&mut bytes)
        .write_image(image.as_raw(), width, height, ColorType::Rgba8.into())
        .expect("encode png");
    bytes
}

struct FakeLookup;

#[async_trait]
impl ProfileLookup for FakeLookup {
    async fn get_supplier_profile(
        &self,
        _id: &str,
    ) -> Result<SupplierProfileRecord, ProfilePortError> {
        Ok(SupplierProfileRecord {
            phone: "+998901234567".to_string(),
            image: String::new(),
        })
    }

    async fn get_customer_profile(
        &self,
        _id: &str,
    ) -> Result<CustomerProfileRecord, ProfilePortError> {
        Ok(CustomerProfileRecord {
            phone: "+998901234568".to_string(),
        })
    }

    async fn download_file(&self, _file_url: &str) -> Result<DownloadedFile, ProfilePortError> {
        Ok(DownloadedFile {
            content_type: "image/png".to_string(),
            body: b"png".to_vec(),
        })
    }

    async fn upload_supplier_image(
        &self,
        supplier_id: &str,
        filename: &str,
        content_type: &str,
        content: Vec<u8>,
    ) -> Result<String, ProfilePortError> {
        assert_eq!(supplier_id, "SUP-001");
        assert_eq!(filename, "avatar.jpg");
        assert_eq!(content_type, "image/jpeg");
        let decoded = image::load_from_memory(&content).expect("canonical avatar");
        assert_eq!(decoded.dimensions(), (1000, 500));
        Ok("/files/uploaded.png".to_string())
    }
}

#[derive(Default)]
struct FakeProfileStore {
    prefs: std::sync::Mutex<std::collections::HashMap<String, ProfilePrefs>>,
}

#[async_trait]
impl ProfileStorePort for FakeProfileStore {
    async fn get(&self, key: &str) -> Result<ProfilePrefs, ProfileStoreError> {
        Ok(self
            .prefs
            .lock()
            .expect("prefs")
            .get(key)
            .cloned()
            .unwrap_or_default())
    }

    async fn put(&self, key: &str, prefs: ProfilePrefs) -> Result<(), ProfileStoreError> {
        self.prefs
            .lock()
            .expect("prefs")
            .insert(key.to_string(), prefs);
        Ok(())
    }
}

struct FakeAvatarStorage;

#[async_trait]
impl ProfileAvatarStorage for FakeAvatarStorage {
    async fn put_profile_avatar(
        &self,
        role: &str,
        principal_ref: &str,
        filename: &str,
        content_type: &str,
        content: Vec<u8>,
    ) -> Result<StoredProfileAvatar, ProfilePortError> {
        assert_eq!(role, "werka");
        assert_eq!(principal_ref, "werka_1");
        assert_eq!(filename, "avatar.jpg");
        assert_eq!(content_type, "image/jpeg");
        let decoded = image::load_from_memory(&content).expect("canonical avatar");
        assert_eq!(decoded.dimensions(), (1000, 500));
        Ok(StoredProfileAvatar {
            object_key: "profile_avatars/werka/werka_1/avatar.jpg".to_string(),
            public_url: "https://cdn.test/profile_avatars/werka/werka_1/avatar.jpg".to_string(),
        })
    }

    async fn get_profile_avatar(
        &self,
        _object_key: &str,
    ) -> Result<DownloadedFile, ProfilePortError> {
        Ok(DownloadedFile {
            content_type: "image/png".to_string(),
            body: b"pngdata".to_vec(),
        })
    }
}
