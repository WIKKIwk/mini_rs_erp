use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use tokio::sync::Mutex;
use tower::ServiceExt;

use super::router::build_router;
use crate::app::AppState;
use crate::config::AppConfig;
use crate::core::admin::models::{
    AdminDirectoryEntry, AdminItemDetail, AdminItemGroup, AdminState,
};
use crate::core::admin::ports::{AdminPortError, AdminReadPort, AdminStatePort, AdminWritePort};
use crate::core::admin::service::AdminService;
use crate::core::apparatus_standard::ApparatusId;
use crate::core::apparatus_standard::test_support::{TestApparatusSpec, canonical_draft};
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::auth::ports::{
    AdminAccessState, AdminAccessStateLookup, AuthPortError, CustomerLookup, CustomerRecord,
    MaterialTaminotchiLookup, MaterialTaminotchiRecord, SupplierLookup, SupplierRecord,
};
use crate::core::authz::{
    MemoryRoleDefinitionStore, RoleAssignment, RoleDefinition, RoleDefinitionStorePort,
};
use crate::core::calculate_orders::{
    CalculateOrderError, CalculateOrderImage, CalculateOrderStorePort, CalculateOrderTemplate,
};
use crate::core::gscale::GscaleService;
use crate::core::gscale::models::{
    CreateMaterialReceiptDraftInput, MaterialReceiptDraft, RawMaterialStockDeleteInput,
    RawMaterialStockEntry, RawMaterialStockUpdateInput, ScaleDriverPrintRequest,
    ScaleDriverPrintResponse,
};
use crate::core::gscale::ports::{GscalePortError, MaterialReceiptStorePort, ScaleDriverPort};
use crate::core::inventory_movements::{
    InventoryAsset, InventoryAssetKind, InventoryLocation, InventoryLocationApparatus,
    InventoryLocationKind, InventoryLocationRef, InventoryMovementService,
    MemoryInventoryMovementStore,
};
use crate::core::mini_orders::{MiniOrderError, MiniOrderSink, NoopMiniOrderSink};
use crate::core::production_map::{
    CanonicalServiceApparatusResolver, MemoryProductionMapStore, ProductionMapService,
};
use crate::core::profile::ports::{ProfilePrefs, ProfileStoreError, ProfileStorePort};
use crate::core::returned_paint::{MemoryReturnedPaintStore, ReturnedPaintService};
use crate::core::session::manager::SessionManager;
use crate::core::system_users::{MemorySystemUserStore, SystemUserService};
use crate::core::warehouses::{
    MemoryWarehouseStore, WarehouseAssignmentUpsert, WarehouseService, WarehouseStockItem,
    WarehouseUpsert,
};
use crate::core::werka::models::{CustomerDirectoryEntry, DispatchRecord, SupplierItem};
use crate::core::werka::ports::{WerkaHomeLookup, WerkaPortError};
use crate::core::werka::service::WerkaService;
use crate::core::worker_groups::{MemoryWorkerGroupStore, WorkerGroupService};
use crate::core::workers::{MemoryWorkerStore, WorkerService, WorkerUpsert};
use crate::store::calculate_order_store::CalculateOrderStore;

mod admin_edge_cases;
mod apparatus_aasx;
mod apparatus_collections;
mod auth_roles;
mod batch_move_advanced;
mod batch_move_basic;
mod boyoqchi_returned_paint;
mod completion_rejections;
mod completion_requests;
mod factory_locations;
mod fakes;
mod inventory_movements;
mod item_groups;
mod opening_wip;
mod production_map_basic;
mod production_map_save_order;
mod production_map_validation;
mod qolip_blocks;
mod qolip_cell_qr;
mod qolip_checkout;
mod qolip_return_move;
mod qolipchi_workers;
mod queue_history;
mod queue_progress;
mod raw_materials;
mod run_capabilities;
mod suppliers_customers;
mod system_monitor;
mod users_settings;
mod warehouses_groups;
mod workers;

use self::fakes::*;

struct FailCalculateUpsertStore;

#[async_trait]
impl CalculateOrderStorePort for FailCalculateUpsertStore {
    async fn list(
        &self,
        _owner_key: &str,
    ) -> Result<Vec<CalculateOrderTemplate>, CalculateOrderError> {
        Ok(Vec::new())
    }

    async fn upsert(
        &self,
        _owner_key: &str,
        template: CalculateOrderTemplate,
    ) -> Result<CalculateOrderTemplate, CalculateOrderError> {
        let _ = template;
        Err(CalculateOrderError::StoreFailed)
    }

    async fn delete(&self, _owner_key: &str, _id: &str) -> Result<(), CalculateOrderError> {
        Ok(())
    }

    async fn save_image(
        &self,
        _owner_key: &str,
        _image: CalculateOrderImage,
    ) -> Result<CalculateOrderImage, CalculateOrderError> {
        Err(CalculateOrderError::StoreFailed)
    }

    async fn get_image(
        &self,
        _owner_key: &str,
        _image_id: &str,
    ) -> Result<Option<CalculateOrderImage>, CalculateOrderError> {
        Err(CalculateOrderError::StoreFailed)
    }
}

#[derive(Debug)]
struct FakeProductionOrderSink {
    calls: AtomicUsize,
    fail: bool,
    delay: Option<Duration>,
}

impl FakeProductionOrderSink {
    fn fail_after(delay: Duration) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            fail: true,
            delay: Some(delay),
        }
    }
}

#[async_trait]
impl MiniOrderSink for FakeProductionOrderSink {
    async fn save_order(
        &self,
        _map: &crate::core::production_map::ProductionMapDefinition,
        _template: &CalculateOrderTemplate,
    ) -> Result<(), MiniOrderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(delay) = self.delay {
            tokio::time::sleep(delay).await;
        }
        if self.fail {
            Err(MiniOrderError::StoreFailed)
        } else {
            Ok(())
        }
    }
}

fn test_state_with_failing_calculate() -> AppState {
    let mut state = test_state();
    state.calculate_orders = Arc::new(FailCalculateUpsertStore);
    state
}

fn pechat_order_map_json(id: &str, title: &str, order_number: &str, apparatus: &str) -> String {
    pechat_order_map_json_with_dims(id, title, order_number, apparatus, 7, 1250.0)
}

fn canonical_test_apparatus_id(apparatus: &str) -> String {
    ApparatusId::new(apparatus.trim().to_string())
        .expect("test production-map fixtures must use a canonical apparatus id")
        .to_string()
}

fn canonical_material_policy_body(
    apparatus_id: &str,
    expected_revision: u64,
    material: serde_json::Value,
    tooling_required: bool,
) -> String {
    let tooling = if tooling_required {
        serde_json::json!({
            "mode": "qolip_scan_required",
            "tooling_class_id": "tooling-class:qolip"
        })
    } else {
        serde_json::json!({"mode": "not_required"})
    };
    serde_json::json!({
        "apparatus_id": apparatus_id,
        "expected_revision": expected_revision,
        "material": material,
        "tooling": tooling
    })
    .to_string()
}

fn canonical_requirement_set_material_policy_body(
    apparatus_id: &str,
    expected_revision: u64,
    item_group_ids: &[&str],
    tooling_required: bool,
) -> String {
    canonical_material_policy_body(
        apparatus_id,
        expected_revision,
        serde_json::json!({
            "mode": "requirement_sets",
            "sets": [{
                "requirement_id": "test-material",
                "item_group_ids": item_group_ids,
                "minimum_required_count": 1
            }]
        }),
        tooling_required,
    )
}

fn canonical_apparatus_draft_body(seed: &str, display_name: &str) -> String {
    let apparatus_id = format!("apparatus:test:{seed}");
    serde_json::to_string(&canonical_draft(&TestApparatusSpec::print(
        &apparatus_id,
        display_name,
        crate::core::apparatus_standard::ProcessTechnology::Flexographic,
        None,
    )))
    .expect("canonical apparatus test draft")
}

fn canonical_apparatus_update_body(
    seed: &str,
    display_name: &str,
    expected_revision: u64,
) -> String {
    serde_json::json!({
        "expected_revision": expected_revision,
        "draft": serde_json::from_str::<serde_json::Value>(&canonical_apparatus_draft_body(
            seed,
            display_name,
        ))
        .expect("canonical apparatus draft JSON")
    })
    .to_string()
}

fn two_apparatus_order_map_json(
    id: &str,
    title: &str,
    order_number: &str,
    first_apparatus: &str,
    second_apparatus: &str,
) -> String {
    let first_id = canonical_test_apparatus_id(first_apparatus);
    let second_id = canonical_test_apparatus_id(second_apparatus);
    let first_rezka_config = rezka_test_node_config(&first_id);
    let second_rezka_config = rezka_test_node_config(&second_id);
    format!(
        r#"{{
            "id":"{id}",
            "product_code":"PECHAT-{order_number}",
            "title":"{title}",
            "order_number":"{order_number}",
            "nodes":[
                {{"id":"start","kind":"start","title":"Start"}},
                {{"id":"first","kind":"apparatus","title":"{first_apparatus}","apparatus_id":"{first_id}"{first_rezka_config}}},
                {{"id":"second","kind":"apparatus","title":"{second_apparatus}","apparatus_id":"{second_id}"{second_rezka_config}}},
                {{"id":"end","kind":"end","title":"End"}}
            ],
            "edges":[
                {{"from":"start","to":"first"}},
                {{"from":"first","to":"second"}},
                {{"from":"second","to":"end"}}
            ]
        }}"#
    )
}

fn pechat_order_map_json_with_dims(
    id: &str,
    title: &str,
    order_number: &str,
    apparatus: &str,
    roll_count: i64,
    width_mm: f64,
) -> String {
    production_order_map_json_with_product(
        id,
        title,
        &format!("PECHAT-{order_number}"),
        order_number,
        apparatus,
        roll_count,
        width_mm,
    )
}

fn production_order_map_json_with_product(
    id: &str,
    title: &str,
    product_code: &str,
    order_number: &str,
    apparatus: &str,
    roll_count: i64,
    width_mm: f64,
) -> String {
    let apparatus_id = canonical_test_apparatus_id(apparatus);
    let rezka_config = rezka_test_node_config(&apparatus_id);
    format!(
        r#"{{
            "id":"{id}",
            "product_code":"{product_code}",
            "title":"{title}",
            "order_number":"{order_number}",
            "roll_count":{roll_count},
            "width_mm":{width_mm},
            "nodes":[
                {{"id":"start","kind":"start","title":"Start"}},
                {{"id":"apparatus","kind":"apparatus","title":"{apparatus}","apparatus_id":"{apparatus_id}"{rezka_config}}},
                {{"id":"end","kind":"end","title":"End"}}
            ],
            "edges":[
                {{"from":"start","to":"apparatus"}},
                {{"from":"apparatus","to":"end"}}
            ]
        }}"#
    )
}

fn rezka_test_node_config(apparatus_id: &str) -> &'static str {
    if apparatus_id.eq_ignore_ascii_case("apparatus:default:asset-010") {
        r#", "rezka_kadr_count": 4, "rezka_label_length": 100"#
    } else {
        ""
    }
}

fn pechat_task_rezka_order_map_json(id: &str, title: &str, order_number: &str) -> String {
    r#"{{
        "id":"{id}",
        "product_code":"PECHAT-REZKA-{order_number}",
        "title":"{title}",
        "order_number":"{order_number}",
        "roll_count":7,
        "width_mm":1250,
        "nodes":[
            {{"id":"start","kind":"start","title":"Start"}},
            {{"id":"pechat","kind":"apparatus","title":"Pechat","apparatus_id":"apparatus:default:bosma_7"}},
            {{"id":"laminatsiya_task","kind":"task","title":"Laminatsiya"}},
            {{"id":"rezka","kind":"apparatus","title":"Rezka","apparatus_id":"apparatus:default:asset-010","rezka_kadr_count":4,"rezka_label_length":100}},
            {{"id":"end","kind":"end","title":"End"}}
        ],
        "edges":[
            {{"from":"start","to":"pechat"}},
            {{"from":"pechat","to":"laminatsiya_task"}},
            {{"from":"laminatsiya_task","to":"rezka"}},
            {{"from":"rezka","to":"end"}}
        ]
    }}"#
    .replace("{id}", id)
    .replace("{title}", title)
    .replace("{order_number}", order_number)
    .replace("{{", "{")
    .replace("}}", "}")
}

fn laminatsiya_order_map_json(id: &str, width_mm: f64) -> String {
    format!(
        r#"{{
            "id":"{id}",
            "product_code":"LAMIN-{id}",
            "title":"Laminatsiya order",
            "order_number":"{id}",
            "roll_count":7,
            "width_mm":{width_mm},
            "nodes":[
                {{"id":"start","kind":"start","title":"Start"}},
                {{
                    "id":"laminatsiya",
                    "kind":"apparatus",
                    "title":"Laminatsiya 1",
                    "apparatus_id":"apparatus:default:asset-007"
                }},
                {{"id":"end","kind":"end","title":"End"}}
            ],
            "edges":[
                {{"from":"start","to":"laminatsiya"}},
                {{"from":"laminatsiya","to":"end"}}
            ]
        }}"#
    )
}

fn test_state() -> AppState {
    let mut state = AppState::new(AppConfig {
        bind_addr: "127.0.0.1:8081".parse().expect("addr"),
        default_target_warehouse: "Stores - CH".to_string(),
        http_timeout: std::time::Duration::from_secs(15),
        session_store_path: "data/mobile_sessions.json".into(),
        profile_store_path: "data/mobile_profile_prefs.json".into(),
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
    state.calculate_orders = Arc::new(CalculateOrderStore::new(test_calculate_order_store_path()));
    let admin_port = Arc::new(FakeAdminReadPort);
    let admin_state_port = Arc::new(FakeAdminStatePort::new());
    state.admin = AdminService::new(&state.config)
        .with_read_port(admin_port.clone())
        .with_write_port(admin_port.clone())
        .with_state_port(admin_state_port.clone());
    state.returned_paint = ReturnedPaintService::new(Arc::new(MemoryReturnedPaintStore::new()));
    let resolver = Arc::new(CanonicalServiceApparatusResolver::new(
        state.apparatus.clone(),
    ));
    state.production_maps =
        ProductionMapService::new(Arc::new(MemoryProductionMapStore::new()), resolver.clone());
    state.warehouses = WarehouseService::new(Arc::new(MemoryWarehouseStore::new()), resolver);
    state.workers = WorkerService::new(Arc::new(MemoryWorkerStore::new()));
    state.system_users = SystemUserService::new(Arc::new(MemorySystemUserStore::new()));
    state.auth = crate::core::auth::service::AuthService::new(&state.config)
        .with_customer_dependencies(admin_port.clone(), admin_state_port.clone())
        .with_supplier_dependencies(admin_port.clone(), admin_state_port.clone())
        .with_material_taminotchi_dependencies(admin_port, admin_state_port.clone())
        .with_worker_dependencies(Arc::new(state.workers.clone()), admin_state_port.clone())
        .with_system_user_dependencies(Arc::new(state.system_users.clone()), admin_state_port);
    state.worker_groups = WorkerGroupService::new(Arc::new(MemoryWorkerGroupStore::new()));
    state.production_orders = Arc::new(NoopMiniOrderSink);
    state
}

fn production_map_service_with_store(
    state: &AppState,
    store: Arc<MemoryProductionMapStore>,
) -> ProductionMapService {
    ProductionMapService::new(
        store,
        Arc::new(CanonicalServiceApparatusResolver::new(
            state.apparatus.clone(),
        )),
    )
}

fn test_calculate_order_store_path() -> std::path::PathBuf {
    let id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!("mini-rs-erp-admin-route-calculate-{id}.sqlite"))
}

async fn session(state: &AppState, role: PrincipalRole) -> String {
    session_for(state, role, "admin").await
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
    static IDEMPOTENCY_SEQUENCE: AtomicUsize = AtomicUsize::new(1);
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(
            "idempotency-key",
            format!(
                "admin-route-test-{}",
                IDEMPOTENCY_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ),
        )
        .body(Body::from(body.to_string()))
        .expect("request")
}

async fn json_body(response: axum::response::Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json")
}

async fn provision_test_qolip(router: &axum::Router, token: &str, order_id: &str) {
    let map_response = router
        .clone()
        .oneshot(request(
            "GET",
            &format!("/v1/mobile/admin/production-maps?id={order_id}"),
            token,
        ))
        .await
        .expect("load map for test qolip");
    assert_eq!(map_response.status(), StatusCode::OK);
    let map_body = json_body(map_response).await;
    let item_code = map_body["map"]["product_code"]
        .as_str()
        .expect("map product code");
    let item_name = map_body["map"]["title"].as_str().expect("map title");
    let qolip_code = test_qolip_code(order_id);

    let spec = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/product-specs",
            token,
            &serde_json::json!({
                "item_code": item_code,
                "item_name": item_name,
                "item_group": "Tayyor mahsulot Test",
                "qolip_code": qolip_code,
                "size": 42,
            })
            .to_string(),
        ))
        .await
        .expect("save test qolip spec");
    assert_eq!(spec.status(), StatusCode::OK);

    let location = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/locations",
            token,
            &serde_json::json!({
                "block": "TEST",
                "warehouse": "Qolip ombor",
                "item_code": item_code,
                "item_name": item_name,
                "qolip_code": test_qolip_code(order_id),
                "size": 42,
                "quantity": 100,
            })
            .to_string(),
        ))
        .await
        .expect("save test qolip location");
    assert_eq!(location.status(), StatusCode::OK);
}

fn test_qolip_code(order_id: &str) -> String {
    format!("TEST-QOLIP-{order_id}")
}

fn with_test_qolip(body: &str, order_id: &str) -> String {
    let mut payload: serde_json::Value = serde_json::from_str(body).expect("queue action json");
    payload["qolip_code"] = serde_json::Value::String(test_qolip_code(order_id));
    payload.to_string()
}

fn with_test_returned_paint(body: &str) -> String {
    let mut payload: serde_json::Value = serde_json::from_str(body).expect("queue action json");
    payload["returned_paint_items"] = serde_json::json!([
        {
            "usage": "rasxot",
            "category": "colors",
            "name": "Oq",
            "values": {"Mix": 3, "Oq": 1, "Qora": 0}
        },
        {
            "usage": "astatka",
            "category": "colors",
            "name": "Oq",
            "values": {"Mix": 1, "Oq": 0, "Qora": 0}
        }
    ]);
    payload.to_string()
}

fn entry(ref_: &str, name: &str, phone: &str) -> AdminDirectoryEntry {
    AdminDirectoryEntry {
        ref_: ref_.to_string(),
        name: name.to_string(),
        phone: phone.to_string(),
    }
}

fn item(code: &str) -> SupplierItem {
    SupplierItem {
        code: code.to_string(),
        name: match code {
            "INK-BLACK" => "Black ink".to_string(),
            "INK-WHITE" => "White ink".to_string(),
            "ROLL-980" => "CPP 980/35".to_string(),
            "ROLL-1000" => "CPP 1000/35".to_string(),
            "ROLL-1020" => "CPP 1020/35".to_string(),
            _ => "Rice".to_string(),
        },
        uom: "Kg".to_string(),
        warehouse: "Stores - CH".to_string(),
        item_group: match code {
            "INK-BLACK" | "INK-WHITE" => "Kraska".to_string(),
            "ROLL-980" | "ROLL-1000" | "ROLL-1020" => "Rulon eni".to_string(),
            _ => "Products".to_string(),
        },
        customer_names: if code == "ITEM-001" {
            vec!["Customer One".to_string()]
        } else {
            Vec::new()
        },
    }
}
