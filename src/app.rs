use std::sync::Arc;
use std::time::Instant;

use crate::ai::werka_search::WerkaAiSearchService;
use crate::config::{AppConfig, DotEnvPersister};
use crate::core::admin::monitor_hub::SystemMonitorHub;
use crate::core::admin::service::AdminService;
use crate::core::apparatus_groups::ApparatusGroupService;
use crate::core::auth::ports::CustomerLookup;
use crate::core::auth::service::AuthService;
use crate::core::backup_doctor::BackupDoctor;
use crate::core::calculate_orders::CalculateOrderStorePort;
use crate::core::chat::ChatService;
use crate::core::chat_media::ChatMediaService;
use crate::core::customer::service::CustomerService;
use crate::core::gscale::GscaleService;
use crate::core::mini_orders::MiniOrderSink;
use crate::core::mobile_release::MobileReleaseStore;
use crate::core::production_map::ProductionMapService;
use crate::core::profile::ports::ProfileStorePort;
use crate::core::profile::service::ProfileService;
use crate::core::push::service::PushService;
use crate::core::qolip::QolipService;
use crate::core::returned_paint::ReturnedPaintService;
use crate::core::rezka::RezkaService;
use crate::core::rps_batch::RpsBatchService;
use crate::core::session::manager::SessionManager;
use crate::core::system_users::SystemUserService;
use crate::core::warehouse_events::WarehouseEventHub;
use crate::core::warehouses::WarehouseService;
use crate::core::werka::service::WerkaService;
use crate::core::worker_groups::WorkerGroupService;
use crate::core::workers::WorkerService;
use crate::db::postgres_customer::PostgresCustomerStore;
use crate::db::postgres_engine::PostgresEngineStore;
use crate::db::postgres_raw_material_events::PostgresRawMaterialEventStore;
use crate::fcm::discover_push_sender;
use crate::google_sheets::{OrderSheetSink, discover_order_sheet_sink};
use crate::rps::RpsDriverClient;
use crate::store::admin_store::JsonAdminStore;
use crate::store::chat_media_local::LocalChatMediaStorage;
use crate::store::chat_media_r2::R2ChatMediaStorage;
use crate::store::profile_avatar_local::LocalProfileAvatarStorage;
use crate::store::profile_avatar_r2::R2ProfileAvatarStorage;
use crate::store::role_store::RoleDefinitionStore;

#[path = "app_local_store.rs"]
mod app_local_store;
use app_local_store::*;

mod admin_catalog_overlay;
mod builders;
mod order_sheets;
mod postgres_pool;
mod unavailable_production_map_store;

use self::admin_catalog_overlay::build_admin_catalog_ports;
use self::builders::*;
use self::order_sheets::*;

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
    pub chat: ChatService,
    pub chat_media: ChatMediaService,
    pub order_sheets: Arc<dyn OrderSheetSink>,
    pub production_orders: Arc<dyn MiniOrderSink>,
    pub mobile_releases: MobileReleaseStore,
    pub calculate_order_image_dir: Arc<std::path::PathBuf>,
    pub push: PushService,
    pub gscale: GscaleService,
    pub qolip: QolipService,
    pub rezka: RezkaService,
    pub returned_paint: ReturnedPaintService,
    pub rps_batch: RpsBatchService,
    pub werka: WerkaService,
    pub warehouses: WarehouseService,
    pub workers: WorkerService,
    pub system_users: SystemUserService,
    pub worker_groups: WorkerGroupService,
    pub sessions: SessionManager,
    pub warehouse_events: WarehouseEventHub,
    pub raw_material_events: Option<PostgresRawMaterialEventStore>,
    pub system_monitor_hub: SystemMonitorHub,
    pub backup_doctor: BackupDoctor,
    #[allow(dead_code)]
    pub mini_engine: Option<PostgresEngineStore>,
    pub started_at: Instant,
    pub started_at_unix: i64,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        let admin_store = Arc::new(JsonAdminStore::new(admin_store_path()));
        let customer_store = build_customer_store();
        let workers = build_worker_service();
        let system_users = build_system_user_service();
        let auth = build_auth_service(
            &config,
            admin_store.clone(),
            workers.clone(),
            system_users.clone(),
            customer_store.clone(),
        );
        let profile_store = build_profile_store(&config);
        let admin = build_admin_service(
            &config,
            admin_store.clone(),
            auth.clone(),
            profile_store.clone(),
            customer_store,
        );
        let customer = CustomerService::new();
        let production_maps = build_production_map_service();
        let apparatus_groups = build_apparatus_groups_service();
        let calculate_orders = build_calculate_order_store();
        let order_sheets = discover_order_sheet_sink();
        let production_orders = build_mini_order_sink();
        let mobile_releases = MobileReleaseStore::from_env();
        spawn_mini_orders_sync_loop_if_enabled(
            production_maps.clone(),
            calculate_orders.clone(),
            production_orders.clone(),
        );
        let mini_engine = build_mini_engine_store();
        let raw_material_events = build_raw_material_event_store();
        spawn_order_sheets_sync_loop_if_enabled(
            production_maps.clone(),
            calculate_orders.clone(),
            order_sheets.clone(),
        );
        let calculate_order_image_dir = Arc::new(calculate_order_image_dir());
        let push_token_store = build_push_token_store(&config);
        let profiles = build_profile_service(&config, profile_store);
        let push = PushService::new(push_token_store.clone())
            .with_sender(discover_push_sender(push_token_store));
        let chat = build_chat_service();
        let chat_media = build_chat_media_service(&config);
        chat.start_delivery_worker(push.clone());
        let rps_batch = RpsBatchService::new(build_rps_batch_store());
        let scale_driver = build_scale_driver(&config);
        let warehouse_events = WarehouseEventHub::new();
        let system_monitor_hub = SystemMonitorHub::new();
        let backup_doctor = BackupDoctor::from_env();
        backup_doctor.start_scheduler();
        let gscale = build_gscale_service(scale_driver.clone(), warehouse_events.clone());
        let rezka = build_rezka_service(scale_driver);
        let qolip = build_qolip_service();
        let returned_paint = build_returned_paint_service();
        let werka = build_werka_service(&config);
        let warehouses = build_warehouse_service();
        let worker_groups = build_worker_group_service();
        let sessions = build_session_manager(&config);

        Self {
            config: Arc::new(config),
            admin,
            auth,
            customer,
            profiles,
            production_maps,
            apparatus_groups,
            calculate_orders,
            chat,
            chat_media,
            order_sheets,
            production_orders,
            mobile_releases,
            calculate_order_image_dir,
            push,
            gscale,
            qolip,
            rezka,
            returned_paint,
            rps_batch,
            werka,
            warehouses,
            workers,
            system_users,
            worker_groups,
            sessions,
            warehouse_events,
            raw_material_events,
            system_monitor_hub,
            backup_doctor,
            mini_engine,
            started_at: Instant::now(),
            started_at_unix: time::OffsetDateTime::now_utc().unix_timestamp(),
        }
    }
}

fn build_auth_service(
    config: &AppConfig,
    admin_store: Arc<JsonAdminStore>,
    workers: WorkerService,
    system_users: SystemUserService,
    customer_store: Option<Arc<PostgresCustomerStore>>,
) -> AuthService {
    let customer_lookup: Arc<dyn CustomerLookup> = customer_store
        .map(|store| store as Arc<dyn CustomerLookup>)
        .unwrap_or_else(|| admin_store.clone());
    AuthService::new(config)
        .with_supplier_dependencies(admin_store.clone(), admin_store.clone())
        .with_customer_dependencies(customer_lookup, admin_store.clone())
        .with_material_taminotchi_dependencies(admin_store.clone(), admin_store.clone())
        .with_worker_dependencies(Arc::new(workers), admin_store.clone())
        .with_system_user_dependencies(Arc::new(system_users), admin_store)
}

fn build_admin_service(
    config: &AppConfig,
    admin_store: Arc<JsonAdminStore>,
    auth: AuthService,
    profile_store: Arc<dyn ProfileStorePort>,
    customer_store: Option<Arc<PostgresCustomerStore>>,
) -> AdminService {
    let mut admin =
        AdminService::new(config).with_env_persister(Arc::new(DotEnvPersister::new(".env")));
    let (admin_read_port, admin_write_port) =
        build_admin_catalog_ports(admin_store.clone(), customer_store);
    admin = admin
        .with_read_port(admin_read_port)
        .with_write_port(admin_write_port)
        .with_state_port(admin_store);
    admin = admin.with_role_store(Arc::new(RoleDefinitionStore::new(role_store_path())));
    admin
        .with_auth_config_sink(Arc::new(auth))
        .with_profile_store(profile_store)
}

fn build_scale_driver(config: &AppConfig) -> Arc<RpsDriverClient> {
    Arc::new(RpsDriverClient::new(
        config.http_timeout,
        default_scale_driver_url(),
    ))
}

fn build_rezka_service(scale_driver: Arc<RpsDriverClient>) -> RezkaService {
    RezkaService::new()
        .with_driver(scale_driver)
        .with_epc_source(Arc::new(crate::core::gscale::epc::GscaleEpcGenerator::new()))
}

fn build_werka_service(config: &AppConfig) -> WerkaService {
    let werka = WerkaService::new();
    let ai_key = std::env::var("GEMINI_API_KEY").unwrap_or_default();
    if ai_key.trim().is_empty() {
        return werka;
    }
    werka.with_ai_search(Arc::new(WerkaAiSearchService::new(
        &ai_key,
        &std::env::var("GEMINI_VISION_MODEL").unwrap_or_default(),
        config.http_timeout,
    )))
}

fn build_profile_service(
    config: &AppConfig,
    profile_store: Arc<dyn ProfileStorePort>,
) -> ProfileService {
    let profiles = ProfileService::new(String::new()).with_store(profile_store);
    if let Some(avatar_storage) = R2ProfileAvatarStorage::from_env(config.http_timeout) {
        profiles.with_avatar_storage(Arc::new(avatar_storage))
    } else {
        profiles.with_avatar_storage(Arc::new(LocalProfileAvatarStorage::from_env()))
    }
}

fn build_session_manager(config: &AppConfig) -> SessionManager {
    match local_store_backend("MOBILE_API_SESSION_STORE_BACKEND") {
        LocalStoreBackend::Lmdb => {
            let lmdb_path = session_lmdb_path(config);
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
    }
}

fn spawn_order_sheets_sync_loop_if_enabled(
    production_maps: ProductionMapService,
    calculate_orders: Arc<dyn CalculateOrderStorePort>,
    order_sheets: Arc<dyn OrderSheetSink>,
) {
    if order_sheets.enabled() {
        tokio::spawn(run_order_sheets_sync_loop(
            production_maps,
            calculate_orders,
            order_sheets,
            order_sheets_sync_interval(),
        ));
    }
}

fn spawn_mini_orders_sync_loop_if_enabled(
    production_maps: ProductionMapService,
    calculate_orders: Arc<dyn CalculateOrderStorePort>,
    production_orders: Arc<dyn MiniOrderSink>,
) {
    if production_orders.enabled() {
        tokio::spawn(run_mini_orders_sync_loop(
            production_maps,
            calculate_orders,
            production_orders,
            mini_orders_sync_interval(),
        ));
    }
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
