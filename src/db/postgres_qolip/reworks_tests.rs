use super::*;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::qolip::{
    QolipCheckoutCreate, QolipReworkReturn, QolipReworkSend, QolipService, QolipStorePort,
};
use crate::db::postgres::{
    apply_foundation_migration, apply_postgres_migrations_through_version,
    canonical_apparatus_service, postgres_test_database_options,
};
use std::sync::Arc;

struct Fixture {
    admin_url: String,
    database: String,
    admin: PgPool,
    runtime: PgPool,
}

impl Fixture {
    async fn new() -> Self {
        let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
            .expect("explicit isolated test database URL");
        let database = format!("qolip_reworks_{:032x}", rand::random::<u128>());
        let server = PgPool::connect(&admin_url).await.unwrap();
        sqlx::query(&format!("CREATE DATABASE \"{database}\""))
            .execute(&server)
            .await
            .unwrap();
        server.close().await;
        let admin = PgPool::connect_with(postgres_test_database_options(&admin_url, &database))
            .await
            .unwrap();
        apply_postgres_migrations_through_version(&admin, "0121")
            .await
            .unwrap();
        canonical_apparatus_service(admin.clone())
            .bootstrap_factory_defaults()
            .await
            .unwrap();
        apply_foundation_migration(&admin).await.unwrap();
        sqlx::query("INSERT INTO mini_item_groups(name,parent_item_group,is_group) VALUES ('Flexo','All Item Groups',false)").execute(&admin).await.unwrap();
        sqlx::query(
            "INSERT INTO mini_items(code,name,item_group) VALUES ('ITEM-R','Label R','Flexo')",
        )
        .execute(&admin)
        .await
        .unwrap();
        sqlx::query("INSERT INTO mini_warehouses(id,name,parent_warehouse) VALUES ('rw','Qolip ombor',''),('rb','Block A','Qolip ombor')")
            .execute(&admin).await.unwrap();
        sqlx::query("INSERT INTO mini_warehouse_assignments(warehouse,warehouse_name,principal_role,principal_ref,assignment_kind)
            VALUES ('Qolip ombor','Qolip ombor','qolipchi','clerk-1','warehouse')")
            .execute(&admin).await.unwrap();
        let set_id: String = sqlx::query_scalar("SELECT mini_qolip_next_set_id()")
            .fetch_one(&admin)
            .await
            .unwrap();
        for code in ["Q-ONE", "Q-TWO", "CATALOG-ONLY"] {
            sqlx::query("INSERT INTO mini_qolip_product_specs
                (item_code,item_name,item_group,qolip_code,size,created_by_role,created_by_ref,created_by_name,payload_json)
                VALUES ('ITEM-R','Label R','Flexo',$1,420,'qolipchi','clerk-1','Qolipchi',
                    jsonb_build_object('warehouse','Qolip ombor','color','BLACK','qolip_set_id',$2::text))")
                .bind(code).bind(&set_id).execute(&admin).await.unwrap();
        }
        // LEGACY intentionally has no product-spec row: its home receipt remains
        // an authoritative compatibility source during and after peredelka.
        for code in ["Q-ONE", "Q-TWO", "LEGACY"] {
            sqlx::query("INSERT INTO mini_qolip_locations
                (id,block,warehouse,item_code,item_name,qolip_code,size,quantity,row_letter,column_number,location_label,created_by_role,created_by_ref,created_by_name)
                VALUES ($1,'Block A','Qolip ombor','ITEM-R','Label R',$2,420,1,'A',1,'A1','qolipchi','clerk-1','Qolipchi')")
                .bind(format!("home-{code}")).bind(code).execute(&admin).await.unwrap();
        }
        let runtime = PgPool::connect_with(
            postgres_test_database_options(&admin_url, &database).username("mini_rs_erp"),
        )
        .await
        .expect("real unprivileged runtime login");
        Self {
            admin_url,
            database,
            admin,
            runtime,
        }
    }

    async fn cleanup(self) {
        self.runtime.close().await;
        self.admin.close().await;
        let server = PgPool::connect(&self.admin_url).await.unwrap();
        sqlx::query(&format!("DROP DATABASE \"{}\" WITH (FORCE)", self.database))
            .execute(&server)
            .await
            .unwrap();
        server.close().await;
    }
}

fn actor() -> Principal {
    Principal {
        role: PrincipalRole::Qolipchi,
        ref_: "clerk-1".into(),
        display_name: "Qolipchi".into(),
        legal_name: "Qolipchi".into(),
        phone: String::new(),
        avatar_url: String::new(),
    }
}

fn send_request(id: &str, codes: &[&str]) -> QolipReworkSend {
    QolipReworkSend {
        request_id: id.into(),
        qolip_codes: codes.iter().map(|code| (*code).into()).collect(),
    }
}

fn return_request(id: &str, reworks: &[&QolipRework]) -> QolipReworkReturn {
    QolipReworkReturn {
        request_id: id.into(),
        rework_ids: reworks.iter().map(|entry| entry.id.clone()).collect(),
    }
}

#[tokio::test]
#[ignore = "requires explicit isolated PostgreSQL admin URL"]
async fn postgres_rework_full_migrations_runtime_role_lifecycle_and_identity_guards() {
    let f = Fixture::new().await;
    let store = Arc::new(crate::db::postgres_qolip::PostgresQolipStore::new(
        f.runtime.clone(),
    ));
    let service = QolipService::new(store.clone());
    let actor = actor();
    assert!(!service.mileage("Q-ONE").await.unwrap().tracking_enabled);
    let request = send_request("send-one", &["Q-ONE", "LEGACY"]);
    let sent = service
        .send_reworks(request.clone(), &actor, false)
        .await
        .unwrap();
    assert_eq!(sent.len(), 2);
    assert_eq!(
        service.send_reworks(request, &actor, false).await.unwrap(),
        sent
    );
    assert_eq!(
        service
            .send_reworks(send_request("send-one", &["Q-TWO"]), &actor, false)
            .await
            .unwrap_err(),
        QolipError::ReworkRequestConflict
    );
    assert_eq!(store.locations("Block A").await.unwrap().len(), 1);
    assert!(
        store
            .location_by_qolip_code("LEGACY")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .product_spec_by_qolip_code("LEGACY")
            .await
            .unwrap()
            .is_some()
    );
    let stored_home_count: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_qolip_locations")
        .fetch_one(&f.runtime)
        .await
        .unwrap();
    assert_eq!(
        stored_home_count, 3,
        "home rows reserve identity while operational stock hides away molds"
    );
    for statement in [
        "DELETE FROM mini_qolip_product_specs WHERE qolip_code='Q-ONE'",
        "UPDATE mini_qolip_product_specs SET qolip_code='RENAMED' WHERE qolip_code='Q-ONE'",
        "DELETE FROM mini_qolip_locations WHERE qolip_code='LEGACY'",
        "UPDATE mini_qolip_locations SET row_letter='B' WHERE qolip_code='LEGACY'",
        "UPDATE mini_warehouses SET name='Renamed block' WHERE name='Block A'",
        "DELETE FROM mini_qolip_set_products WHERE item_code='ITEM-R'",
    ] {
        let error = sqlx::query(statement)
            .execute(&f.runtime)
            .await
            .expect_err("away identity is immutable");
        assert!(
            error
                .as_database_error()
                .unwrap()
                .message()
                .contains("qolip_in_rework"),
            "{statement}: {error}"
        );
    }
    let one = sent
        .iter()
        .find(|entry| entry.qolip_code == "Q-ONE")
        .unwrap();
    let legacy = sent
        .iter()
        .find(|entry| entry.qolip_code == "LEGACY")
        .unwrap();
    service
        .return_reworks(return_request("return-one", &[one]), &actor, false)
        .await
        .unwrap();
    service
        .return_reworks(return_request("return-one", &[one]), &actor, false)
        .await
        .unwrap();
    assert_eq!(service.mileage("Q-ONE").await.unwrap().cycles.len(), 1);
    assert_eq!(
        service.mileage("Q-ONE").await.unwrap().current_meter,
        Some(0.0)
    );
    assert!(!service.mileage("LEGACY").await.unwrap().tracking_enabled);
    assert_eq!(
        service.reworks("open", &actor, false).await.unwrap(),
        vec![legacy.clone()]
    );
    assert_eq!(store.locations("Block A").await.unwrap().len(), 2);
    sqlx::query("UPDATE mini_qolip_mileage_cycles SET meter=123.5 WHERE qolip_code_key='q-one' AND ended_at IS NULL")
        .execute(&f.runtime).await.unwrap();
    let next = service
        .send_reworks(send_request("send-again", &["Q-ONE"]), &actor, false)
        .await
        .unwrap();
    assert_eq!(
        service
            .return_reworks(return_request("stale-return", &[one]), &actor, false)
            .await
            .unwrap_err(),
        QolipError::ReworkConflict
    );
    assert_eq!(
        service.mileage("Q-ONE").await.unwrap().current_meter,
        Some(123.5)
    );
    assert_eq!(
        store.active_rework_codes(&["Q-ONE".into()]).await.unwrap(),
        vec!["Q-ONE"]
    );
    service
        .return_reworks(return_request("return-again", &[&next[0]]), &actor, false)
        .await
        .unwrap();
    let history = service.mileage("Q-ONE").await.unwrap();
    assert_eq!(history.current_meter, Some(0.0));
    assert_eq!(history.cycles.len(), 2);
    assert!(
        history
            .cycles
            .iter()
            .any(|cycle| cycle.meter == 123.5 && cycle.ended_at.is_some())
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM mini_qolip_locations WHERE qolip_code='Q-ONE'"
        )
        .fetch_one(&f.runtime)
        .await
        .unwrap(),
        1
    );
    assert_eq!(
        service
            .send_reworks(
                send_request("atomic-invalid", &["Q-TWO", "MISSING"]),
                &actor,
                false
            )
            .await
            .unwrap_err(),
        QolipError::QolipCodeNotFound
    );
    assert!(
        store
            .active_rework_codes(&["Q-TWO".into()])
            .await
            .unwrap()
            .is_empty()
    );
    let apparatus: (String, String) =
        sqlx::query_as("SELECT id, name FROM mini_apparatus ORDER BY id LIMIT 1")
            .fetch_one(&f.runtime)
            .await
            .unwrap();
    sqlx::query("INSERT INTO mini_production_maps(id,product_code,title,map_json) VALUES ('rework-test-order','ITEM-R','Label R','{}')")
        .execute(&f.runtime).await.unwrap();
    sqlx::query("INSERT INTO mini_order_run_sessions(session_id,apparatus,canonical_apparatus_id,order_id,status,worker_ref,payload_json)
        VALUES ('catalog-active',$1,$2,'rework-test-order','active','worker-1',
            jsonb_build_object('qolip_lock_owner',true,'qolip_code','CATALOG-ONLY','qolip_codes',jsonb_build_array('CATALOG-ONLY')))")
        .bind(&apparatus.1).bind(&apparatus.0).execute(&f.runtime).await.unwrap();
    assert_eq!(
        service
            .send_reworks(
                send_request("catalog-busy", &["CATALOG-ONLY"]),
                &actor,
                false
            )
            .await
            .unwrap_err(),
        QolipError::QolipInUse
    );
    sqlx::query("UPDATE mini_order_run_sessions SET payload_json=jsonb_build_object('qolip_lock_owner',true,'qolip_code','Q-ONE','qolip_codes',jsonb_build_array('Q-ONE')) WHERE session_id='catalog-active'")
        .execute(&f.runtime).await.unwrap();
    assert_eq!(
        service
            .send_reworks(
                send_request("send-one", &["Q-ONE", "LEGACY"]),
                &actor,
                false
            )
            .await
            .unwrap(),
        sent,
        "replay must precede a later active production guard"
    );
    sqlx::query("DELETE FROM mini_order_run_sessions WHERE session_id='catalog-active'")
        .execute(&f.runtime)
        .await
        .unwrap();
    assert_eq!(
        store
            .delete_product_specs(&["Q-ONE".into()])
            .await
            .unwrap_err(),
        QolipError::QolipHistoryLocked
    );
    service
        .return_reworks(return_request("return-legacy", &[legacy]), &actor, false)
        .await
        .unwrap();
    assert_eq!(
        store
            .delete_product_specs(&["LEGACY".into()])
            .await
            .unwrap_err(),
        QolipError::QolipHistoryLocked,
        "legacy-only catalog deletion must not orphan activated history"
    );
    service
        .issue_checkout(
            QolipCheckoutCreate {
                location_id: "home-Q-TWO".into(),
                quantity: 1,
                worker_id: "worker-1".into(),
            },
            "worker-1",
            "Worker",
            &actor,
        )
        .await
        .unwrap();
    assert_eq!(
        service
            .send_reworks(send_request("busy-worker", &["Q-TWO"]), &actor, false)
            .await
            .unwrap_err(),
        QolipError::QolipInUse
    );
    f.cleanup().await;
}

#[tokio::test]
#[ignore = "requires explicit isolated PostgreSQL admin URL"]
async fn postgres_rework_generated_request_ids_round_trip_through_http_and_persist() {
    use crate::app::AppState;
    use crate::config::AppConfig;
    use crate::core::admin::service::AdminService;
    use crate::core::session::manager::SessionManager;
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode, header};
    use tower::ServiceExt;

    async fn request(
        router: &axum::Router,
        token: &str,
        method: &str,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> (StatusCode, serde_json::Value) {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        body.map(ToString::to_string).unwrap_or_default(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    fn request_id() -> String {
        format!(
            "qolip-rework-{}-{:032x}",
            time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1000,
            rand::random::<u128>()
        )
    }

    let f = Fixture::new().await;
    let paths = tempfile::tempdir().unwrap();
    let mut state = AppState::new(AppConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        default_target_warehouse: String::new(),
        http_timeout: std::time::Duration::from_secs(5),
        session_store_path: paths.path().join("sessions.json"),
        profile_store_path: paths.path().join("profiles.json"),
        push_token_store_path: paths.path().join("push.json"),
        session_ttl_seconds: Some(3600),
        supplier_prefix: "10".into(),
        werka_prefix: "20".into(),
        werka_code: String::new(),
        werka_name: "Werka".into(),
        werka_phone: String::new(),
        material_taminotchi_code: String::new(),
        material_taminotchi_name: "Material taminotchisi".into(),
        material_taminotchi_phone: String::new(),
        admin_phone: String::new(),
        admin_name: "Admin".into(),
        admin_code: String::new(),
    });
    state.sessions = SessionManager::memory(Some(3600));
    state.admin = AdminService::new(&state.config);
    state.qolip = QolipService::new(Arc::new(
        crate::db::postgres_qolip::PostgresQolipStore::new(f.runtime.clone()),
    ));
    let token = state.sessions.create(actor()).await.unwrap();
    let router = crate::http::router::build_router(state);
    let send = serde_json::json!({
        "request_id": request_id(),
        "qolip_codes": [" Q-ONE ", "q-two", "q-one"]
    });
    assert!(send["request_id"].as_str().unwrap().len() <= 128);
    let (status, sent) = request(
        &router,
        &token,
        "POST",
        "/v1/mobile/qolip/reworks/send",
        Some(&send),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    assert_eq!(sent["ok"], true);
    assert_eq!(sent["reworks"].as_array().unwrap().len(), 2);
    let replay = request(
        &router,
        &token,
        "POST",
        "/v1/mobile/qolip/reworks/send",
        Some(&send),
    )
    .await;
    assert_eq!(replay, (StatusCode::OK, sent.clone()));
    let (status, open) = request(
        &router,
        &token,
        "GET",
        "/v1/mobile/qolip/reworks?status=open",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(open["reworks"].as_array().unwrap().len(), 2);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mini_qolip_reworks")
            .fetch_one(&f.runtime)
            .await
            .unwrap(),
        2
    );

    let receive = serde_json::json!({
        "request_id": request_id(),
        "rework_ids": sent["reworks"].as_array().unwrap().iter()
            .map(|entry| entry["id"].clone()).collect::<Vec<_>>()
    });
    assert_ne!(send["request_id"], receive["request_id"]);
    let (status, returned) = request(
        &router,
        &token,
        "POST",
        "/v1/mobile/qolip/reworks/return",
        Some(&receive),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{returned}");
    assert!(
        returned["reworks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["returned_at"].is_string())
    );
    assert_eq!(
        request(
            &router,
            &token,
            "POST",
            "/v1/mobile/qolip/reworks/return",
            Some(&receive),
        )
        .await,
        (StatusCode::OK, returned)
    );
    let (status, open) = request(
        &router,
        &token,
        "GET",
        "/v1/mobile/qolip/reworks?status=open",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(open["reworks"].as_array().unwrap().is_empty());
    let persisted: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM mini_qolip_reworks WHERE returned_at IS NOT NULL),
                (SELECT count(*) FROM mini_qolip_rework_requests),
                (SELECT count(*) FROM mini_qolip_mileage_cycles WHERE ended_at IS NULL)",
    )
    .fetch_one(&f.runtime)
    .await
    .unwrap();
    assert_eq!(
        persisted,
        (2, 2, 2),
        "retries must not duplicate visits or mileage cycles"
    );
    drop(router);
    f.cleanup().await;
}
