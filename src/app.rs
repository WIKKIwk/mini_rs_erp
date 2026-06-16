use std::sync::Arc;
use std::time::Duration;

use crate::ai::werka_search::WerkaAiSearchService;
use crate::config::{AppConfig, DotEnvPersister};
use crate::core::admin::service::AdminService;
use crate::core::apparatus_groups::ApparatusGroupService;
use crate::core::auth::service::AuthService;
use crate::core::calculate_orders::CalculateOrderStorePort;
use crate::core::customer::service::CustomerService;
use crate::core::gscale::GscaleService;
use crate::core::mini_orders::{MiniOrderSink, NoopMiniOrderSink};
use crate::core::production_map::ProductionMapService;
use crate::core::profile::ports::ProfileStorePort;
use crate::core::profile::service::ProfileService;
use crate::core::push::ports::PushTokenStorePort;
use crate::core::push::service::PushService;
use crate::core::rezka::RezkaService;
use crate::core::rps_batch::{RpsBatchLmdbStore, RpsBatchService};
use crate::core::session::manager::SessionManager;
use crate::core::werka::service::WerkaService;
use crate::core::worker_groups::WorkerGroupService;
use crate::core::workers::WorkerService;
use crate::db::postgres::PostgresConfig;
use crate::db::postgres_apparatus_group::PostgresApparatusGroupStore;
use crate::db::postgres_calculate_order::PostgresCalculateOrderStore;
use crate::db::postgres_engine::PostgresEngineStore;
use crate::db::postgres_mini_order::PostgresMiniOrderSink;
use crate::db::postgres_production_map::PostgresProductionMapStore;
use crate::db::postgres_worker::PostgresWorkerStore;
use crate::db::postgres_worker_group::PostgresWorkerGroupStore;
use crate::fcm::discover_push_sender;
use crate::google_sheets::{OrderSheetSink, discover_order_sheet_sink};
use crate::rps::RpsDriverClient;
use crate::store::admin_store::JsonAdminStore;
use crate::store::apparatus_group_store::ApparatusGroupStore;
use crate::store::calculate_order_store::CalculateOrderStore;
use crate::store::production_map_store::ProductionMapStore;
use crate::store::profile_store::{LmdbProfileStore, ProfileStore};
use crate::store::push_token_store::{LmdbPushTokenStore, PushTokenStore};
use crate::store::role_store::RoleDefinitionStore;
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
    pub workers: WorkerService,
    pub worker_groups: WorkerGroupService,
    pub sessions: SessionManager,
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
        admin = admin
            .with_read_port(admin_store.clone())
            .with_write_port(admin_store.clone())
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
        let rps_batch = RpsBatchService::new(Arc::new(build_rps_batch_store()));
        let scale_driver = Arc::new(RpsDriverClient::new(
            config.http_timeout,
            std::env::var("RP_SCALE_DRIVER_URL").unwrap_or_default(),
        ));
        let gscale = GscaleService::new().with_driver(scale_driver.clone());
        let rezka = RezkaService::new()
            .with_driver(scale_driver)
            .with_epc_source(Arc::new(crate::core::gscale::epc::GscaleEpcGenerator::new()));
        let mut werka = WerkaService::new();
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
            workers,
            worker_groups,
            sessions,
            mini_engine,
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
        Err(_) => return build_sqlite_production_map_service(),
    };
    match config.pool_options().connect_lazy(&config.database_url) {
        Ok(pool) => {
            tracing::info!("mini ERP postgres production map store configured");
            ProductionMapService::new(Arc::new(PostgresProductionMapStore::new(pool)))
        }
        Err(error) => {
            tracing::warn!(%error, "mini ERP postgres production map store disabled");
            build_sqlite_production_map_service()
        }
    }
}

fn build_sqlite_production_map_service() -> ProductionMapService {
    ProductionMapService::new(Arc::new(ProductionMapStore::new(product_map_store_path())))
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
