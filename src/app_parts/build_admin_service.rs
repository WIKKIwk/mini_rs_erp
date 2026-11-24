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

fn build_werka_service(config: &AppConfig, pool: Option<sqlx::PgPool>) -> WerkaService {
    let werka = match pool {
        Some(pool) => WerkaService::new().with_lookup(Arc::new(PostgresWerkaHomeLookup::new(pool))),
        None => WerkaService::new().with_lookup(Arc::new(EmptyWerkaHomeLookup)),
    };
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
