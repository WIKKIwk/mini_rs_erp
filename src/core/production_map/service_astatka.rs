use super::*;

use std::time::{SystemTime, UNIX_EPOCH};

use super::apparatus::is_laminatsiya_title;
use super::progress::unix_seconds;

impl ProductionMapService {
    pub async fn record_laminatsiya_astatka(
        &self,
        apparatus: &str,
        order_id: &str,
        actor: QueueActionActor,
        lamination_print_leftover_rolls: Option<f64>,
        lamination_film_leftover_rolls: Option<f64>,
        total_waste: Option<f64>,
        description: &str,
    ) -> Result<LaminatsiyaAstatkaReport, ProductionMapError> {
        let apparatus = apparatus.trim();
        let order_id = order_id.trim();
        if apparatus.is_empty() || order_id.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        if !is_laminatsiya_title(apparatus) {
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

        let order_id = self
            .store
            .maps()
            .await?
            .into_iter()
            .find(|map| map.id.trim().eq_ignore_ascii_case(order_id))
            .map(|map| map.id.trim().to_string())
            .ok_or(ProductionMapError::MapNotFound)?;

        // Astatka is order-level audit data. Serialize the anchor lookup and
        // insert so two quick reports cannot receive the same interval.
        let _guard = self.queue_action_guard().await;
        let previous_reports = self
            .store
            .laminatsiya_astatka_reports_for_order(&order_id)
            .await?;
        let previous_to = previous_reports.iter().map(|report| report.to_at_unix).max();
        let from_at_unix = if let Some(previous_to) = previous_to {
            previous_to
        } else {
            let session_start = self
                .store
                .order_run_sessions_for_order(&order_id)
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
                .get(&order_id)
                .into_iter()
                .flatten()
                .filter(|log| log.action == queue_state::ApparatusQueueAction::Start)
                .map(|log| log.created_at_unix)
                .filter(|value| *value > 0)
                .min();
            session_start.into_iter().chain(log_start).min()
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
}
