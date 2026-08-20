use super::*;

use std::time::{SystemTime, UNIX_EPOCH};

use super::progress::unix_seconds;
use crate::core::apparatus_standard::ApparatusId;
use crate::core::quantity::positive_erp_quantity;

impl ProductionMapService {
    #[allow(clippy::too_many_arguments)]
    pub async fn record_laminatsiya_astatka(
        &self,
        apparatus: &str,
        order_id: &str,
        actor: QueueActionActor,
        lamination_print_leftover_rolls: Option<f64>,
        lamination_film_leftover_rolls: Option<f64>,
        total_waste: Option<f64>,
        finished_goods_meter: Option<f64>,
        finished_goods_kg: Option<f64>,
        bobina_kg: Option<f64>,
        description: &str,
    ) -> Result<LaminatsiyaAstatkaReport, ProductionMapError> {
        let apparatus = apparatus.trim();
        let order_id = order_id.trim();
        if apparatus.is_empty() || order_id.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        let apparatus_id = ApparatusId::new(apparatus.to_string())
            .map_err(|_| ProductionMapError::ProgressInputInvalid)?;
        let canonical = self.resolve_canonical_apparatus(&apparatus_id).await?;
        if canonical.identity.id != apparatus_id
            || !super::apparatus::is_laminatsiya_apparatus(&canonical)
        {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        for metric in [
            lamination_print_leftover_rolls,
            lamination_film_leftover_rolls,
            total_waste,
        ] {
            if !metric.is_some_and(|value| value.is_finite() && value >= 0.0) {
                return Err(ProductionMapError::LaminatsiyaAstatkaMetricsRequired);
            }
        }
        for metric in [finished_goods_meter, finished_goods_kg, bobina_kg] {
            if metric.is_some_and(|value| positive_erp_quantity(value).is_none()) {
                return Err(ProductionMapError::LaminatsiyaAstatkaMetricsRequired);
            }
        }

        let order_id = self.astatka_order_id(order_id).await?;

        // Astatka is order-level audit data. Serialize the anchor lookup and
        // insert so two quick reports cannot receive the same interval.
        let _guard = self.queue_action_guard().await;
        let previous_reports = self
            .store
            .laminatsiya_astatka_reports_for_order(&order_id)
            .await?;
        let previous_to = previous_reports
            .iter()
            .map(|report| report.to_at_unix)
            .max();
        let from_at_unix = if let Some(previous_to) = previous_to {
            Some(previous_to)
        } else {
            self.astatka_initial_from_at(&order_id).await?
        }
        .ok_or(ProductionMapError::OrderNotStarted)?;

        let to_at_unix = unix_seconds();
        if to_at_unix < from_at_unix {
            return Err(ProductionMapError::ProgressInputInvalid);
        }

        let created_at_unix = to_at_unix;
        let entropy = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let report = LaminatsiyaAstatkaReport {
            report_id: format!("laminatsiya-astatka:{entropy}:{order_id}"),
            order_id: order_id.to_string(),
            apparatus: apparatus.to_string(),
            from_at_unix,
            to_at_unix,
            lamination_print_leftover_rolls: lamination_print_leftover_rolls
                .expect("validated laminatsiya astatka metric"),
            lamination_film_leftover_rolls: lamination_film_leftover_rolls
                .expect("validated laminatsiya astatka metric"),
            total_waste: total_waste.expect("validated laminatsiya astatka metric"),
            finished_goods_meter,
            finished_goods_kg,
            bobina_kg,
            worker_role: actor.role.trim().to_string(),
            worker_ref: actor.ref_.trim().to_string(),
            worker_display_name: actor.display_name.trim().to_string(),
            description: description.trim().to_string(),
            created_at_unix,
        };
        self.store
            .put_laminatsiya_astatka_report(report.clone())
            .await?;
        self.notify_live();
        Ok(report)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_rezka_astatka(
        &self,
        apparatus: &str,
        order_id: &str,
        actor: QueueActionActor,
        total_waste: Option<f64>,
        rezka_bosma_waste: Option<f64>,
        rezka_lamination_waste: Option<f64>,
        rezka_edge_waste: Option<f64>,
        finished_goods_meter: Option<f64>,
        finished_goods_kg: Option<f64>,
        bobina_kg: Option<f64>,
        description: &str,
    ) -> Result<RezkaAstatkaReport, ProductionMapError> {
        let apparatus = apparatus.trim();
        let order_id = order_id.trim();
        if apparatus.is_empty() || order_id.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        let apparatus_id = ApparatusId::new(apparatus.to_string())
            .map_err(|_| ProductionMapError::ProgressInputInvalid)?;
        let canonical = self.resolve_canonical_apparatus(&apparatus_id).await?;
        if canonical.identity.id != apparatus_id
            || !super::apparatus::is_rezka_apparatus(&canonical)
        {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        for metric in [
            total_waste,
            rezka_bosma_waste,
            rezka_lamination_waste,
            rezka_edge_waste,
        ] {
            if !metric.is_some_and(|value| value.is_finite() && value >= 0.0) {
                return Err(ProductionMapError::RezkaAstatkaMetricsRequired);
            }
        }
        for metric in [finished_goods_meter, finished_goods_kg, bobina_kg] {
            if metric.is_some_and(|value| positive_erp_quantity(value).is_none()) {
                return Err(ProductionMapError::RezkaAstatkaMetricsRequired);
            }
        }

        let order_id = self.astatka_order_id(order_id).await?;
        let _guard = self.queue_action_guard().await;
        let previous_reports = self
            .store
            .rezka_astatka_reports_for_order(&order_id)
            .await?;
        let previous_to = previous_reports
            .iter()
            .map(|report| report.to_at_unix)
            .max();
        let from_at_unix = if let Some(previous_to) = previous_to {
            Some(previous_to)
        } else {
            self.astatka_initial_from_at(&order_id).await?
        }
        .ok_or(ProductionMapError::OrderNotStarted)?;

        let to_at_unix = unix_seconds();
        if to_at_unix < from_at_unix {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        let created_at_unix = to_at_unix;
        let entropy = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let report = RezkaAstatkaReport {
            report_id: format!("rezka-astatka:{entropy}:{order_id}"),
            order_id: order_id.to_string(),
            apparatus: apparatus.to_string(),
            from_at_unix,
            to_at_unix,
            total_waste: total_waste.expect("validated rezka astatka metric"),
            rezka_bosma_waste: rezka_bosma_waste.expect("validated rezka astatka metric"),
            rezka_lamination_waste: rezka_lamination_waste.expect("validated rezka astatka metric"),
            rezka_edge_waste: rezka_edge_waste.expect("validated rezka astatka metric"),
            finished_goods_meter,
            finished_goods_kg,
            bobina_kg,
            worker_role: actor.role.trim().to_string(),
            worker_ref: actor.ref_.trim().to_string(),
            worker_display_name: actor.display_name.trim().to_string(),
            description: description.trim().to_string(),
            created_at_unix,
        };
        self.store.put_rezka_astatka_report(report.clone()).await?;
        self.notify_live();
        Ok(report)
    }

    async fn astatka_order_id(&self, order_id: &str) -> Result<String, ProductionMapError> {
        self.store
            .maps()
            .await?
            .into_iter()
            .find(|map| map.id.trim().eq_ignore_ascii_case(order_id))
            .map(|map| map.id.trim().to_string())
            .ok_or(ProductionMapError::MapNotFound)
    }

    async fn astatka_initial_from_at(
        &self,
        order_id: &str,
    ) -> Result<Option<i64>, ProductionMapError> {
        let session_start = self
            .store
            .order_run_sessions_for_order(order_id)
            .await?
            .into_iter()
            .map(|session| session.started_at_unix)
            .filter(|value| *value > 0)
            .min();
        let order_ids = vec![order_id.to_string()];
        let log_start = self
            .store
            .queue_action_logs_for_orders(&order_ids)
            .await?
            .get(order_id)
            .into_iter()
            .flatten()
            .filter(|log| log.action == queue_state::ApparatusQueueAction::Start)
            .map(|log| log.created_at_unix)
            .filter(|value| *value > 0)
            .min();
        Ok(session_start.into_iter().chain(log_start).min())
    }
}
