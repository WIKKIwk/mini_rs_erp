use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;

use super::app_local_store::{LocalStoreBackend, derive_lmdb_path, local_store_backend_from};
use super::catalog_cache_sync_interval;
use crate::core::production_map::{
    MemoryProductionMapStore, ProductionMapDefinition, ProductionMapEdge, ProductionMapNode,
    ProductionMapNodeKind, ProductionMapService,
};
use crate::erpnext::production_order::{ProductionOrderErpError, ProductionOrderErpSource};

#[test]
fn local_store_backend_defaults_to_lmdb_for_production() {
    assert_eq!(local_store_backend_from(None), LocalStoreBackend::Lmdb);
    assert_eq!(local_store_backend_from(Some("")), LocalStoreBackend::Lmdb);
    assert_eq!(
        local_store_backend_from(Some("unknown")),
        LocalStoreBackend::Lmdb
    );
}

#[test]
fn local_store_backend_accepts_explicit_json_and_lmdb() {
    assert_eq!(
        local_store_backend_from(Some("json")),
        LocalStoreBackend::Json
    );
    assert_eq!(
        local_store_backend_from(Some(" JSON ")),
        LocalStoreBackend::Json
    );
    assert_eq!(
        local_store_backend_from(Some("lmdb")),
        LocalStoreBackend::Lmdb
    );
    assert_eq!(
        local_store_backend_from(Some(" LMDB ")),
        LocalStoreBackend::Lmdb
    );
}

#[test]
fn lmdb_path_defaults_next_to_legacy_json_path() {
    assert_eq!(
        derive_lmdb_path(Path::new("data/mobile_sessions.json"), "fallback.lmdb"),
        PathBuf::from("data/mobile_sessions.lmdb")
    );
    assert_eq!(
        derive_lmdb_path(Path::new(""), "fallback.lmdb"),
        PathBuf::from("fallback.lmdb")
    );
}

#[test]
fn catalog_cache_sync_interval_defaults_to_one_second() {
    unsafe {
        std::env::remove_var("ERP_CATALOG_CACHE_SYNC_INTERVAL_MS");
    }

    assert_eq!(
        catalog_cache_sync_interval(),
        std::time::Duration::from_secs(1)
    );
}

#[tokio::test]
async fn erp_work_order_sync_once_upserts_maps_into_local_cache() {
    let service = ProductionMapService::new(Arc::new(MemoryProductionMapStore::new()));
    let source: Arc<dyn ProductionOrderErpSource> = Arc::new(FakeProductionOrderSource {
        maps: vec![test_production_map("zakaz-333", "333")],
    });

    let synced = super::sync_erp_work_orders_once(service.clone(), source)
        .await
        .expect("sync");

    assert_eq!(synced, 1);
    let saved = service
        .map("zakaz-333")
        .await
        .expect("map read")
        .expect("saved map");
    assert_eq!(saved.map.id, "zakaz-333");
    assert_eq!(saved.map.order_number, "333");
}

#[derive(Debug)]
struct FakeProductionOrderSource {
    maps: Vec<ProductionMapDefinition>,
}

#[async_trait]
impl ProductionOrderErpSource for FakeProductionOrderSource {
    async fn maps(&self) -> Result<Vec<ProductionMapDefinition>, ProductionOrderErpError> {
        Ok(self.maps.clone())
    }
}

fn test_production_map(id: &str, order_number: &str) -> ProductionMapDefinition {
    ProductionMapDefinition {
        id: id.to_string(),
        product_code: "ITEM-333".to_string(),
        title: "test map".to_string(),
        code: order_number.to_string(),
        order_number: order_number.to_string(),
        roll_count: Some(7.0),
        width_mm: Some(630.0),
        nodes: vec![
            test_node("start", ProductionMapNodeKind::Start, "Start", 0.0, 0.0),
            test_node("task", ProductionMapNodeKind::Task, "test map", 0.0, 120.0),
            test_node(
                "apparatus-1",
                ProductionMapNodeKind::Apparatus,
                "7 ta rangli pechat - A",
                0.0,
                240.0,
            ),
            test_node("end", ProductionMapNodeKind::End, "test map", 0.0, 360.0),
        ],
        edges: vec![
            test_edge("start", "task"),
            test_edge("task", "apparatus-1"),
            test_edge("apparatus-1", "end"),
        ],
    }
}

fn test_node(
    id: &str,
    kind: ProductionMapNodeKind,
    title: &str,
    x: f64,
    y: f64,
) -> ProductionMapNode {
    ProductionMapNode {
        id: id.to_string(),
        kind,
        title: title.to_string(),
        formula: None,
        role_code: String::new(),
        item_code: String::new(),
        qty_formula: String::new(),
        from_location: String::new(),
        to_location: String::new(),
        alternative_group_id: String::new(),
        alternative_group_label: String::new(),
        alternative_assigned_title: String::new(),
        x,
        y,
    }
}

fn test_edge(from: &str, to: &str) -> ProductionMapEdge {
    ProductionMapEdge {
        from: from.to_string(),
        to: to.to_string(),
        branch: String::new(),
    }
}
