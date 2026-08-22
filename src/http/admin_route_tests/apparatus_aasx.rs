use sqlx::PgPool;
use sqlx::postgres::PgConnectOptions;

use super::*;
use crate::core::apparatus_standard::isa95::tests::revision_with;
use crate::core::apparatus_standard::{
    CanonicalApparatusDraft, CanonicalApparatusService, FactoryMapPlacement,
};
use crate::db::postgres::apply_foundation_migration;
use crate::db::postgres_canonical_apparatus::PostgresCanonicalApparatusRepository;
use crate::http::handlers::admin::MAX_AASX_UPLOAD_BYTES;

#[tokio::test]
async fn authenticated_canonical_apparatus_routes_commit_only_through_postgres_service() {
    let database = RouterDatabase::create().await;
    let mut state = test_state();
    state.apparatus = CanonicalApparatusService::new(Arc::new(
        PostgresCanonicalApparatusRepository::new(database.pool.clone()),
    ));
    let token = session(&state, PrincipalRole::Admin).await;
    let router = build_router(state);
    let first_draft = canonical_draft("physical-asset:router-01", "Shared route display");

    let legacy = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/v1/mobile/admin/apparatus",
            &token,
            serde_json::json!({"name":"Legacy apparatus","master":{}}),
            Some("legacy-contract-rejected"),
        ))
        .await
        .unwrap();
    assert_eq!(legacy.status(), StatusCode::BAD_REQUEST);

    let created = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/v1/mobile/admin/apparatus",
            &token,
            serde_json::to_value(&first_draft).unwrap(),
            Some("router-create-01"),
        ))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::OK);
    let created = json_body(created).await;
    let apparatus_id = created["revision"]["apparatus_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(ApparatusId::new(apparatus_id.clone()).is_ok());
    assert_eq!(created["revision"]["revision_metadata"]["revision"], 1);

    let second = router
        .clone()
        .oneshot(json_request(
            "POST",
            "/v1/mobile/admin/apparatus",
            &token,
            serde_json::to_value(canonical_draft(
                "physical-asset:router-02",
                "Shared route display",
            ))
            .unwrap(),
            Some("router-create-02"),
        ))
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::OK);
    let second = json_body(second).await;
    assert_ne!(second["revision"]["apparatus_id"], apparatus_id);

    let list = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/apparatus?limit=10",
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    assert_eq!(json_body(list).await.as_array().unwrap().len(), 2);

    let mut updated_draft = first_draft;
    updated_draft.display.description = "complete PUT revision".to_string();
    let updated = router
        .clone()
        .oneshot(json_request(
            "PUT",
            &format!("/v1/mobile/admin/apparatus/{apparatus_id}"),
            &token,
            serde_json::json!({
                "expected_revision": 1,
                "draft": updated_draft,
            }),
            Some("router-update-01"),
        ))
        .await
        .unwrap();
    assert_eq!(updated.status(), StatusCode::OK);
    assert_eq!(
        json_body(updated).await["revision"]["revision_metadata"]["revision"],
        2
    );

    let exported = router
        .clone()
        .oneshot(request(
            "GET",
            &format!("/v1/mobile/admin/apparatus/{apparatus_id}/aasx"),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(exported.status(), StatusCode::OK);
    assert_eq!(
        exported.headers()[header::CONTENT_TYPE],
        crate::core::apparatus_standard::AASX_MEDIA_TYPE
    );
    let exported_bytes = to_bytes(exported.into_body(), MAX_AASX_UPLOAD_BYTES)
        .await
        .unwrap()
        .to_vec();
    assert_eq!(
        exported_bytes,
        stored_aasx_bytes(&database.pool, &apparatus_id).await
    );

    let imported = router
        .clone()
        .oneshot(aasx_request(
            &format!("/v1/mobile/admin/apparatus/{apparatus_id}/aasx"),
            &token,
            &exported_bytes,
            "2",
            "router-aasx-01",
        ))
        .await
        .unwrap();
    assert_eq!(imported.status(), StatusCode::OK);
    assert_eq!(
        json_body(imported).await["revision"]["revision_metadata"]["revision"],
        3
    );

    let stale_import = router
        .clone()
        .oneshot(aasx_request(
            &format!("/v1/mobile/admin/apparatus/{apparatus_id}/aasx"),
            &token,
            &exported_bytes,
            "2",
            "router-aasx-stale",
        ))
        .await
        .unwrap();
    assert_eq!(stale_import.status(), StatusCode::CONFLICT);

    let patch_a = patch_request(&apparatus_id, &token, "CAS route A", "router-cas-a", 3);
    let patch_b = patch_request(&apparatus_id, &token, "CAS route B", "router-cas-b", 3);
    let (first, second) = tokio::join!(
        router.clone().oneshot(patch_a),
        router.clone().oneshot(patch_b)
    );
    let statuses = [first.unwrap().status(), second.unwrap().status()];
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == StatusCode::OK)
            .count(),
        1
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == StatusCode::CONFLICT)
            .count(),
        1
    );

    let retired = router
        .clone()
        .oneshot(json_request(
            "DELETE",
            &format!("/v1/mobile/admin/apparatus/{apparatus_id}"),
            &token,
            serde_json::json!({
                "expected_revision": 4,
                "retirement_reason": "router-retirement"
            }),
            Some("router-retire-01"),
        ))
        .await
        .unwrap();
    assert_eq!(retired.status(), StatusCode::OK);
    assert_eq!(
        json_body(retired).await["revision"]["revision_metadata"]["revision"],
        5
    );

    let (revision_count, outbox_count): (i64, i64) = sqlx::query_as(
        "SELECT
             (SELECT COUNT(*) FROM mini_canonical_apparatus_revisions
               WHERE apparatus_id = $1),
             (SELECT COUNT(*) FROM mini_canonical_apparatus_change_outbox
               WHERE apparatus_id = $1)",
    )
    .bind(&apparatus_id)
    .fetch_one(&database.pool)
    .await
    .unwrap();
    assert_eq!((revision_count, outbox_count), (5, 5));

    let options = router
        .clone()
        .oneshot(request("GET", "/v1/mobile/admin/apparatus/options", &token))
        .await
        .unwrap();
    assert_eq!(options.status(), StatusCode::OK);
    let options = json_body(options).await;
    assert_eq!(options["contract"], "canonical_apparatus_revision");
    assert!(options.get("apparatus").is_none());
    assert!(!options.to_string().contains("default_apparatus"));

    let oversized = router
        .oneshot(binary_request(
            &format!("/v1/mobile/admin/apparatus/{apparatus_id}/aasx"),
            &token,
            vec![0; MAX_AASX_UPLOAD_BYTES + 1],
            "5",
            "router-oversized",
        ))
        .await
        .unwrap();
    assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
    database.close().await;
}

fn canonical_draft(physical_asset_id: &str, display_name: &str) -> CanonicalApparatusDraft {
    let mut draft = revision_with(
        "apparatus:test:router-draft",
        physical_asset_id,
        display_name,
    )
    .to_draft();
    draft.placement = Some(FactoryMapPlacement {
        factory_map_object_id: format!("factory-map-object:{physical_asset_id}"),
    });
    draft
}

fn patch_request(
    apparatus_id: &str,
    token: &str,
    display_name: &str,
    key: &str,
    expected_revision: u64,
) -> Request<Body> {
    json_request(
        "PATCH",
        &format!("/v1/mobile/admin/apparatus/{apparatus_id}"),
        token,
        serde_json::json!({
            "expected_revision": expected_revision,
            "patch": {
                "display": {
                    "display_name": display_name,
                    "description": "parallel CAS contender",
                    "catalog_order": 1
                }
            }
        }),
        Some(key),
    )
}

fn json_request(
    method: &str,
    uri: &str,
    token: &str,
    body: serde_json::Value,
    idempotency_key: Option<&str>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(key) = idempotency_key {
        builder = builder.header("idempotency-key", key);
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

fn aasx_request(
    uri: &str,
    token: &str,
    body: &[u8],
    expected_revision: &str,
    idempotency_key: &str,
) -> Request<Body> {
    binary_request(
        uri,
        token,
        body.to_vec(),
        expected_revision,
        idempotency_key,
    )
}

fn binary_request(
    uri: &str,
    token: &str,
    body: Vec<u8>,
    expected_revision: &str,
    idempotency_key: &str,
) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(
            header::CONTENT_TYPE,
            crate::core::apparatus_standard::AASX_MEDIA_TYPE,
        )
        .header(header::IF_MATCH, expected_revision)
        .header("idempotency-key", idempotency_key)
        .body(Body::from(body))
        .unwrap()
}

async fn stored_aasx_bytes(pool: &PgPool, apparatus_id: &str) -> Vec<u8> {
    sqlx::query_scalar(
        "SELECT revision.aasx_package
         FROM mini_canonical_apparatus_heads head
         JOIN mini_canonical_apparatus_revisions revision
           ON revision.apparatus_id = head.apparatus_id
          AND revision.revision = head.current_revision
         WHERE head.apparatus_id = $1",
    )
    .bind(apparatus_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

struct RouterDatabase {
    admin_url: String,
    name: String,
    pool: PgPool,
}

impl RouterDatabase {
    async fn create() -> Self {
        let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
        let name = format!("mini_rs_erp_test_apparatus_router_{}", std::process::id());
        let admin_pool = PgPool::connect(&admin_url).await.unwrap();
        sqlx::query(&format!(r#"DROP DATABASE IF EXISTS "{name}" WITH (FORCE)"#))
            .execute(&admin_pool)
            .await
            .unwrap();
        sqlx::query(&format!(r#"CREATE DATABASE "{name}""#))
            .execute(&admin_pool)
            .await
            .unwrap();
        admin_pool.close().await;
        let options = admin_url
            .parse::<PgConnectOptions>()
            .unwrap()
            .database(&name);
        let pool = PgPool::connect_with(options).await.unwrap();
        apply_foundation_migration(&pool).await.unwrap();
        Self {
            admin_url,
            name,
            pool,
        }
    }

    async fn close(self) {
        self.pool.close().await;
        let admin_pool = PgPool::connect(&self.admin_url).await.unwrap();
        sqlx::query(&format!(
            r#"DROP DATABASE IF EXISTS "{}" WITH (FORCE)"#,
            self.name
        ))
        .execute(&admin_pool)
        .await
        .unwrap();
        admin_pool.close().await;
    }
}
