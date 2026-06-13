use std::sync::Arc;
use std::time::Duration;

use crate::ai::werka_search::WerkaAiSearchService;
use crate::config::{AppConfig, DotEnvPersister};
use crate::core::admin::service::AdminService;
use crate::core::apparatus_groups::ApparatusGroupService;
use crate::core::auth::admin_read_lookup::AdminReadAuthLookup;
use crate::core::auth::service::AuthService;
use crate::core::calculate_orders::CalculateOrderStorePort;
use crate::core::customer::service::CustomerService;
use crate::core::gscale::GscaleService;
use crate::core::production_map::ProductionMapService;
use crate::core::profile::ports::ProfileStorePort;
use crate::core::profile::service::ProfileService;
use crate::core::push::ports::PushTokenStorePort;
use crate::core::push::service::PushService;
use crate::core::rezka::RezkaService;
use crate::core::rps_batch::{RpsBatchLmdbStore, RpsBatchService};
use crate::core::session::manager::SessionManager;
use crate::core::werka::service::WerkaService;
use crate::db::postgres::PostgresConfig;
use crate::db::postgres_apparatus_group::PostgresApparatusGroupStore;
use crate::db::postgres_calculate_order::PostgresCalculateOrderStore;
use crate::db::postgres_engine::PostgresEngineStore;
use crate::db::postgres_production_map::PostgresProductionMapStore;
use crate::erpdb::catalog_cache::reader::CatalogCacheReader;
use crate::erpdb::catalog_cache::store::CatalogCacheStore;
use crate::erpdb::catalog_cache::sync::{sync_catalog_delta_once, sync_catalog_once};
use crate::erpdb::reader::DirectDbReader;
use crate::erpnext::client::ErpnextClient;
use crate::erpnext::production_order::{
    NoopProductionOrderErpSink, ProductionOrderErpSink, ProductionOrderErpSource,
};
use crate::fcm::discover_push_sender;
use crate::google_sheets::{OrderSheetSink, discover_order_sheet_sink};
use crate::rps::RpsDriverClient;
use crate::store::admin_state_store::AdminSupplierStateBackend;
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
    pub production_orders: Arc<dyn ProductionOrderErpSink>,
    pub calculate_order_image_dir: Arc<std::path::PathBuf>,
    pub push: PushService,
    pub gscale: GscaleService,
    pub rezka: RezkaService,
    pub rps_batch: RpsBatchService,
    pub werka: WerkaService,
    pub sessions: SessionManager,
    #[allow(dead_code)]
    pub mini_engine: Option<PostgresEngineStore>,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        let mut auth = AuthService::new(&config);
        let mut admin =
            AdminService::new(&config).with_env_persister(Arc::new(DotEnvPersister::new(".env")));
        admin = admin.with_role_store(Arc::new(RoleDefinitionStore::new(role_store_path())));
        admin = admin.with_auth_config_sink(Arc::new(auth.clone()));
        let mut customer = CustomerService::new();
        let profile_store = build_profile_store(&config);
        let production_maps = build_production_map_service();
        let apparatus_groups = build_apparatus_groups_service();
        let calculate_orders = build_calculate_order_store();
        let order_sheets = discover_order_sheet_sink();
        let mut production_orders: Arc<dyn ProductionOrderErpSink> =
            Arc::new(NoopProductionOrderErpSink);
        let mut production_order_source: Option<Arc<dyn ProductionOrderErpSource>> = None;
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
        let mut profiles = ProfileService::new(config.erp_url.clone()).with_store(profile_store);
        let push = PushService::new(push_token_store.clone())
            .with_sender(discover_push_sender(push_token_store));
        let rps_batch = RpsBatchService::new(Arc::new(build_rps_batch_store()));
        let scale_driver = Arc::new(RpsDriverClient::new(
            config.erp_timeout,
            std::env::var("RP_SCALE_DRIVER_URL").unwrap_or_default(),
        ));
        let mut gscale = GscaleService::new().with_driver(scale_driver.clone());
        let mut rezka = RezkaService::new()
            .with_driver(scale_driver)
            .with_epc_source(Arc::new(crate::core::gscale::epc::GscaleEpcGenerator::new()));
        let mut werka = WerkaService::new();
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
                config.erp_timeout,
            )));
        }

        let mut erp_client = None;
        let mut admin_state_store = None;
        if config.erp_configured() {
            let state_store = Arc::new(build_admin_supplier_state_store(&config));
            admin_state_store = Some(state_store.clone());
            let client = Arc::new(
                ErpnextClient::new(
                    config.erp_url.clone(),
                    config.erp_api_key.clone(),
                    config.erp_api_secret.clone(),
                    config.erp_timeout,
                )
                .with_default_warehouse(config.default_target_warehouse.clone()),
            );
            admin = admin
                .with_read_port(client.clone())
                .with_write_port(client.clone())
                .with_erp_config_sink(client.clone())
                .with_state_port(state_store.clone());
            auth = auth.with_supplier_dependencies(client.clone(), state_store.clone());
            auth = auth.with_customer_dependencies(client.clone(), state_store.clone());
            customer = customer.with_delivery_port(client.clone());
            profiles = profiles.with_erp_lookup(client.clone());
            gscale = gscale.with_erp(client.clone());
            rezka = rezka.with_erp(client.clone());
            werka = werka
                .with_customer_issue_writer(client.clone())
                .with_unannounced_writer(client.clone())
                .with_supplier_unannounced_writer(client.clone())
                .with_supplier_purchase_receipt_lookup(client.clone())
                .with_supplier_item_lookup(client.clone())
                .with_confirm_writer(client.clone())
                .with_notification_detail_writer(client.clone())
                .with_supplier_admin_state_lookup(state_store);
            production_orders = client.clone();
            production_order_source = Some(client.clone());
            erp_client = Some(client);
        }
        match config.direct_db_config() {
            Ok(Some(db_config)) => {
                tracing::info!(
                    host = %db_config.host,
                    port = db_config.port,
                    database = %db_config.name,
                    "direct DB read enabled for Werka home"
                );
                let direct_reader = Arc::new(DirectDbReader::new(db_config.clone()));
                if let Some(client) = &erp_client {
                    client.set_credential_provider(direct_reader.clone());
                }
                production_order_source = Some(direct_reader.clone());
                let catalog_reader = if config.catalog_cache_enabled {
                    match CatalogCacheStore::open(&config.catalog_cache_path) {
                        Ok(store) => {
                            let store = Arc::new(store);
                            let sync_store = store.clone();
                            let sync_reader = (*direct_reader).clone();
                            let sync_interval = catalog_cache_sync_interval();
                            tokio::spawn(async move {
                                run_catalog_cache_sync_loop(sync_reader, sync_store, sync_interval)
                                    .await;
                            });
                            Some(Arc::new(
                                CatalogCacheReader::new(store, db_config.default_warehouse.clone())
                                    .with_fallback(direct_reader.clone()),
                            ))
                        }
                        Err(error) => {
                            if config.catalog_cache_fallback_direct_db {
                                tracing::warn!(
                                    %error,
                                    path = %config.catalog_cache_path.display(),
                                    "catalog cache unavailable; using direct DB reads"
                                );
                                None
                            } else {
                                panic!("catalog cache unavailable: {error}");
                            }
                        }
                    }
                } else {
                    None
                };

                if let Some(catalog_reader) = catalog_reader {
                    admin = admin
                        .with_read_port(catalog_reader.clone())
                        .with_credential_port(direct_reader.clone());
                    werka = werka
                        .with_lookup(catalog_reader.clone())
                        .with_customer_issue_source_lookup(direct_reader.clone())
                        .with_notification_detail_lookup(direct_reader.clone())
                        .with_supplier_read_lookup(direct_reader.clone());
                    profiles = profiles.with_read_lookup(catalog_reader.clone());
                    if let Some(state_store) = admin_state_store.clone() {
                        let lookup = Arc::new(AdminReadAuthLookup::new(catalog_reader));
                        auth = auth.with_supplier_dependencies(lookup.clone(), state_store.clone());
                        auth = auth.with_customer_dependencies(lookup, state_store);
                        tracing::info!("auth login lookup uses catalog/direct DB reads");
                    }
                } else {
                    admin = admin
                        .with_read_port(direct_reader.clone())
                        .with_credential_port(direct_reader.clone());
                    werka = werka
                        .with_lookup(direct_reader.clone())
                        .with_customer_issue_source_lookup(direct_reader.clone())
                        .with_notification_detail_lookup(direct_reader.clone())
                        .with_supplier_read_lookup(direct_reader.clone());
                    profiles = profiles.with_read_lookup(direct_reader.clone());
                    if let Some(state_store) = admin_state_store.clone() {
                        let lookup = Arc::new(AdminReadAuthLookup::new(direct_reader.clone()));
                        auth = auth.with_supplier_dependencies(lookup.clone(), state_store.clone());
                        auth = auth.with_customer_dependencies(lookup, state_store);
                        tracing::info!("auth login lookup uses direct DB reads");
                    }
                }
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(%error, "direct DB read disabled");
            }
        }
        if erp_work_order_sync_enabled()
            && let Some(production_order_source) = production_order_source
        {
            tokio::spawn(run_erp_work_order_sync_loop(
                production_maps.clone(),
                production_order_source,
                erp_work_order_sync_interval(),
            ));
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
            sessions,
            mini_engine,
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

async fn run_erp_work_order_sync_loop(
    production_maps: ProductionMapService,
    production_order_source: Arc<dyn ProductionOrderErpSource>,
    interval: Duration,
) {
    loop {
        match sync_erp_work_orders_once(production_maps.clone(), production_order_source.clone())
            .await
        {
            Ok(synced) => {
                tracing::info!(synced, "erpnext work order cache sync completed");
            }
            Err(error) => {
                tracing::warn!(%error, "erpnext work order cache sync failed");
            }
        }
        if interval.is_zero() {
            break;
        }
        sleep(interval).await;
    }
}

async fn sync_erp_work_orders_once(
    production_maps: ProductionMapService,
    production_order_source: Arc<dyn ProductionOrderErpSource>,
) -> Result<usize, String> {
    let maps = production_order_source
        .maps()
        .await
        .map_err(|error| error.to_string())?;
    let count = maps.len();
    if count == 0 {
        return Ok(0);
    }
    production_maps
        .upsert_maps_batch(maps)
        .await
        .map_err(|error| error.to_string())?;
    Ok(count)
}

fn erp_work_order_sync_enabled() -> bool {
    std::env::var("ERP_WORK_ORDER_SYNC_ENABLED")
        .map(|raw| raw.trim() != "0")
        .unwrap_or(true)
}

fn erp_work_order_sync_interval() -> Duration {
    let seconds = std::env::var("ERP_WORK_ORDER_SYNC_INTERVAL_SECONDS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(60);
    Duration::from_secs(seconds)
}

async fn run_catalog_cache_sync_loop(
    direct_reader: DirectDbReader,
    store: Arc<CatalogCacheStore>,
    interval: Duration,
) {
    let mut full_sync_needed = true;
    loop {
        let was_full_sync = full_sync_needed;
        let sync_result = if full_sync_needed {
            sync_catalog_once(&direct_reader, &store).await
        } else {
            sync_catalog_delta_once(&direct_reader, &store).await
        };

        match sync_result {
            Ok(report) => {
                full_sync_needed = false;
                tracing::info!(
                    full = was_full_sync,
                    items = report.items,
                    item_groups = report.item_groups,
                    suppliers = report.suppliers,
                    customers = report.customers,
                    item_suppliers = report.item_suppliers,
                    item_customers = report.item_customers,
                    "catalog cache sync completed"
                );
            }
            Err(error) => {
                tracing::warn!(%error, "catalog cache sync failed");
                full_sync_needed = true;
            }
        }
        if interval.is_zero() {
            break;
        }
        sleep(interval).await;
    }
}

fn catalog_cache_sync_interval() -> Duration {
    let ms = std::env::var("ERP_CATALOG_CACHE_SYNC_INTERVAL_MS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(1_000);
    Duration::from_millis(ms)
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
