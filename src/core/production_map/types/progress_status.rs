use std::collections::BTreeMap;

use crate::core::production_map::queue_state;

use super::completion::ProductionOrderLogEntry;
use super::progress::{
    OrderProgressBatch, OrderProgressBatchStatus, OrderProgressBatchStatusDetail,
    OrderProgressBatchWipStatus, OrderRunSession, OrderRunStatus, ProductionOrderStatusDetail,
};

impl OrderProgressBatch {
    pub fn refresh_status_detail(&mut self) {
        self.status_detail = OrderProgressBatchStatusDetail::from_batch(self);
    }
}

impl OrderProgressBatchStatusDetail {
    pub fn from_batch(batch: &OrderProgressBatch) -> Self {
        let work_status = match batch.status {
            OrderProgressBatchStatus::Paused => "paused",
            OrderProgressBatchStatus::Resumed => "in_progress",
            OrderProgressBatchStatus::Completed => "completed",
        }
        .to_string();
        let wip_status = batch.wip_status.as_str().to_string();
        let processed_by = batch.processed_by_apparatus.trim();
        let is_final_output = batch.action == queue_state::ApparatusQueueAction::Complete
            && batch.status == OrderProgressBatchStatus::Completed
            && batch.next_apparatus.trim().is_empty();
        let flow_status = match batch.wip_status {
            OrderProgressBatchWipStatus::Waiting if is_final_output => "free_wip",
            OrderProgressBatchWipStatus::Waiting => "waiting_next_stage",
            OrderProgressBatchWipStatus::InUse => "in_progress",
            OrderProgressBatchWipStatus::Processed
                if processed_by.to_ascii_lowercase().starts_with("warehouse:") =>
            {
                "accepted_to_stock"
            }
            OrderProgressBatchWipStatus::Processed => "consumed_by_next_stage",
        }
        .to_string();
        let stock_status = match flow_status.as_str() {
            "accepted_to_stock" => "accepted",
            _ => "",
        }
        .to_string();
        Self {
            work_status,
            wip_status,
            flow_status,
            stock_status,
        }
    }
}

impl ProductionOrderStatusDetail {
    pub fn from_order_flow(
        progress_batches: &[OrderProgressBatch],
        run_sessions: &[OrderRunSession],
        queue_states: &BTreeMap<String, BTreeMap<String, String>>,
        logs: &[ProductionOrderLogEntry],
    ) -> Self {
        let mut detail = Self::default();
        detail.add_run_session_counts(run_sessions);
        detail.add_wip_counts(progress_batches);

        let has_pending_queue = queue_states
            .values()
            .flat_map(|states| states.values())
            .any(|state| state == "pending");
        let has_in_progress_queue = queue_states
            .values()
            .flat_map(|states| states.values())
            .any(|state| state == "in_progress");
        detail.completed_queue_count = queue_states
            .values()
            .flat_map(|states| states.values())
            .filter(|state| state.as_str() == "completed")
            .count();
        detail.completed_with_issue_count = logs
            .iter()
            .filter(|entry| entry.completed_with_issue)
            .count();

        let order_status = detail.derive_order_status(has_pending_queue, has_in_progress_queue);
        detail.order_status = order_status.to_string();
        detail.work_status = work_status_for_order(order_status).to_string();
        detail.flow_status = detail.derive_flow_status(order_status).to_string();
        detail.stock_status = detail.derive_stock_status().to_string();
        detail
    }

    fn add_run_session_counts(&mut self, run_sessions: &[OrderRunSession]) {
        for session in run_sessions {
            match session.status {
                OrderRunStatus::Active => self.active_session_count += 1,
                OrderRunStatus::Paused => self.paused_session_count += 1,
                OrderRunStatus::Completed => {}
            }
        }
    }

    fn add_wip_counts(&mut self, progress_batches: &[OrderProgressBatch]) {
        for batch in progress_batches {
            let mut batch = batch.clone();
            batch.refresh_status_detail();
            self.total_wip_count += 1;
            match batch.wip_status {
                OrderProgressBatchWipStatus::Waiting => self.waiting_wip_count += 1,
                OrderProgressBatchWipStatus::InUse => self.in_use_wip_count += 1,
                OrderProgressBatchWipStatus::Processed => self.processed_wip_count += 1,
            }
            match batch.status_detail.flow_status.as_str() {
                "waiting_next_stage" => self.waiting_next_stage_count += 1,
                "in_progress" => {}
                "consumed_by_next_stage" => self.consumed_by_next_stage_count += 1,
                "free_wip" => self.free_wip_count += 1,
                "accepted_to_stock" => self.accepted_wip_count += 1,
                _ => {}
            }
        }
    }

    fn derive_order_status(
        &self,
        has_pending_queue: bool,
        has_in_progress_queue: bool,
    ) -> &'static str {
        if self.active_session_count > 0 || self.in_use_wip_count > 0 || has_in_progress_queue {
            "in_progress"
        } else if !has_pending_queue && self.all_remaining_wips_are_final_outputs() {
            if self.completed_with_issue_count > 0 {
                "completed_with_issue"
            } else {
                "completed"
            }
        } else if self.processed_wip_count > 0
            || self.free_wip_count > 0
            || self.consumed_by_next_stage_count > 0
        {
            "partially_completed"
        } else if self.completed_with_issue_count > 0 {
            "completed_with_issue"
        } else if self.completed_queue_count > 0 {
            "completed"
        } else if self.paused_session_count > 0 {
            "paused"
        } else if self.waiting_next_stage_count > 0 {
            "waiting_next_stage"
        } else if self.waiting_wip_count > 0 || has_pending_queue {
            "ready"
        } else {
            "not_started"
        }
    }

    fn all_remaining_wips_are_final_outputs(&self) -> bool {
        self.free_wip_count + self.accepted_wip_count > 0
            && self.waiting_wip_count == self.free_wip_count
            && self.in_use_wip_count == 0
            && self.waiting_next_stage_count == 0
    }

    fn derive_flow_status(&self, order_status: &str) -> &'static str {
        if self.free_wip_count > 0 && self.waiting_next_stage_count == 0 {
            "free_wip"
        } else if self.accepted_wip_count > 0
            && self.free_wip_count == 0
            && self.waiting_next_stage_count == 0
            && self.in_use_wip_count == 0
        {
            "accepted_to_stock"
        } else {
            flow_status_for_order(order_status)
        }
    }

    fn derive_stock_status(&self) -> &'static str {
        if self.accepted_wip_count > 0
            && self.free_wip_count == 0
            && self.waiting_next_stage_count == 0
            && self.in_use_wip_count == 0
        {
            "accepted"
        } else {
            ""
        }
    }
}

fn work_status_for_order(order_status: &str) -> &'static str {
    match order_status {
        "in_progress" => "in_progress",
        "paused" => "paused",
        "completed" | "completed_with_issue" => "completed",
        "partially_completed" => "partially_completed",
        "waiting_next_stage" | "ready" => "waiting",
        _ => "not_started",
    }
}

fn flow_status_for_order(order_status: &str) -> &'static str {
    match order_status {
        "completed_with_issue" => "completed_with_issue",
        "completed" => "completed",
        "partially_completed" => "partially_completed",
        "in_progress" => "in_progress",
        "paused" => "paused",
        "waiting_next_stage" => "waiting_next_stage",
        "ready" => "ready",
        _ => "not_started",
    }
}
