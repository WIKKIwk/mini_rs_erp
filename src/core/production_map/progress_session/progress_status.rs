use crate::core::production_map::queue_state;

use super::progress::{
    OrderProgressBatch, OrderProgressBatchStatus, OrderProgressBatchStatusDetail,
    OrderProgressBatchWipStatus, ProductionOrderStatusDetail,
};
use super::ProductionOrderLifecycleRecord;

impl OrderProgressBatch {
    pub fn refresh_status_detail(&mut self) {
        self.status_detail = OrderProgressBatchStatusDetail::from_batch(self);
    }

    pub fn is_finished_goods_output(&self) -> bool {
        self.next_apparatus.trim().is_empty() && self.has_consistent_action_status()
    }

    pub(crate) fn has_consistent_action_status(&self) -> bool {
        match self.action {
            queue_state::ApparatusQueueAction::Pause => matches!(
                self.status,
                OrderProgressBatchStatus::Paused | OrderProgressBatchStatus::Resumed
            ),
            queue_state::ApparatusQueueAction::DetachRoll => matches!(
                self.status,
                OrderProgressBatchStatus::RollDetached | OrderProgressBatchStatus::Resumed
            ),
            queue_state::ApparatusQueueAction::RollComplete
            | queue_state::ApparatusQueueAction::Complete => {
                self.status == OrderProgressBatchStatus::Completed
            }
            _ => false,
        }
    }
}

impl OrderProgressBatchStatusDetail {
    pub fn from_batch(batch: &OrderProgressBatch) -> Self {
        let work_status = match batch.status {
            OrderProgressBatchStatus::Paused => "paused",
            OrderProgressBatchStatus::RollDetached => "roll_detached",
            OrderProgressBatchStatus::Resumed => "in_progress",
            OrderProgressBatchStatus::Completed => "completed",
        }
        .to_string();
        let wip_status = batch.wip_status.as_str().to_string();
        let flow_status = Self::flow_status_for_batch(batch);
        let stock_status = match flow_status {
            "accepted_to_stock" => "accepted",
            _ => "",
        }
        .to_string();
        Self {
            work_status,
            wip_status,
            flow_status: flow_status.to_string(),
            stock_status,
        }
    }

    pub(crate) fn flow_status_for_batch(batch: &OrderProgressBatch) -> &'static str {
        let processed_by = batch.processed_by_apparatus.trim();
        match batch.wip_status {
            OrderProgressBatchWipStatus::Waiting if batch.is_finished_goods_output() => "free_wip",
            OrderProgressBatchWipStatus::Waiting => "waiting_next_stage",
            OrderProgressBatchWipStatus::InUse => "in_progress",
            OrderProgressBatchWipStatus::Processed
                if processed_by.to_ascii_lowercase().starts_with("warehouse:") =>
            {
                "accepted_to_stock"
            }
            OrderProgressBatchWipStatus::Processed => "consumed_by_next_stage",
        }
    }
}

pub fn derive_order_flow_and_stock_status(
    operational_status: &str,
    free_wip_count: usize,
    waiting_next_stage_count: usize,
    in_use_wip_count: usize,
    accepted_wip_count: usize,
) -> (&'static str, &'static str) {
    if free_wip_count > 0 && waiting_next_stage_count == 0 {
        ("free_wip", "")
    } else if accepted_wip_count > 0
        && free_wip_count == 0
        && waiting_next_stage_count == 0
        && in_use_wip_count == 0
    {
        ("accepted_to_stock", "accepted")
    } else {
        (flow_status_for_order(operational_status), "")
    }
}

impl ProductionOrderStatusDetail {
    pub fn force_frozen(&mut self) {
        self.order_status = "frozen".to_string();
        self.work_status = "frozen".to_string();
        self.flow_status = "frozen".to_string();
    }

    pub fn from_persisted_projection(record: &ProductionOrderLifecycleRecord) -> Self {
        let order_status = record.operational_status.as_str();
        Self {
            lifecycle_status: record.status,
            order_status: order_status.to_string(),
            work_status: work_status_for_order(order_status).to_string(),
            flow_status: record.flow_status.clone(),
            stock_status: record.stock_status.clone(),
            completed_with_issue_count: record.completed_with_issue_count,
        }
    }
}

fn work_status_for_order(order_status: &str) -> &'static str {
    match order_status {
        "in_progress" => "in_progress",
        "paused" => "paused",
        "frozen" => "frozen",
        "completed" | "completed_with_issue" => "completed",
        "partially_completed" => "partially_completed",
        "waiting_next_stage" | "ready" => "waiting",
        _ => "not_started",
    }
}

pub(crate) fn flow_status_for_order(order_status: &str) -> &'static str {
    match order_status {
        "completed_with_issue" => "completed_with_issue",
        "completed" => "completed",
        "partially_completed" => "partially_completed",
        "in_progress" => "in_progress",
        "paused" => "paused",
        "frozen" => "frozen",
        "waiting_next_stage" => "waiting_next_stage",
        "ready" => "ready",
        _ => "not_started",
    }
}
