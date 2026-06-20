use std::sync::Arc;

use crate::core::mini_orders::{MiniOrderSink, NoopMiniOrderSink};
use crate::db::postgres::PostgresConfig;
use crate::db::postgres_apparatus_group::PostgresApparatusGroupStore;
use crate::db::postgres_calculate_order::PostgresCalculateOrderStore;
use crate::db::postgres_engine::PostgresEngineStore;
use crate::db::postgres_gscale_receipt::PostgresGscaleReceiptStore;
use crate::db::postgres_mini_order::PostgresMiniOrderSink;
use crate::db::postgres_production_map::PostgresProductionMapStore;
use crate::db::postgres_qolip::PostgresQolipStore;
use crate::db::postgres_warehouse::PostgresWarehouseStore;
use crate::db::postgres_worker::PostgresWorkerStore;
use crate::db::postgres_worker_group::PostgresWorkerGroupStore;
use crate::rps::RpsDriverClient;
use crate::store::apparatus_group_store::ApparatusGroupStore;
use crate::store::calculate_order_store::CalculateOrderStore;

use super::app_local_store::{apparatus_group_store_path, calculate_order_store_path};
use super::unavailable_production_map_store::UnavailableProductionMapStore;
use super::*;

pub(super) fn default_scale_driver_url() -> String {
    std::env::var("RP_SCALE_DRIVER_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "http://gscale.local:39117".to_string())
}

pub(super) fn build_gscale_service(
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

pub(super) fn build_warehouse_service() -> WarehouseService {
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

pub(super) fn build_qolip_service() -> QolipService {
    let config = match PostgresConfig::from_env() {
        Ok(config) => config,
        Err(_) => return QolipService::new(Arc::new(crate::core::qolip::MemoryQolipStore::new())),
    };
    match config.pool_options().connect_lazy(&config.database_url) {
        Ok(pool) => {
            tracing::info!("mini ERP postgres qolip store configured");
            QolipService::new(Arc::new(PostgresQolipStore::new(pool)))
        }
        Err(error) => {
            tracing::warn!(%error, "mini ERP postgres qolip store disabled");
            QolipService::new(Arc::new(crate::core::qolip::MemoryQolipStore::new()))
        }
    }
}

pub(super) fn build_worker_group_service() -> WorkerGroupService {
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

pub(super) fn build_worker_service() -> WorkerService {
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

pub(super) fn build_mini_engine_store() -> Option<PostgresEngineStore> {
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

pub(super) fn build_mini_order_sink() -> Arc<dyn MiniOrderSink> {
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

pub(super) fn build_production_map_service() -> ProductionMapService {
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

pub(super) fn build_apparatus_groups_service() -> ApparatusGroupService {
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

pub(super) fn build_calculate_order_store() -> Arc<dyn CalculateOrderStorePort> {
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
