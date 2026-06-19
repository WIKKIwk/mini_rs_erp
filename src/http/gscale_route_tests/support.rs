use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::http::{Request, header};

use crate::app::AppState;
use crate::config::AppConfig;
use crate::core::admin::models::{AdminDirectoryEntry, AdminItemGroup};
use crate::core::admin::ports::{AdminPortError, AdminReadPort};
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::gscale::models::{
    CreateMaterialReceiptDraftInput, MaterialReceiptDraft, ScaleDriverPrintRequest,
    ScaleDriverPrintResponse,
};
use crate::core::gscale::ports::{
    EpcSource, GscalePortError, MaterialReceiptStorePort, ScaleDriverPort,
};
use crate::core::rps_batch::RpsBatchService;
use crate::core::rps_batch::models::RpsBatchSession;
use crate::core::rps_batch::ports::{RpsBatchStoreError, RpsBatchStorePort};
use crate::core::session::manager::SessionManager;
use crate::core::werka::models::SupplierItem;

pub(super) fn test_state() -> AppState {
    let mut state = AppState::new(AppConfig {
        bind_addr: "127.0.0.1:8081".parse().expect("addr"),
        default_target_warehouse: String::new(),
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
    state.rps_batch = RpsBatchService::new(Arc::new(MemoryRpsBatchStore::default()));
    state
}

pub(super) async fn session(state: &AppState, role: PrincipalRole) -> String {
    state
        .sessions
        .create(Principal {
            role,
            display_name: "Admin".to_string(),
            legal_name: "Admin".to_string(),
            ref_: "admin".to_string(),
            phone: "+998880000000".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("session")
}

pub(super) fn request(method: &str, uri: &str, token: &str, body: &str) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if !token.trim().is_empty() {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("request")
}

pub(super) async fn json_body(response: axum::response::Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json")
}

pub(super) struct FakeReceiptStore {
    pub(super) events: Arc<Mutex<Vec<String>>>,
}

#[derive(Default)]
struct MemoryRpsBatchStore {
    batches: Mutex<BTreeMap<String, RpsBatchSession>>,
}

#[async_trait]
impl RpsBatchStorePort for MemoryRpsBatchStore {
    async fn get(&self, owner_key: &str) -> Result<Option<RpsBatchSession>, RpsBatchStoreError> {
        Ok(self.batches.lock().unwrap().get(owner_key.trim()).cloned())
    }

    async fn put(&self, batch: RpsBatchSession) -> Result<(), RpsBatchStoreError> {
        self.batches
            .lock()
            .unwrap()
            .insert(batch.owner_key.trim().to_string(), batch);
        Ok(())
    }
}

pub(super) struct FakeAdminCatalogReadPort;

#[async_trait]
impl AdminReadPort for FakeAdminCatalogReadPort {
    async fn suppliers_page(
        &self,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        Ok(Vec::new())
    }

    async fn supplier_by_ref(&self, _ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        Err(AdminPortError::NotFound)
    }

    async fn customers_page(
        &self,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        Ok(Vec::new())
    }

    async fn customer_by_ref(&self, _ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        Err(AdminPortError::NotFound)
    }

    async fn items_page(
        &self,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Ok(Vec::new())
    }

    async fn items_page_by_group(
        &self,
        group: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        assert_eq!(group, "Products");
        assert_eq!(query, "film");
        assert_eq!(limit, 20);
        assert_eq!(offset, 0);
        Ok(vec![SupplierItem {
            code: "GSCALE-ITEM-001".to_string(),
            name: "GScale Film".to_string(),
            uom: "Kg".to_string(),
            warehouse: "Stores - A".to_string(),
            item_group: "Products".to_string(),
        }])
    }

    async fn items_by_codes(
        &self,
        _item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Ok(Vec::new())
    }

    async fn item_groups(
        &self,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<String>, AdminPortError> {
        Ok(Vec::new())
    }

    async fn item_group_tree(&self) -> Result<Vec<AdminItemGroup>, AdminPortError> {
        Ok(Vec::new())
    }

    async fn assigned_supplier_items(
        &self,
        _supplier_ref: &str,
        _limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Ok(Vec::new())
    }

    async fn customer_items(
        &self,
        _customer_ref: &str,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl MaterialReceiptStorePort for FakeReceiptStore {
    async fn create_material_receipt_draft(
        &self,
        input: CreateMaterialReceiptDraftInput,
    ) -> Result<MaterialReceiptDraft, GscalePortError> {
        self.events
            .lock()
            .unwrap()
            .push(format!("create:{:.3}", input.qty));
        Ok(MaterialReceiptDraft {
            name: "MAT-STE-ROUTE".to_string(),
            item_code: input.item_code,
            warehouse: input.warehouse,
            qty: input.qty,
            uom: "Kg".to_string(),
            barcode: input.barcode,
        })
    }

    async fn submit_stock_entry_draft(&self, name: &str) -> Result<(), GscalePortError> {
        self.events.lock().unwrap().push(format!("submit:{name}"));
        Ok(())
    }

    async fn delete_stock_entry_draft(&self, name: &str) -> Result<(), GscalePortError> {
        self.events.lock().unwrap().push(format!("delete:{name}"));
        Ok(())
    }
}

pub(super) struct FailingSubmitStore {
    pub(super) events: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl MaterialReceiptStorePort for FailingSubmitStore {
    async fn create_material_receipt_draft(
        &self,
        input: CreateMaterialReceiptDraftInput,
    ) -> Result<MaterialReceiptDraft, GscalePortError> {
        self.events
            .lock()
            .unwrap()
            .push(format!("create:{:.3}", input.qty));
        Ok(MaterialReceiptDraft {
            name: "MAT-STE-ROUTE".to_string(),
            item_code: input.item_code,
            warehouse: input.warehouse,
            qty: input.qty,
            uom: "Kg".to_string(),
            barcode: input.barcode,
        })
    }

    async fn submit_stock_entry_draft(&self, name: &str) -> Result<(), GscalePortError> {
        self.events.lock().unwrap().push(format!("submit:{name}"));
        Err(GscalePortError::StoreWrite(
            "NegativeStockError: insufficient stock".to_string(),
        ))
    }

    async fn delete_stock_entry_draft(&self, name: &str) -> Result<(), GscalePortError> {
        self.events.lock().unwrap().push(format!("delete:{name}"));
        Ok(())
    }
}

pub(super) struct SlowReceiptStore {
    pub(super) events: Arc<Mutex<Vec<String>>>,
    pub(super) delay: Duration,
}

#[async_trait]
impl MaterialReceiptStorePort for SlowReceiptStore {
    async fn create_material_receipt_draft(
        &self,
        input: CreateMaterialReceiptDraftInput,
    ) -> Result<MaterialReceiptDraft, GscalePortError> {
        tokio::time::sleep(self.delay).await;
        self.events
            .lock()
            .unwrap()
            .push(format!("create:{:.3}", input.qty));
        Ok(MaterialReceiptDraft {
            name: "MAT-STE-ROUTE".to_string(),
            item_code: input.item_code,
            warehouse: input.warehouse,
            qty: input.qty,
            uom: "Kg".to_string(),
            barcode: input.barcode,
        })
    }

    async fn submit_stock_entry_draft(&self, name: &str) -> Result<(), GscalePortError> {
        self.events.lock().unwrap().push(format!("submit:{name}"));
        Ok(())
    }

    async fn delete_stock_entry_draft(&self, name: &str) -> Result<(), GscalePortError> {
        self.events.lock().unwrap().push(format!("delete:{name}"));
        Ok(())
    }
}

pub(super) struct FakeDriver {
    pub(super) events: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl ScaleDriverPort for FakeDriver {
    async fn print_material_receipt(
        &self,
        request: ScaleDriverPrintRequest,
    ) -> Result<ScaleDriverPrintResponse, GscalePortError> {
        self.events.lock().unwrap().push("print".to_string());
        Ok(ScaleDriverPrintResponse {
            ok: true,
            status: "done".to_string(),
            epc: request.epc,
            printer: request.printer,
            mode: request.print_mode,
            printer_status: "OK".to_string(),
            ..ScaleDriverPrintResponse::default()
        })
    }
}

pub(super) struct FixedEpc(pub(super) &'static str);

impl EpcSource for FixedEpc {
    fn next_epc(&self) -> String {
        self.0.to_string()
    }
}
