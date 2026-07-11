use std::sync::Arc;
use std::time::Duration;

use tokio::time::sleep;

use super::*;

pub(super) async fn run_order_sheets_sync_loop(
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

pub(super) fn order_sheets_sync_interval() -> Duration {
    let seconds = std::env::var("GOOGLE_SHEETS_ORDER_SYNC_INTERVAL_SECONDS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(60 * 60);
    Duration::from_secs(seconds)
}

pub(super) async fn run_mini_orders_sync_loop(
    production_maps: ProductionMapService,
    calculate_orders: Arc<dyn CalculateOrderStorePort>,
    production_orders: Arc<dyn MiniOrderSink>,
    interval: Duration,
) {
    loop {
        match sync_mini_orders_once(
            production_maps.clone(),
            calculate_orders.clone(),
            production_orders.clone(),
        )
        .await
        {
            Ok(synced) => tracing::info!(synced, "mini order reconciliation completed"),
            Err(error) => tracing::warn!(%error, "mini order reconciliation failed"),
        }
        if interval.is_zero() {
            break;
        }
        sleep(interval).await;
    }
}

async fn sync_mini_orders_once(
    production_maps: ProductionMapService,
    calculate_orders: Arc<dyn CalculateOrderStorePort>,
    production_orders: Arc<dyn MiniOrderSink>,
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
    production_orders
        .sync_orders(&maps, &templates)
        .await
        .map_err(|error| error.to_string())
}

pub(super) fn mini_orders_sync_interval() -> Duration {
    let seconds = std::env::var("MINI_ORDER_SYNC_INTERVAL_SECONDS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(30);
    Duration::from_secs(seconds)
}
