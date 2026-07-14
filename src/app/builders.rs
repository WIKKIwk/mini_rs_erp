use std::sync::Arc;

use crate::core::mini_orders::{MiniOrderSink, NoopMiniOrderSink};
use crate::db::postgres_apparatus_group::PostgresApparatusGroupStore;
use crate::db::postgres_calculate_order::PostgresCalculateOrderStore;
use crate::db::postgres_chat::PostgresChatStore;
use crate::db::postgres_customer::PostgresCustomerStore;
use crate::db::postgres_engine::PostgresEngineStore;
use crate::db::postgres_gscale_receipt::PostgresGscaleReceiptStore;
use crate::db::postgres_mini_order::PostgresMiniOrderSink;
use crate::db::postgres_production_map::PostgresProductionMapStore;
use crate::db::postgres_qolip::PostgresQolipStore;
use crate::db::postgres_raw_material_events::PostgresRawMaterialEventStore;
use crate::db::postgres_returned_paint::PostgresReturnedPaintStore;
use crate::db::postgres_warehouse::PostgresWarehouseStore;
use crate::db::postgres_worker::PostgresWorkerStore;
use crate::db::postgres_worker_group::PostgresWorkerGroupStore;
use crate::db::postgres_system_user::PostgresSystemUserStore;
use crate::rps::RpsDriverClient;
use crate::store::apparatus_group_store::ApparatusGroupStore;
use crate::store::calculate_order_store::CalculateOrderStore;

use super::app_local_store::{apparatus_group_store_path, calculate_order_store_path};
use super::postgres_pool::postgres_pool;
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
    match postgres_pool("GScale receipt") {
        Some(pool) => {
            tracing::info!("mini ERP postgres GScale receipt store configured");
            service.with_receipt_store(Arc::new(PostgresGscaleReceiptStore::new(pool)))
        }
        None => service,
    }
}

pub(super) fn build_warehouse_service() -> WarehouseService {
    match postgres_pool("warehouse") {
        Some(pool) => {
            tracing::info!("mini ERP postgres warehouse store configured");
            WarehouseService::new(Arc::new(PostgresWarehouseStore::new(pool)))
        }
        None => WarehouseService::new(Arc::new(
            crate::core::warehouses::MemoryWarehouseStore::new(),
        )),
    }
}

pub(super) fn build_raw_material_event_store() -> Option<PostgresRawMaterialEventStore> {
    match postgres_pool("raw material events") {
        Some(pool) => {
            tracing::info!("mini ERP postgres raw material event store configured");
            Some(PostgresRawMaterialEventStore::new(pool))
        }
        None => None,
    }
}

pub(super) fn build_qolip_service() -> QolipService {
    match postgres_pool("qolip") {
        Some(pool) => {
            tracing::info!("mini ERP postgres qolip store configured");
            QolipService::new(Arc::new(PostgresQolipStore::new(pool)))
        }
        None => QolipService::new(Arc::new(crate::core::qolip::MemoryQolipStore::new())),
    }
}

pub(super) fn build_worker_group_service() -> WorkerGroupService {
    match postgres_pool("worker group") {
        Some(pool) => {
            tracing::info!("mini ERP postgres worker group store configured");
            WorkerGroupService::new(Arc::new(PostgresWorkerGroupStore::new(pool)))
        }
        None => WorkerGroupService::unavailable(),
    }
}

pub(super) fn build_worker_service() -> WorkerService {
    match postgres_pool("worker") {
        Some(pool) => {
            tracing::info!("mini ERP postgres worker store configured");
            WorkerService::new(Arc::new(PostgresWorkerStore::new(pool)))
        }
        None => WorkerService::unavailable(),
    }
}

pub(super) fn build_system_user_service() -> SystemUserService {
    match postgres_pool("system user") {
        Some(pool) => {
            tracing::info!("mini ERP postgres system user store configured");
            SystemUserService::new(Arc::new(PostgresSystemUserStore::new(pool)))
        }
        None => SystemUserService::unavailable(),
    }
}

pub(super) fn build_returned_paint_service() -> ReturnedPaintService {
    match postgres_pool("returned paint") {
        Some(pool) => {
            tracing::info!("mini ERP postgres returned paint store configured");
            ReturnedPaintService::new(Arc::new(PostgresReturnedPaintStore::new(pool)))
        }
        None => ReturnedPaintService::unavailable(),
    }
}

pub(super) fn build_customer_store() -> Option<Arc<PostgresCustomerStore>> {
    postgres_pool("customer directory").map(|pool| Arc::new(PostgresCustomerStore::new(pool)))
}

pub(super) fn build_mini_engine_store() -> Option<PostgresEngineStore> {
    match postgres_pool("engine") {
        Some(pool) => {
            tracing::info!("mini ERP postgres engine store configured");
            Some(PostgresEngineStore::new(pool))
        }
        None => None,
    }
}

pub(super) fn build_mini_order_sink() -> Arc<dyn MiniOrderSink> {
    match postgres_pool("order sink") {
        Some(pool) => {
            tracing::info!("mini ERP postgres order sink configured");
            Arc::new(PostgresMiniOrderSink::new(pool))
        }
        None => Arc::new(NoopMiniOrderSink),
    }
}

pub(super) fn build_production_map_service() -> ProductionMapService {
    match postgres_pool("production map") {
        Some(pool) => {
            tracing::info!("mini ERP postgres production map store configured");
            ProductionMapService::new(Arc::new(PostgresProductionMapStore::new(pool)))
        }
        None => ProductionMapService::new(Arc::new(UnavailableProductionMapStore)),
    }
}

pub(super) fn build_apparatus_groups_service() -> ApparatusGroupService {
    match postgres_pool("apparatus group") {
        Some(pool) => {
            tracing::info!("mini ERP postgres apparatus group store configured");
            ApparatusGroupService::new(Arc::new(PostgresApparatusGroupStore::new(pool)))
        }
        None => build_sqlite_apparatus_groups_service(),
    }
}

fn build_sqlite_apparatus_groups_service() -> ApparatusGroupService {
    ApparatusGroupService::new(Arc::new(ApparatusGroupStore::new(
        apparatus_group_store_path(),
    )))
}

pub(super) fn build_calculate_order_store() -> Arc<dyn CalculateOrderStorePort> {
    match postgres_pool("calculate order") {
        Some(pool) => {
            tracing::info!("mini ERP postgres calculate order store configured");
            Arc::new(PostgresCalculateOrderStore::new(pool))
        }
        None => build_sqlite_calculate_order_store(),
    }
}

pub(super) fn build_chat_service() -> ChatService {
    match postgres_pool("chat") {
        Some(pool) => {
            tracing::info!("mini ERP postgres chat store configured");
            ChatService::new(Arc::new(PostgresChatStore::new(pool)))
        }
        None => ChatService::unavailable(),
    }
}

fn build_sqlite_calculate_order_store() -> Arc<dyn CalculateOrderStorePort> {
    Arc::new(CalculateOrderStore::new(calculate_order_store_path()))
}
