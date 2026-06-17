use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use crate::ai::werka_search::WerkaAiSearchService;
use crate::config::{AppConfig, DotEnvPersister};
use crate::core::admin::models::{AdminDirectoryEntry, AdminItemGroup, AdminWarehouse};
use crate::core::admin::ports::{AdminPortError, AdminReadPort, AdminWritePort};
use crate::core::admin::service::AdminService;
use crate::core::apparatus_groups::ApparatusGroupService;
use crate::core::auth::service::AuthService;
use crate::core::calculate_orders::CalculateOrderStorePort;
use crate::core::customer::service::CustomerService;
use crate::core::gscale::GscaleService;
use crate::core::mini_orders::{MiniOrderSink, NoopMiniOrderSink};
use crate::core::production_map::{
    ApparatusMaterialRule, ApparatusQueueActionEvent, ApparatusQueuePolicy,
    ProductionMapDefinition, ProductionMapError, ProductionMapService, ProductionMapStorePort,
    QueueActionActor, RawMaterialAssignment,
};
use crate::core::profile::ports::ProfileStorePort;
use crate::core::profile::service::ProfileService;
use crate::core::push::ports::PushTokenStorePort;
use crate::core::push::service::PushService;
use crate::core::rezka::RezkaService;
use crate::core::rps_batch::ports::RpsBatchStorePort;
use crate::core::rps_batch::{RpsBatchLmdbStore, RpsBatchService};
use crate::core::session::manager::SessionManager;
use crate::core::warehouse_events::WarehouseEventHub;
use crate::core::warehouses::WarehouseService;
use crate::core::werka::models::SupplierItem;
use crate::core::werka::service::WerkaService;
use crate::core::worker_groups::WorkerGroupService;
use crate::core::workers::WorkerService;
use crate::db::postgres::PostgresConfig;
use crate::db::postgres_admin_catalog::PostgresAdminCatalogStore;
use crate::db::postgres_apparatus_group::PostgresApparatusGroupStore;
use crate::db::postgres_calculate_order::PostgresCalculateOrderStore;
use crate::db::postgres_engine::PostgresEngineStore;
use crate::db::postgres_gscale_receipt::PostgresGscaleReceiptStore;
use crate::db::postgres_mini_order::PostgresMiniOrderSink;
use crate::db::postgres_production_map::PostgresProductionMapStore;
use crate::db::postgres_push_token::PostgresPushTokenStore;
use crate::db::postgres_rps_batch::PostgresRpsBatchStore;
use crate::db::postgres_warehouse::PostgresWarehouseStore;
use crate::db::postgres_worker::PostgresWorkerStore;
use crate::db::postgres_worker_group::PostgresWorkerGroupStore;
use crate::fcm::discover_push_sender;
use crate::google_sheets::{OrderSheetSink, discover_order_sheet_sink};
use crate::rps::RpsDriverClient;
use crate::store::admin_store::JsonAdminStore;
use crate::store::apparatus_group_store::ApparatusGroupStore;
use crate::store::calculate_order_store::CalculateOrderStore;
use crate::store::profile_store::{LmdbProfileStore, ProfileStore};
use crate::store::push_token_store::{LmdbPushTokenStore, PushTokenStore};
use crate::store::role_store::RoleDefinitionStore;
use async_trait::async_trait;
use tokio::time::sleep;

#[path = "app_local_store.rs"]
mod app_local_store;
use app_local_store::*;

#[derive(Clone)]
pub struct AppState {
    #[cfg_attr(not(test), allow(dead_code))]
    pub config: Arc<AppConfig>,
    pub admin: AdminService,
    pub auth: AuthService,
    pub customer: CustomerService,
    pub profiles: ProfileService,
    pub production_maps: ProductionMapService,
    pub apparatus_groups: ApparatusGroupService,
    pub calculate_orders: Arc<dyn CalculateOrderStorePort>,
    pub order_sheets: Arc<dyn OrderSheetSink>,
    pub production_orders: Arc<dyn MiniOrderSink>,
    pub calculate_order_image_dir: Arc<std::path::PathBuf>,
    pub push: PushService,
    pub gscale: GscaleService,
    pub rezka: RezkaService,
    pub rps_batch: RpsBatchService,
    pub werka: WerkaService,
    pub warehouses: WarehouseService,
    pub workers: WorkerService,
    pub worker_groups: WorkerGroupService,
    pub sessions: SessionManager,
    pub warehouse_events: WarehouseEventHub,
    #[allow(dead_code)]
    pub mini_engine: Option<PostgresEngineStore>,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        let admin_store = Arc::new(JsonAdminStore::new(admin_store_path()));
        let workers = build_worker_service();
        let auth = AuthService::new(&config)
            .with_supplier_dependencies(admin_store.clone(), admin_store.clone())
            .with_customer_dependencies(admin_store.clone(), admin_store.clone())
            .with_worker_dependencies(Arc::new(workers.clone()), admin_store.clone());
        let mut admin =
            AdminService::new(&config).with_env_persister(Arc::new(DotEnvPersister::new(".env")));
        let (admin_read_port, admin_write_port) = build_admin_catalog_ports(admin_store.clone());
        admin = admin
            .with_read_port(admin_read_port)
            .with_write_port(admin_write_port)
            .with_state_port(admin_store.clone());
        admin = admin.with_role_store(Arc::new(RoleDefinitionStore::new(role_store_path())));
        admin = admin.with_auth_config_sink(Arc::new(auth.clone()));
        let customer = CustomerService::new();
        let profile_store = build_profile_store(&config);
        let production_maps = build_production_map_service();
        let apparatus_groups = build_apparatus_groups_service();
        let calculate_orders = build_calculate_order_store();
        let order_sheets = discover_order_sheet_sink();
        let production_orders = build_mini_order_sink();
        let mini_engine = build_mini_engine_store();
        if order_sheets.enabled() {
            tokio::spawn(run_order_sheets_sync_loop(
                production_maps.clone(),
                calculate_orders.clone(),
                order_sheets.clone(),
                order_sheets_sync_interval(),
            ));
        }
        let calculate_order_image_dir = Arc::new(calculate_order_image_dir());
        let push_token_store = build_push_token_store(&config);
        let profiles = ProfileService::new(String::new()).with_store(profile_store);
        let push = PushService::new(push_token_store.clone())
            .with_sender(discover_push_sender(push_token_store));
        let rps_batch = RpsBatchService::new(build_rps_batch_store());
        let scale_driver = Arc::new(RpsDriverClient::new(
            config.http_timeout,
            std::env::var("RP_SCALE_DRIVER_URL").unwrap_or_default(),
        ));
        let warehouse_events = WarehouseEventHub::new();
        let gscale = build_gscale_service(scale_driver.clone(), warehouse_events.clone());
        let rezka = RezkaService::new()
            .with_driver(scale_driver)
            .with_epc_source(Arc::new(crate::core::gscale::epc::GscaleEpcGenerator::new()));
        let mut werka = WerkaService::new();
        let warehouses = build_warehouse_service();
        let worker_groups = build_worker_group_service();
        let sessions = match local_store_backend("MOBILE_API_SESSION_STORE_BACKEND") {
            LocalStoreBackend::Lmdb => {
                let lmdb_path = session_lmdb_path(&config);
                match SessionManager::lmdb(
                    lmdb_path.clone(),
                    local_lmdb_map_size_bytes("MOBILE_API_SESSION_LMDB_MAP_SIZE_MB"),
                    config.session_ttl_seconds,
                ) {
                    Ok(sessions) => {
                        tracing::info!(
                            path = %lmdb_path.display(),
                            "LMDB session store enabled"
                        );
                        sessions
                    }
                    Err(error) => {
                        if allow_json_fallback() {
                            tracing::warn!(
                                %error,
                                "LMDB session store unavailable; falling back to JSON session store"
                            );
                            SessionManager::persistent(
                                config.session_store_path.clone(),
                                config.session_ttl_seconds,
                            )
                        } else {
                            panic!("LMDB session store unavailable: {error}");
                        }
                    }
                }
            }
            LocalStoreBackend::Json => SessionManager::persistent(
                config.session_store_path.clone(),
                config.session_ttl_seconds,
            ),
        };
        let ai_key = std::env::var("GEMINI_API_KEY").unwrap_or_default();
        if !ai_key.trim().is_empty() {
            werka = werka.with_ai_search(Arc::new(WerkaAiSearchService::new(
                &ai_key,
                &std::env::var("GEMINI_VISION_MODEL").unwrap_or_default(),
                config.http_timeout,
            )));
        }

        Self {
            config: Arc::new(config),
            admin,
            auth,
            customer,
            profiles,
            production_maps,
            apparatus_groups,
            calculate_orders,
            order_sheets,
            production_orders,
            calculate_order_image_dir,
            push,
            gscale,
            rezka,
            rps_batch,
            werka,
            warehouses,
            workers,
            worker_groups,
            sessions,
            warehouse_events,
            mini_engine,
        }
    }
}

fn build_admin_catalog_ports(
    fallback: Arc<JsonAdminStore>,
) -> (Arc<dyn AdminReadPort>, Arc<dyn AdminWritePort>) {
    let config = match PostgresConfig::from_env() {
        Ok(config) => config,
        Err(_) => return (fallback.clone(), fallback),
    };
    match config.pool_options().connect_lazy(&config.database_url) {
        Ok(pool) => {
            tracing::info!("mini ERP postgres item catalog store configured");
            let catalog = Arc::new(PostgresAdminCatalogStore::new(pool));
            spawn_admin_catalog_seed(catalog.clone(), fallback.clone());
            let overlay = Arc::new(AdminCatalogOverlay { fallback, catalog });
            (
                overlay.clone() as Arc<dyn AdminReadPort>,
                overlay as Arc<dyn AdminWritePort>,
            )
        }
        Err(error) => {
            tracing::warn!(%error, "mini ERP postgres item catalog store disabled");
            (fallback.clone(), fallback)
        }
    }
}

fn spawn_admin_catalog_seed(
    catalog: Arc<PostgresAdminCatalogStore>,
    fallback: Arc<JsonAdminStore>,
) {
    tokio::spawn(async move {
        if let Err(error) = catalog.seed_from_read_port(fallback.as_ref()).await {
            tracing::warn!(%error, "mini ERP postgres item catalog seed failed");
        }
    });
}

struct AdminCatalogOverlay {
    fallback: Arc<JsonAdminStore>,
    catalog: Arc<PostgresAdminCatalogStore>,
}

#[async_trait]
impl AdminReadPort for AdminCatalogOverlay {
    async fn suppliers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        self.fallback.suppliers_page(query, limit, offset).await
    }

    async fn supplier_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        self.fallback.supplier_by_ref(ref_).await
    }

    async fn customers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        self.fallback.customers_page(query, limit, offset).await
    }

    async fn customer_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        self.fallback.customer_by_ref(ref_).await
    }

    async fn items_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        self.catalog.items_page(query, limit, offset).await
    }

    async fn items_page_by_group(
        &self,
        group: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        self.catalog
            .items_page_by_group(group, query, limit, offset)
            .await
    }

    async fn items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        self.catalog.items_by_codes(item_codes).await
    }

    async fn item_groups(&self, query: &str, limit: usize) -> Result<Vec<String>, AdminPortError> {
        self.catalog.item_groups(query, limit).await
    }

    async fn warehouses(
        &self,
        query: &str,
        parent: &str,
        limit: usize,
    ) -> Result<Vec<AdminWarehouse>, AdminPortError> {
        if parent.trim().is_empty() {
            self.catalog.warehouses(query, parent, limit).await
        } else {
            self.fallback.warehouses(query, parent, limit).await
        }
    }

    async fn item_group_tree(&self) -> Result<Vec<AdminItemGroup>, AdminPortError> {
        self.catalog.item_group_tree().await
    }

    async fn assigned_supplier_items(
        &self,
        supplier_ref: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        self.fallback
            .assigned_supplier_items(supplier_ref, limit)
            .await
    }

    async fn customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        self.fallback
            .customer_items(customer_ref, query, limit)
            .await
    }
}

#[async_trait]
impl AdminWritePort for AdminCatalogOverlay {
    async fn create_supplier(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        self.fallback.create_supplier(name, phone).await
    }

    async fn update_supplier_phone(&self, ref_: &str, phone: &str) -> Result<(), AdminPortError> {
        self.fallback.update_supplier_phone(ref_, phone).await
    }

    async fn assign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        self.fallback.assign_supplier_item(ref_, item_code).await
    }

    async fn unassign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        self.fallback.unassign_supplier_item(ref_, item_code).await
    }

    async fn create_customer(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        self.fallback.create_customer(name, phone).await
    }

    async fn update_customer_phone(&self, ref_: &str, phone: &str) -> Result<(), AdminPortError> {
        self.fallback.update_customer_phone(ref_, phone).await
    }

    async fn update_customer_code(&self, ref_: &str, code: &str) -> Result<(), AdminPortError> {
        self.fallback.update_customer_code(ref_, code).await
    }

    async fn assign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        self.fallback.assign_customer_item(ref_, item_code).await
    }

    async fn unassign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        self.fallback.unassign_customer_item(ref_, item_code).await
    }

    async fn create_item(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
    ) -> Result<SupplierItem, AdminPortError> {
        self.catalog.create_item(code, name, uom, item_group).await
    }

    async fn create_item_group(
        &self,
        name: &str,
        parent: &str,
        is_group: bool,
    ) -> Result<AdminItemGroup, AdminPortError> {
        self.catalog.create_item_group(name, parent, is_group).await
    }

    async fn move_item_group_parent(
        &self,
        name: &str,
        parent: &str,
    ) -> Result<AdminItemGroup, AdminPortError> {
        self.catalog.move_item_group_parent(name, parent).await
    }

    async fn update_item_group(
        &self,
        item_code: &str,
        item_group: &str,
    ) -> Result<(), AdminPortError> {
        self.catalog.update_item_group(item_code, item_group).await
    }
}

fn build_gscale_service(
    scale_driver: Arc<RpsDriverClient>,
    warehouse_events: WarehouseEventHub,
) -> GscaleService {
    let events = warehouse_events.clone();
    let service = GscaleService::new()
        .with_driver(scale_driver)
        .with_warehouse_event_handler(Arc::new(move |warehouse, reason| {
            events.notify_updated(&warehouse, &reason);
        }));
    let config = match PostgresConfig::from_env() {
        Ok(config) => config,
        Err(_) => return service,
    };
    match config.pool_options().connect_lazy(&config.database_url) {
        Ok(pool) => {
            tracing::info!("mini ERP postgres GScale receipt store configured");
            service.with_receipt_store(Arc::new(PostgresGscaleReceiptStore::new(pool)))
        }
        Err(error) => {
            tracing::warn!(%error, "mini ERP postgres GScale receipt store disabled");
            service
        }
    }
}

fn build_warehouse_service() -> WarehouseService {
    let config = match PostgresConfig::from_env() {
        Ok(config) => config,
        Err(_) => {
            return WarehouseService::new(Arc::new(
                crate::core::warehouses::MemoryWarehouseStore::new(),
            ));
        }
    };
    match config.pool_options().connect_lazy(&config.database_url) {
        Ok(pool) => {
            tracing::info!("mini ERP postgres warehouse store configured");
            WarehouseService::new(Arc::new(PostgresWarehouseStore::new(pool)))
        }
        Err(error) => {
            tracing::warn!(%error, "mini ERP postgres warehouse store disabled");
            WarehouseService::new(Arc::new(
                crate::core::warehouses::MemoryWarehouseStore::new(),
            ))
        }
    }
}

fn build_worker_group_service() -> WorkerGroupService {
    let config = match PostgresConfig::from_env() {
        Ok(config) => config,
        Err(_) => return WorkerGroupService::unavailable(),
    };
    match config.pool_options().connect_lazy(&config.database_url) {
        Ok(pool) => {
            tracing::info!("mini ERP postgres worker group store configured");
            WorkerGroupService::new(Arc::new(PostgresWorkerGroupStore::new(pool)))
        }
        Err(error) => {
            tracing::warn!(%error, "mini ERP postgres worker group store disabled");
            WorkerGroupService::unavailable()
        }
    }
}

fn build_worker_service() -> WorkerService {
    let config = match PostgresConfig::from_env() {
        Ok(config) => config,
        Err(_) => return WorkerService::unavailable(),
    };
    match config.pool_options().connect_lazy(&config.database_url) {
        Ok(pool) => {
            tracing::info!("mini ERP postgres worker store configured");
            WorkerService::new(Arc::new(PostgresWorkerStore::new(pool)))
        }
        Err(error) => {
            tracing::warn!(%error, "mini ERP postgres worker store disabled");
            WorkerService::unavailable()
        }
    }
}

fn build_mini_engine_store() -> Option<PostgresEngineStore> {
    let config = match PostgresConfig::from_env() {
        Ok(config) => config,
        Err(_) => return None,
    };
    match config.pool_options().connect_lazy(&config.database_url) {
        Ok(pool) => {
            tracing::info!("mini ERP postgres engine store configured");
            Some(PostgresEngineStore::new(pool))
        }
        Err(error) => {
            tracing::warn!(%error, "mini ERP postgres engine store disabled");
            None
        }
    }
}

fn build_mini_order_sink() -> Arc<dyn MiniOrderSink> {
    let config = match PostgresConfig::from_env() {
        Ok(config) => config,
        Err(_) => return Arc::new(NoopMiniOrderSink),
    };
    match config.pool_options().connect_lazy(&config.database_url) {
        Ok(pool) => {
            tracing::info!("mini ERP postgres order sink configured");
            Arc::new(PostgresMiniOrderSink::new(pool))
        }
        Err(error) => {
            tracing::warn!(%error, "mini ERP postgres order sink disabled");
            Arc::new(NoopMiniOrderSink)
        }
    }
}

fn build_production_map_service() -> ProductionMapService {
    let config = match PostgresConfig::from_env() {
        Ok(config) => config,
        Err(error) => {
            tracing::warn!(?error, "mini ERP postgres production map store unavailable");
            return ProductionMapService::new(Arc::new(UnavailableProductionMapStore));
        }
    };
    match config.pool_options().connect_lazy(&config.database_url) {
        Ok(pool) => {
            tracing::info!("mini ERP postgres production map store configured");
            ProductionMapService::new(Arc::new(PostgresProductionMapStore::new(pool)))
        }
        Err(error) => {
            tracing::warn!(%error, "mini ERP postgres production map store unavailable");
            ProductionMapService::new(Arc::new(UnavailableProductionMapStore))
        }
    }
}

struct UnavailableProductionMapStore;

#[async_trait]
impl ProductionMapStorePort for UnavailableProductionMapStore {
    async fn maps(&self) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn put_map(&self, _map: ProductionMapDefinition) -> Result<(), ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn put_maps_batch(
        &self,
        _maps: &[ProductionMapDefinition],
    ) -> Result<(), ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn delete_map(&self, _map_id: &str) -> Result<(), ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn apparatus_sequences(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn put_apparatus_sequence(
        &self,
        _apparatus: &str,
        _order_ids: Vec<String>,
    ) -> Result<(), ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn apparatus_queue_states(
        &self,
    ) -> Result<BTreeMap<String, BTreeMap<String, String>>, ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn put_apparatus_queue_states(
        &self,
        _apparatus: &str,
        _states: BTreeMap<String, String>,
    ) -> Result<(), ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn apparatus_queue_policies(
        &self,
    ) -> Result<BTreeMap<String, ApparatusQueuePolicy>, ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn put_apparatus_queue_policy(
        &self,
        _apparatus: &str,
        _policy: ApparatusQueuePolicy,
        _actor: &QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn append_apparatus_queue_action_event(
        &self,
        _event: ApparatusQueueActionEvent,
    ) -> Result<(), ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn apparatus_material_rules(
        &self,
    ) -> Result<Vec<ApparatusMaterialRule>, ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn put_apparatus_material_rule(
        &self,
        _rule: ApparatusMaterialRule,
    ) -> Result<(), ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn raw_material_assignments(
        &self,
    ) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn put_raw_material_assignment(
        &self,
        _assignment: RawMaterialAssignment,
    ) -> Result<(), ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }
}

fn build_apparatus_groups_service() -> ApparatusGroupService {
    let config = match PostgresConfig::from_env() {
        Ok(config) => config,
        Err(_) => return build_sqlite_apparatus_groups_service(),
    };
    match config.pool_options().connect_lazy(&config.database_url) {
        Ok(pool) => {
            tracing::info!("mini ERP postgres apparatus group store configured");
            ApparatusGroupService::new(Arc::new(PostgresApparatusGroupStore::new(pool)))
        }
        Err(error) => {
            tracing::warn!(%error, "mini ERP postgres apparatus group store disabled");
            build_sqlite_apparatus_groups_service()
        }
    }
}

fn build_sqlite_apparatus_groups_service() -> ApparatusGroupService {
    ApparatusGroupService::new(Arc::new(ApparatusGroupStore::new(
        apparatus_group_store_path(),
    )))
}

fn build_calculate_order_store() -> Arc<dyn CalculateOrderStorePort> {
    let config = match PostgresConfig::from_env() {
        Ok(config) => config,
        Err(_) => return build_sqlite_calculate_order_store(),
    };
    match config.pool_options().connect_lazy(&config.database_url) {
        Ok(pool) => {
            tracing::info!("mini ERP postgres calculate order store configured");
            Arc::new(PostgresCalculateOrderStore::new(pool))
        }
        Err(error) => {
            tracing::warn!(%error, "mini ERP postgres calculate order store disabled");
            build_sqlite_calculate_order_store()
        }
    }
}

fn build_sqlite_calculate_order_store() -> Arc<dyn CalculateOrderStorePort> {
    Arc::new(CalculateOrderStore::new(calculate_order_store_path()))
}

async fn run_order_sheets_sync_loop(
    production_maps: ProductionMapService,
    calculate_orders: Arc<dyn CalculateOrderStorePort>,
    order_sheets: Arc<dyn OrderSheetSink>,
    interval: Duration,
) {
    loop {
        match sync_order_sheets_once(
            production_maps.clone(),
            calculate_orders.clone(),
            order_sheets.clone(),
        )
        .await
        {
            Ok(appended) => {
                tracing::info!(appended, "google sheets order sync completed");
            }
            Err(error) => {
                tracing::warn!(%error, "google sheets order sync failed");
            }
        }
        if interval.is_zero() {
            break;
        }
        sleep(interval).await;
    }
}

async fn sync_order_sheets_once(
    production_maps: ProductionMapService,
    calculate_orders: Arc<dyn CalculateOrderStorePort>,
    order_sheets: Arc<dyn OrderSheetSink>,
) -> Result<usize, String> {
    let maps = production_maps
        .maps()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|saved| saved.map)
        .collect::<Vec<_>>();
    let templates = calculate_orders
        .list_all()
        .await
        .map_err(|error| error.to_string())?;
    order_sheets
        .sync_orders(&maps, &templates)
        .await
        .map_err(|error| error.to_string())
}

fn order_sheets_sync_interval() -> Duration {
    let seconds = std::env::var("GOOGLE_SHEETS_ORDER_SYNC_INTERVAL_SECONDS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(60 * 60);
    Duration::from_secs(seconds)
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
