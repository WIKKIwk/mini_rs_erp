#![cfg(feature = "verification")]
//! Focused no-database tests; avoids compiling the monolithic legacy lib tests.
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use mini_rs_erp::core::{
    apparatus_standard::{ApparatusId, RuntimeApparatusConfiguration},
    auth::models::{Principal, PrincipalRole},
    production_map::*,
    warehouses::{WarehouseAssignmentUpsert, WarehouseUpsert},
};
use mini_rs_erp::{app::AppState, config::AppConfig, http::router::build_router};
use std::{path::Path, sync::Arc, time::Duration};
use tower::ServiceExt;

struct EmptyApparatusCatalog;
#[async_trait::async_trait]
impl CanonicalApparatusResolver for EmptyApparatusCatalog {
    async fn resolve(
        &self,
        _: &ApparatusId,
    ) -> Result<Option<Arc<RuntimeApparatusConfiguration>>, ProductionMapError> {
        Ok(None)
    }
    async fn list(&self) -> Result<Vec<Arc<RuntimeApparatusConfiguration>>, ProductionMapError> {
        Ok(Vec::new())
    }
}
async fn service_with_memory_apparatus(
    store: Arc<MemoryProductionMapStore>,
) -> ProductionMapService {
    ProductionMapService::new(store, Arc::new(EmptyApparatusCatalog))
}
fn canonical_apparatus_stage_map(id: &str, apparatus: &str, name: &str) -> ProductionMapDefinition {
    serde_json::from_value(serde_json::json!({
        "id":id,"product_code":"P-1","title":"Product",
        "nodes":[{"id":"start","kind":"start","title":"Start"},
            {"id":"cut","kind":"apparatus","title":name,"apparatus_id":apparatus},
            {"id":"end","kind":"end","title":"End"}],
        "edges":[{"from":"start","to":"cut"},{"from":"cut","to":"end"}]
    }))
    .unwrap()
}
fn test_config(path: &Path) -> AppConfig {
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
        werka_name: "Warehouse".into(),
        werka_phone: String::new(),
        material_taminotchi_code: String::new(),
        material_taminotchi_name: String::new(),
        material_taminotchi_phone: String::new(),
        admin_phone: String::new(),
        admin_name: String::new(),
        admin_code: String::new(),
    }
}

async fn session_for(state: &AppState, role: PrincipalRole, ref_: &str) -> String {
    state
        .sessions
        .create(Principal {
            role,
            display_name: "Admin".to_string(),
            legal_name: "Admin".to_string(),
            ref_: ref_.to_string(),
            phone: "+998880000000".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("session")
}

async fn assign_warehouse_to_principal(
    state: &AppState,
    role: PrincipalRole,
    ref_: &str,
    warehouse: &str,
) {
    state
        .warehouses
        .upsert_warehouse(WarehouseUpsert {
            warehouse: warehouse.to_string(),
            company: "Company".to_string(),
            is_group: false,
            parent_warehouse: String::new(),
        })
        .await
        .expect("warehouse");
    state
        .warehouses
        .assign_warehouse(WarehouseAssignmentUpsert {
            assignment_kind: "warehouse".to_string(),
            warehouse: warehouse.to_string(),
            warehouse_name: None,
            apparatus_id: None,
            principal_role: role,
            principal_ref: ref_.to_string(),
            display_name: "Materialchi".to_string(),
        })
        .await
        .expect("warehouse assignment");
}

fn request(method: &str, uri: &str, token: &str) -> Request<Body> {
    request_with_body(method, uri, token, "")
}
fn request_with_body(method: &str, uri: &str, token: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

const APPARATUS: &str = "apparatus:default:asset-010";

fn ready_snapshot() -> WarehouseWipSnapshot {
    let mut batch: OrderProgressBatch = serde_json::from_value(serde_json::json!({
        "batch_id":"warehouse-roll", "session_id":"warehouse-session", "started_at_unix":1,
        "completed_at_unix":2, "apparatus":APPARATUS, "order_id":"warehouse-order",
        "action":"roll_complete", "status":"completed", "produced_qty":10.0, "uom":"kg",
        "qr_payload":"PROGRESS:warehouse-roll", "label_item_code":"P-1", "label_item_name":"Product",
        "executor_name":"Operator", "worker_role":"aparatchi", "worker_ref":"worker",
        "worker_display_name":"Operator", "wip_status":"waiting", "current_apparatus":APPARATUS,
        "current_location":"Finished at cutting", "finished_goods_kg":10.0, "payload_json":{}
    })).unwrap();
    batch.refresh_status_detail();
    WarehouseWipSnapshot {
        batch,
        order: Some(canonical_apparatus_stage_map(
            "warehouse-order",
            APPARATUS,
            "Rezka",
        )),
        order_receiving_allowed: true,
        apparatus_active: true,
        paddon_code: None,
        qr_is_paddon: false,
    }
}

fn actor() -> QueueActionActor {
    QueueActionActor {
        role: "werka".into(),
        ref_: "keeper".into(),
        display_name: "Keeper".into(),
    }
}

#[test]
fn warehouse_wip_snapshot_rejects_every_nonreceivable_state() {
    let ready = ready_snapshot();
    assert_eq!(ready.receive_blocked_reason(), None);
    let token = ready.snapshot_token();
    let edits: Vec<Box<dyn Fn(&mut WarehouseWipSnapshot)>> = vec![
        Box::new(|s| s.paddon_code = Some("00001".into())),
        Box::new(|s| s.qr_is_paddon = true),
        Box::new(|s| s.apparatus_active = false),
        Box::new(|s| s.order_receiving_allowed = false),
        Box::new(|s| s.order = None),
        Box::new(|s| s.batch.wip_status = OrderProgressBatchWipStatus::InUse),
        Box::new(|s| s.batch.wip_status = OrderProgressBatchWipStatus::Processed),
        Box::new(|s| s.batch.used_by_session_id = "used".into()),
        Box::new(|s| s.batch.current_apparatus = "apparatus:other".into()),
        Box::new(|s| s.batch.current_location.clear()),
        Box::new(|s| s.batch.next_apparatus = "apparatus:other".into()),
        Box::new(|s| {
            s.batch.finished_goods_kg = Some(0.0);
            s.batch.produced_qty = 0.0;
        }),
        Box::new(|s| s.batch.payload_json["stage_node_id"] = serde_json::json!("deleted-stage")),
    ];
    for edit in edits {
        let mut changed = ready.clone();
        edit(&mut changed);
        assert!(
            changed.receive_blocked_reason().is_some(),
            "accepted invalid snapshot {changed:?}"
        );
        assert_ne!(changed.snapshot_token(), token);
    }
    let mut changed = ready.clone();
    changed.batch.revision += 1;
    assert_ne!(changed.snapshot_token(), token);
    let mut changed = ready;
    changed.order.as_mut().unwrap().title.push_str(" corrected");
    assert_ne!(changed.snapshot_token(), token);
}

#[tokio::test]
async fn warehouse_wip_scan_is_read_only_and_receipt_retries_are_bounded() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = service_with_memory_apparatus(store.clone()).await;
    let ready = ready_snapshot();
    store.put_map(ready.order.clone().unwrap()).await.unwrap();
    store
        .put_order_progress_batch(ready.batch.clone())
        .await
        .unwrap();
    let first = service
        .warehouse_wip_snapshot(&ready.batch.qr_payload)
        .await
        .unwrap()
        .unwrap();
    let second = service
        .warehouse_wip_snapshot(&ready.batch.qr_payload)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(
        store
            .progress_batch(&ready.batch.batch_id)
            .await
            .unwrap()
            .unwrap(),
        ready.batch
    );
    assert_eq!(first.receive_blocked_reason(), None);
    let token = first.snapshot_token();
    assert_eq!(
        service
            .receive_warehouse_wip(
                &ready.batch.batch_id,
                &ready.batch.qr_payload,
                "WH-1",
                "stale",
                actor()
            )
            .await,
        Err(ProductionMapError::WarehouseWipConflict)
    );
    assert_eq!(
        store
            .progress_batch(&ready.batch.batch_id)
            .await
            .unwrap()
            .unwrap(),
        ready.batch
    );
    let received = service
        .receive_warehouse_wip(
            &ready.batch.batch_id,
            &ready.batch.qr_payload,
            "WH-1",
            &token,
            actor(),
        )
        .await
        .unwrap();
    let retried = service
        .receive_warehouse_wip(
            &ready.batch.batch_id,
            &ready.batch.qr_payload,
            "WH-1",
            &token,
            actor(),
        )
        .await
        .unwrap();
    assert_eq!(received, retried);
    assert_eq!(
        received.0.wip_status,
        OrderProgressBatchWipStatus::Processed
    );
    let preview = service
        .warehouse_wip_snapshot(&ready.batch.qr_payload)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(preview.receive_blocked_reason(), Some("already_received"));
    assert_eq!(preview.receipt(), Some(received.1));
    assert_eq!(
        service
            .receive_warehouse_wip(
                &ready.batch.batch_id,
                &ready.batch.qr_payload,
                "WH-2",
                &token,
                actor()
            )
            .await,
        Err(ProductionMapError::WarehouseWipConflict)
    );
    assert_eq!(
        service
            .receive_warehouse_wip(
                "other-batch",
                &ready.batch.qr_payload,
                "WH-1",
                &token,
                actor()
            )
            .await,
        Err(ProductionMapError::WarehouseWipConflict)
    );
}

#[tokio::test]
async fn warehouse_wip_rejects_duplicate_qr_and_postscan_corrections() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = service_with_memory_apparatus(store.clone()).await;
    let ready = ready_snapshot();
    store.put_map(ready.order.unwrap()).await.unwrap();
    store
        .put_order_progress_batch(ready.batch.clone())
        .await
        .unwrap();
    let preview = service
        .warehouse_wip_snapshot(&ready.batch.qr_payload)
        .await
        .unwrap()
        .unwrap();
    let mut changed = ready.batch.clone();
    changed.revision += 1;
    store
        .put_order_progress_batch(changed.clone())
        .await
        .unwrap();
    assert_eq!(
        service
            .receive_warehouse_wip(
                &ready.batch.batch_id,
                &ready.batch.qr_payload,
                "WH-1",
                &preview.snapshot_token(),
                actor()
            )
            .await,
        Err(ProductionMapError::WarehouseWipConflict)
    );
    changed.batch_id = "duplicate-roll".into();
    changed.qr_payload = changed.qr_payload.to_lowercase();
    store.put_order_progress_batch(changed).await.unwrap();
    assert_eq!(
        service
            .warehouse_wip_snapshot(&ready.batch.qr_payload)
            .await,
        Err(ProductionMapError::WarehouseQrAmbiguous)
    );
}

#[tokio::test]
async fn warehouse_wip_independent_services_recover_one_persisted_receipt() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let first = service_with_memory_apparatus(store.clone()).await;
    let second = service_with_memory_apparatus(store.clone()).await;
    let ready = ready_snapshot();
    store.put_map(ready.order.unwrap()).await.unwrap();
    store
        .put_order_progress_batch(ready.batch.clone())
        .await
        .unwrap();
    let scan = first
        .warehouse_wip_snapshot(&ready.batch.qr_payload)
        .await
        .unwrap()
        .unwrap();
    let token = scan.snapshot_token();
    let (a, b) = tokio::join!(
        first.receive_warehouse_wip(
            &ready.batch.batch_id,
            &ready.batch.qr_payload,
            "WH-1",
            &token,
            actor()
        ),
        second.receive_warehouse_wip(
            &ready.batch.batch_id,
            &ready.batch.qr_payload,
            "WH-1",
            &token,
            actor()
        ),
    );
    assert_eq!(a.unwrap(), b.unwrap());
}

#[tokio::test]
async fn warehouse_qr_preview_never_receives_and_explicit_receipt_is_scoped() {
    let directory = tempfile::tempdir().unwrap();
    let mut state = AppState::verification(test_config(directory.path()));
    let store = Arc::new(MemoryProductionMapStore::new());
    state.production_maps = service_with_memory_apparatus(store.clone()).await;
    let station = "apparatus:default:asset-010";
    let batch: OrderProgressBatch = serde_json::from_value(serde_json::json!({
        "batch_id":"scan-roll", "session_id":"scan-session", "started_at_unix":1,
        "completed_at_unix":2, "apparatus":station, "order_id":"scan-order",
        "action":"roll_complete", "status":"completed", "produced_qty":10.0, "uom":"kg",
        "qr_payload":"PROGRESS:scan-roll", "label_item_code":"P-1", "label_item_name":"Product",
        "executor_name":"Operator", "worker_role":"aparatchi", "worker_ref":"worker",
        "worker_display_name":"Operator", "wip_status":"waiting", "current_apparatus":station,
        "current_location":"Finished at cutting", "finished_goods_kg":10.0, "payload_json":{}
    }))
    .unwrap();
    store
        .put_map(
            serde_json::from_value(serde_json::json!({
                "id":"scan-order", "product_code":"P-1", "title":"Product",
                "nodes":[{"id":"start","kind":"start","title":"Start"},
                    {"id":"cut","kind":"apparatus","title":"Rezka","apparatus_id":station},
                    {"id":"end","kind":"end","title":"End"}],
                "edges":[{"from":"start","to":"cut"},{"from":"cut","to":"end"}]
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    store.put_order_progress_batch(batch.clone()).await.unwrap();
    let token = session_for(&state, PrincipalRole::Werka, "scanner-keeper").await;
    assign_warehouse_to_principal(&state, PrincipalRole::Werka, "scanner-keeper", "WH-1").await;
    let preview_uri = "/v1/mobile/werka/qr/preview?qr_payload=PROGRESS%3Ascan-roll";
    let response = build_router(state.clone())
        .oneshot(request("GET", preview_uri, &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let preview: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(preview["kind"], "wip");
    assert_eq!(preview["can_receive"], true);
    assert!(preview["receipt"].is_null());
    assert_eq!(
        store
            .progress_batch(&batch.batch_id)
            .await
            .unwrap()
            .unwrap(),
        batch
    );
    let mut input = serde_json::json!({"progress_batch_id":batch.batch_id,"qr_payload":batch.qr_payload,
        "warehouse":"WH-2","snapshot_token":preview["snapshot_token"]});
    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/werka/wip/receive",
            &token,
            &input.to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        store
            .progress_batch(&batch.batch_id)
            .await
            .unwrap()
            .unwrap(),
        batch
    );
    input["warehouse"] = serde_json::json!("WH-1");
    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/werka/wip/receive",
            &token,
            &input.to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = build_router(state.clone())
        .oneshot(request("GET", preview_uri, &token))
        .await
        .unwrap();
    let received: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(received["can_receive"], false);
    assert_eq!(received["receipt"]["warehouse"], "WH-1");
    let other = session_for(&state, PrincipalRole::Werka, "other-scanner").await;
    assign_warehouse_to_principal(&state, PrincipalRole::Werka, "other-scanner", "WH-2").await;
    let response = build_router(state)
        .oneshot(request("GET", preview_uri, &other))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
