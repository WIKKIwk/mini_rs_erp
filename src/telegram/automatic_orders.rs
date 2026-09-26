use crate::core::{
    apparatus_standard::CanonicalApparatusService,
    calculate_materials::CalculateMaterialStorePort,
    pending_orders::{PendingOrder, PendingOrderCompletion, PendingOrderStore},
    production_map::{ProductionMapService, automatic},
};
use crate::google_sheets::OrderSheetSink;
use std::sync::Arc;

pub(crate) struct IntakeResult {
    pub completed: bool,
    pub reason: String,
}
impl IntakeResult {
    pub fn completed() -> Self {
        Self {
            completed: true,
            reason: String::new(),
        }
    }
    pub fn pending(reason: String) -> Self {
        Self {
            completed: false,
            reason,
        }
    }
    pub fn message(&self, number: &str) -> String {
        if self.completed {
            format!("✅ Order №T{number} va production map avtomatik yaratildi.")
        } else {
            format!(
                "Order №T{number} chala buyurtma sifatida saqlandi. Avtomatik map yaratilmadi: {}. Mobile’da tugallang.",
                self.reason
            )
        }
    }
}

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(super) struct AutomaticOrders {
    pub apparatus: CanonicalApparatusService,
    pub production_maps: ProductionMapService,
    pub materials: Arc<dyn CalculateMaterialStorePort>,
    pub sheets: Arc<dyn OrderSheetSink>,
}
impl AutomaticOrders {
    pub async fn complete(
        &self,
        pending: &PendingOrder,
        store: &dyn PendingOrderStore,
    ) -> Result<(), String> {
        let apparatus = self
            .apparatus
            .list_runtime_configurations()
            .await
            .map_err(|e| e.to_string())?;
        let materials = self.materials.list().await.map_err(|e| e.to_string())?;
        let mut template = pending.template.clone();
        let map = automatic::generate(&template, &apparatus, &materials)?;
        template.width_mm = map.width_mm.unwrap_or_default();
        template.image_url.clear();
        let saved = self
            .production_maps
            .prepare_map_for_save(map)
            .await
            .map_err(|e| e.to_string())?;
        let mut source = saved.map.clone();
        source.id = format!("template-{:032x}", rand::random::<u128>());
        source.code.clear();
        source.order_number.clear();
        source.order_kg = None;
        source.base_length = None;
        template.source_map_id = source.id.clone();
        let template = crate::db::postgres_calculate_order::stamp_template(template, None);
        let source = self
            .production_maps
            .prepare_map_for_save(source)
            .await
            .map_err(|e| e.to_string())?;
        // Existing pending completion owns the atomic map/template/image/mini-order commit,
        // including row locking and retry identity. Delivery occurs separately afterwards.
        let owner = format!("telegram:{}", pending.telegram_user_id);
        let (done, created) = store
            .complete(
                &pending.id,
                &owner,
                PendingOrderCompletion { saved, template },
                source,
            )
            .await
            .map_err(|e| e.to_string())?;
        if created {
            self.production_maps.notify_live();
            let sheets = self.sheets.clone();
            tokio::spawn(async move {
                if let Err(error) = sheets.append_order(&done.saved.map, &done.template).await {
                    tracing::warn!(?error, "automatic Telegram order sheet append failed");
                }
            });
        }
        Ok(())
    }
}
