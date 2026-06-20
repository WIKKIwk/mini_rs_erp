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
use crate::core::admin::models::{AdminDirectoryEntry, AdminItemGroup, AdminState};
use crate::core::admin::ports::{AdminPortError, AdminReadPort, AdminStatePort, AdminWritePort};
use crate::core::admin::service::AdminService;
use crate::core::apparatus_groups::{ApparatusGroupService, MemoryApparatusGroupStore};
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::auth::ports::{
    AdminAccessState, AdminAccessStateLookup, AuthPortError, CustomerLookup, CustomerRecord,
    SupplierLookup, SupplierRecord,
};
use crate::core::authz::{
    MemoryRoleDefinitionStore, RoleAssignment, RoleDefinition, RoleDefinitionStorePort,
};
use crate::core::calculate_orders::{
    CalculateOrderError, CalculateOrderImage, CalculateOrderStorePort, CalculateOrderTemplate,
};
use crate::core::gscale::GscaleService;
use crate::core::gscale::models::{
    CreateMaterialReceiptDraftInput, MaterialReceiptDraft, RawMaterialStockEntry,
    ScaleDriverPrintRequest, ScaleDriverPrintResponse,
};
use crate::core::gscale::ports::{GscalePortError, MaterialReceiptStorePort, ScaleDriverPort};
use crate::core::mini_orders::{MiniOrderError, MiniOrderSink, NoopMiniOrderSink};
use crate::core::production_map::{MemoryProductionMapStore, ProductionMapService};
use crate::core::session::manager::SessionManager;
use crate::core::warehouses::{MemoryWarehouseStore, WarehouseService};
use crate::core::werka::models::{DispatchRecord, SupplierItem};
use crate::core::werka::ports::{WerkaHomeLookup, WerkaPortError};
use crate::core::werka::service::WerkaService;
use crate::core::worker_groups::{MemoryWorkerGroupStore, WorkerGroupService};
use crate::core::workers::{MemoryWorkerStore, WorkerService};
use crate::store::calculate_order_store::CalculateOrderStore;

mod admin_edge_cases;
mod auth_roles;
mod batch_move_advanced;
mod batch_move_basic;
mod completion_rejections;
mod completion_requests;
mod fakes;
mod item_groups;
mod production_map_basic;
mod production_map_save_order;
mod production_map_validation;
mod queue_history;
mod queue_progress;
mod raw_materials;
mod run_capabilities;
mod suppliers_customers;
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
    pechat_order_map_json_with_dims(id, title, order_number, apparatus, 7.0, 1250.0)
}

fn two_apparatus_order_map_json(
    id: &str,
    title: &str,
    order_number: &str,
    first_apparatus: &str,
    second_apparatus: &str,
) -> String {
    format!(
        r#"{{
            "id":"{id}",
            "product_code":"PECHAT-{order_number}",
            "title":"{title}",
            "order_number":"{order_number}",
            "nodes":[
                {{"id":"start","kind":"start","title":"Start"}},
                {{"id":"first","kind":"apparatus","title":"{first_apparatus}"}},
                {{"id":"second","kind":"apparatus","title":"{second_apparatus}"}},
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
    roll_count: f64,
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
    roll_count: f64,
    width_mm: f64,
) -> String {
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
                {{"id":"apparatus","kind":"apparatus","title":"{apparatus}"}},
                {{"id":"end","kind":"end","title":"End"}}
            ],
            "edges":[
                {{"from":"start","to":"apparatus"}},
                {{"from":"apparatus","to":"end"}}
            ]
        }}"#
    )
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
                {{"id":"laminatsiya","kind":"task","title":"Laminatsiya - A"}},
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
    state.production_maps = ProductionMapService::new(Arc::new(MemoryProductionMapStore::new()));
    state.apparatus_groups = ApparatusGroupService::new(Arc::new(MemoryApparatusGroupStore::new()));
    state.warehouses = WarehouseService::new(Arc::new(MemoryWarehouseStore::new()));
    state.workers = WorkerService::new(Arc::new(MemoryWorkerStore::new()));
    state.auth = crate::core::auth::service::AuthService::new(&state.config)
        .with_customer_dependencies(admin_port.clone(), admin_state_port.clone())
        .with_supplier_dependencies(admin_port, admin_state_port.clone())
        .with_worker_dependencies(Arc::new(state.workers.clone()), admin_state_port);
    state.worker_groups = WorkerGroupService::new(Arc::new(MemoryWorkerGroupStore::new()));
    state.production_orders = Arc::new(NoopMiniOrderSink);
    state
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
        .expect("request")
}

async fn json_body(response: axum::response::Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json")
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
    }
}
