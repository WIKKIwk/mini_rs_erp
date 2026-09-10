use super::*;
use crate::core::apparatus_standard::ApparatusId;
use crate::core::returned_paint::ReturnedPaintRequest;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BosmaAstatkaReport {
    pub report_id: String,
    pub order_id: String,
    pub apparatus: String,
    pub from_at_unix: i64,
    pub to_at_unix: i64,
    pub total_waste: f64,
    // Optional legacy output metrics; new astatka reports only record waste/paint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_goods_meter: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_goods_kg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bobina_kg: Option<f64>,
    pub returned_paint: ReturnedPaintRequest,
    pub description: String,
}

impl ProductionMapService {
    /// No output or WIP mutation; reports for finished executions may close their stage.
    pub async fn record_bosma_astatka(
        &self,
        mut report: BosmaAstatkaReport,
    ) -> Result<BosmaAstatkaReport, ProductionMapError> {
        let apparatus_id = ApparatusId::new(report.apparatus.trim().to_string())
            .map_err(|_| ProductionMapError::ProgressInputInvalid)?;
        let canonical = self.resolve_canonical_apparatus(&apparatus_id).await?;
        if canonical.runtime.apparatus_id != apparatus_id
            || !pechat::is_pechat_apparatus(&canonical)
            || !report.total_waste.is_finite()
            || report.total_waste < 0.0
            || [
                report.finished_goods_meter,
                report.finished_goods_kg,
                report.bobina_kg,
            ]
            .iter()
            .flatten()
            .any(|value| !value.is_finite() || *value <= 0.0)
            || report.returned_paint.order_id != report.order_id
            || report.returned_paint.apparatus != report.apparatus
        {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        self.raw_map(&report.order_id)
            .await?
            .ok_or(ProductionMapError::MapNotFound)?;
        let _guard = self.queue_action_guard().await;
        let anchor = self.astatka_execution_anchor(&report.order_id, &report.apparatus).await?;
        if self
            .store
            .order_control_states()
            .await?
            .get(&report.order_id)
            .is_some_and(|control| control.state == OrderControlState::Frozen)
        {
            return Err(ProductionMapError::OrderFrozen);
        }
        let states = self.store.apparatus_queue_states().await?;
        if anchor.as_ref().is_none_or(|s| s.status != OrderRunStatus::Completed) && !states
            .get(&report.apparatus)
            .and_then(|orders| orders.get(&report.order_id))
            .is_some_and(|state| matches!(state.as_str(), "in_progress" | "paused" | "completed"))
        {
            return Err(ProductionMapError::OrderNotStarted);
        }
        let initial = self
            .store
            .order_run_sessions_for_order(&report.order_id)
            .await?
            .into_iter()
            .filter(|session| session.apparatus == report.apparatus)
            .map(|session| session.started_at_unix)
            .filter(|time| *time > 0)
            .min()
            .ok_or(ProductionMapError::OrderNotStarted)?;
        let previous = self
            .store
            .bosma_astatka_reports_for_order(&report.order_id)
            .await?
            .into_iter()
            .filter(|item| item.apparatus == report.apparatus)
            .map(|item| item.to_at_unix)
            .max();
        report.from_at_unix = previous.unwrap_or(initial);
        report.to_at_unix = super::progress::unix_seconds();
        if report.to_at_unix < report.from_at_unix {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        let entropy = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        report.report_id = format!("bosma-astatka:{entropy}:{}", report.order_id);
        report.returned_paint.id = report.report_id.clone();
        report.description = report.description.trim().to_string();
        let actor = QueueActionActor {
            role: serde_json::to_value(report.returned_paint.sender_role).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default(), ref_: report.returned_paint.sender_ref.clone(),
            display_name: report.returned_paint.sender_display_name.clone(),
        };
        self.store.commit_stage_astatka_report(StageAstatkaReport::Bosma(report.clone()), anchor, actor).await?;
        self.notify_live();
        Ok(report)
    }
}
