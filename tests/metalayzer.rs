#![cfg(feature = "verification")]
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use mini_rs_erp::{
    app::AppState,
    config::AppConfig,
    core::{
        auth::models::{Principal, PrincipalRole},
        gscale::{GscaleService, models::*, ports::*},
        session::manager::SessionManager,
        warehouses::{WarehouseAssignmentUpsert, WarehouseUpsert},
    },
    http::router::build_router,
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tower::ServiceExt;

struct Stocks(Mutex<Vec<RawMaterialStockEntry>>);
#[async_trait::async_trait]
impl MaterialReceiptStorePort for Stocks {
    async fn create_material_receipt_draft(
        &self,
        _: CreateMaterialReceiptDraftInput,
    ) -> Result<MaterialReceiptDraft, GscalePortError> {
        unreachable!()
    }
    async fn submit_stock_entry_draft(&self, _: &str) -> Result<(), GscalePortError> {
        unreachable!()
    }
    async fn delete_stock_entry_draft(&self, _: &str) -> Result<(), GscalePortError> {
        unreachable!()
    }
    async fn raw_material_stock_by_barcode(
        &self,
        barcode: &str,
    ) -> Result<Option<RawMaterialStockEntry>, GscalePortError> {
        Ok(self
            .0
            .lock()
            .await
            .iter()
            .find(|stock| stock.barcode.eq_ignore_ascii_case(barcode.trim()))
            .cloned())
    }
    async fn mark_raw_material_metalayzer(
        &self,
        barcode: &str,
        warehouses: &[String],
    ) -> Result<RawMaterialStockEntry, GscalePortError> {
        let mut stocks = self.0.lock().await;
        let row = stocks
            .iter_mut()
            .find(|stock| stock.barcode.eq_ignore_ascii_case(barcode.trim()))
            .ok_or_else(|| GscalePortError::InvalidInput("raw_material_stock_not_found".into()))?;
        if !warehouses
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&row.warehouse))
        {
            return Err(GscalePortError::InvalidInput(
                "metalayzer_warehouse_forbidden".into(),
            ));
        }
        if row.status == "consumed" {
            return Err(GscalePortError::InvalidInput(
                "metalayzer_material_unavailable".into(),
            ));
        }
        row.is_metalayzer = true;
        Ok(row.clone())
    }
}

fn config(path: &Path) -> AppConfig {
    AppConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        default_target_warehouse: String::new(),
        http_timeout: Duration::from_secs(2),
        session_store_path: path.join("sessions.json"),
        profile_store_path: path.join("profiles.json"),
        push_token_store_path: path.join("push.json"),
        session_ttl_seconds: Some(3600),
        supplier_prefix: "10".into(),
        werka_prefix: "20".into(),
        werka_code: String::new(),
        werka_name: String::new(),
        werka_phone: String::new(),
        material_taminotchi_code: String::new(),
        material_taminotchi_name: String::new(),
        material_taminotchi_phone: String::new(),
        admin_phone: String::new(),
        admin_name: String::new(),
        admin_code: String::new(),
    }
}
async fn session(state: &AppState, role: PrincipalRole, id: &str) -> String {
    state
        .sessions
        .create(Principal {
            role,
            ref_: id.into(),
            display_name: id.into(),
            legal_name: String::new(),
            phone: String::new(),
            avatar_url: String::new(),
        })
        .await
        .unwrap()
}
async fn request(
    router: &Router,
    method: &str,
    token: &str,
    barcode: &str,
    extra: bool,
) -> (StatusCode, Value) {
    let path = if method == "GET" {
        format!("/v1/mobile/metalayzer/material?barcode={barcode}")
    } else {
        "/v1/mobile/metalayzer/complete".into()
    };
    let mut body = json!({"barcode":barcode});
    if extra {
        body["is_metalayzer"] = json!(false);
    }
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(if method == "GET" {
                    Body::empty()
                } else {
                    Body::from(body.to_string())
                })
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn scan_mark_rescan_is_role_scoped_and_does_not_change_stock() {
    let directory = tempfile::tempdir().unwrap();
    let mut state = AppState::verification(config(directory.path()));
    state.sessions = SessionManager::memory(Some(3600));
    for name in ["Raw W", "Other W"] {
        state
            .warehouses
            .upsert_warehouse(WarehouseUpsert {
                warehouse: name.into(),
                ..Default::default()
            })
            .await
            .unwrap();
    }
    state
        .warehouses
        .assign_warehouse(WarehouseAssignmentUpsert {
            assignment_kind: "warehouse".into(),
            warehouse: "Raw W".into(),
            warehouse_name: Some("Raw W".into()),
            apparatus_id: None,
            principal_role: PrincipalRole::Metalayzer,
            principal_ref: "metal".into(),
            display_name: "Metal".into(),
        })
        .await
        .unwrap();
    let base = RawMaterialStockEntry {
        id: "stock-1".into(),
        barcode: "ROLL".into(),
        warehouse: "Raw W".into(),
        item_code: "FILM".into(),
        item_name: "Film".into(),
        qty: 100.0,
        uom: "kg".into(),
        status: "reserved".into(),
        reserved_order_id: "order-1".into(),
        ..Default::default()
    };
    let store = Arc::new(Stocks(Mutex::new(vec![
        base.clone(),
        RawMaterialStockEntry {
            barcode: "OTHER".into(),
            warehouse: "Other W".into(),
            ..base.clone()
        },
        RawMaterialStockEntry {
            barcode: "USED".into(),
            status: "consumed".into(),
            ..base.clone()
        },
    ])));
    state.gscale = GscaleService::new().with_receipt_store(store.clone());
    let token = session(&state, PrincipalRole::Metalayzer, "metal").await;
    let no_warehouse = session(&state, PrincipalRole::Metalayzer, "no-warehouse").await;
    let admin = session(&state, PrincipalRole::Admin, "admin").await;
    let router = build_router(state);
    assert_eq!(
        request(&router, "GET", "missing", "ROLL", false).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(&router, "POST", &admin, "ROLL", false).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&router, "POST", &no_warehouse, "ROLL", false)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&router, "GET", &token, "OTHER", false).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&router, "POST", &token, "OTHER", false).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&router, "GET", &token, "UNKNOWN", false).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(&router, "POST", &token, "USED", false).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(&router, "POST", &token, "ROLL", true).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let (status, before) = request(&router, "GET", &token, "ROLL", false).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(before["is_metalayzer"], false);
    for _ in 0..2 {
        let (status, response) = request(&router, "POST", &token, "ROLL", false).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(response["is_metalayzer"], true);
    }
    assert_eq!(
        request(&router, "GET", &token, "ROLL", false).await.1["is_metalayzer"],
        true
    );
    let updated = &store.0.lock().await[0];
    let mut expected = base;
    expected.is_metalayzer = true;
    assert_eq!(updated, &expected);
}

#[tokio::test]
#[ignore = "requires explicit MINI_ERP_TEST_ADMIN_DATABASE_URL; creates a disposable database"]
async fn postgres_mark_is_atomic_scoped_idempotent_and_retains_other_fields() {
    use mini_rs_erp::db::postgres_gscale_receipt::PostgresGscaleReceiptStore;
    use sqlx::{PgPool, postgres::PgConnectOptions};
    let url =
        std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL").expect("explicit test database URL");
    let admin = PgPool::connect(&url).await.unwrap();
    let name = format!("mini_rs_erp_test_metalayzer_{:016x}", rand::random::<u64>());
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&admin)
        .await
        .unwrap();
    let pool = PgPool::connect_with(url.parse::<PgConnectOptions>().unwrap().database(&name))
        .await
        .unwrap();
    sqlx::raw_sql("CREATE TABLE mini_raw_material_stock (id text PRIMARY KEY, barcode text UNIQUE NOT NULL, warehouse text NOT NULL, item_code text, item_name text, qty numeric, uom text, status text, reserved_order_id text, source_receipt_id text, width_mm numeric, micron numeric, length_m numeric, updated_at timestamptz NOT NULL DEFAULT now());
        INSERT INTO mini_raw_material_stock (id,barcode,warehouse,item_code,item_name,qty,uom,status,reserved_order_id,source_receipt_id) VALUES ('raw:roll','ROLL','Raw W','FILM','Film',100,'kg','reserved','order-1','receipt-1'),('raw:used','USED','Raw W','FILM','Film',100,'kg','consumed','order-1','receipt-1')").execute(&pool).await.unwrap();
    sqlx::raw_sql(include_str!(
        "../migrations/postgres/0197_raw_material_metalayzer.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let store = PostgresGscaleReceiptStore::new(pool.clone());
    let original:Value = sqlx::query_scalar("SELECT to_jsonb(s) - 'updated_at' - 'is_metalayzer' FROM mini_raw_material_stock s WHERE id='raw:roll'").fetch_one(&pool).await.unwrap();
    assert!(
        !store
            .raw_material_stock_by_barcode("ROLL")
            .await
            .unwrap()
            .unwrap()
            .is_metalayzer
    );
    assert!(
        store
            .mark_raw_material_metalayzer("ROLL", &["Other W".into()])
            .await
            .is_err()
    );
    assert!(
        !store
            .raw_material_stock_by_barcode("ROLL")
            .await
            .unwrap()
            .unwrap()
            .is_metalayzer
    );
    assert!(
        store
            .mark_raw_material_metalayzer("USED", &["Raw W".into()])
            .await
            .is_err()
    );
    let warehouses = vec!["Raw W".to_string()];
    let (left, right) = tokio::join!(
        store.mark_raw_material_metalayzer("roll", &warehouses),
        store.mark_raw_material_metalayzer("ROLL", &warehouses)
    );
    assert!(left.unwrap().is_metalayzer && right.unwrap().is_metalayzer);
    let version: String = sqlx::query_scalar(
        "SELECT updated_at::text FROM mini_raw_material_stock WHERE id='raw:roll'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        store
            .mark_raw_material_metalayzer("ROLL", &warehouses)
            .await
            .unwrap()
            .is_metalayzer
    );
    let unchanged: bool = sqlx::query_scalar(
        "SELECT updated_at::text=$1 FROM mini_raw_material_stock WHERE id='raw:roll'",
    )
    .bind(version)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(unchanged);
    let after:Value = sqlx::query_scalar("SELECT to_jsonb(s) - 'updated_at' - 'is_metalayzer' FROM mini_raw_material_stock s WHERE id='raw:roll'").fetch_one(&pool).await.unwrap();
    assert_eq!(original, after);
    // Existing writers omit processing metadata, so receipt updates preserve it.
    sqlx::query(
        "UPDATE mini_raw_material_stock SET item_name='Corrected film', qty=90 WHERE id='raw:roll'",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        store
            .raw_material_stock_by_barcode("ROLL")
            .await
            .unwrap()
            .unwrap()
            .is_metalayzer
    );
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {name}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
