use std::collections::{BTreeMap, BTreeSet};

use super::*;

impl ProductionMapService {
    pub async fn audit_production_workflow(
        &self,
    ) -> Result<ProductionWorkflowAuditReport, ProductionMapError> {
        let maps = self.store.maps().await?;
        let known_orders = maps
            .iter()
            .map(|map| map.id.trim().to_string())
            .filter(|id| !id.is_empty())
            .collect::<BTreeSet<_>>();
        let queue_states = self.store.apparatus_queue_states().await?;
        let mut violations = Vec::new();
        let mut checked_session_count = 0;
        let mut qr_owners = BTreeMap::<String, (String, Vec<(String, String)>)>::new();
        let mut active_sessions = BTreeMap::<(String, String), Vec<String>>::new();
        let mut batches_by_id = BTreeMap::<String, OrderProgressBatch>::new();

        for (apparatus, states) in &queue_states {
            for order_id in states.keys() {
                let order_id = order_id.trim();
                if !order_id.is_empty() && !known_orders.contains(order_id) {
                    violations.push(ProductionWorkflowAuditViolation::new(
                        "unknown_order_queue_state",
                        order_id,
                        apparatus,
                        "queue state references an order that is not present in production maps",
                    ));
                }
            }
        }

        let mut order_ids = known_orders.clone();
        for states in queue_states.values() {
            order_ids.extend(
                states
                    .keys()
                    .map(|id| id.trim().to_string())
                    .filter(|id| !id.is_empty()),
            );
        }

        for batch in self.store.progress_batches_for_audit().await? {
            batches_by_id.insert(batch.batch_id.trim().to_string(), batch);
        }

        for batch in batches_by_id.values() {
            audit_progress_batch(&known_orders, batch, &mut violations);
            let qr = batch.qr_payload.trim();
            if !qr.is_empty() {
                qr_owners
                    .entry(qr.to_ascii_lowercase())
                    .or_insert_with(|| (qr.to_string(), Vec::new()))
                    .1
                    .push((
                        batch.order_id.trim().to_string(),
                        batch.batch_id.trim().to_string(),
                    ));
            }
        }

        for session in self.store.order_run_sessions_for_audit().await? {
            checked_session_count += 1;
            if !known_orders.contains(session.order_id.trim()) {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "unknown_order_run_session",
                    &session.order_id,
                    &session.session_id,
                    "run session references an order that is not present in production maps",
                ));
            }
            if matches!(
                session.status,
                OrderRunStatus::Active | OrderRunStatus::Paused
            ) {
                active_sessions
                    .entry((
                        session.apparatus.trim().to_ascii_lowercase(),
                        session.order_id.trim().to_string(),
                    ))
                    .or_default()
                    .push(session.session_id.trim().to_string());
            }
        }

        for (qr_payload, owners) in qr_owners.values() {
            if owners.len() <= 1 {
                continue;
            }
            let batches = owners
                .iter()
                .map(|(_, batch_id)| batch_id.as_str())
                .collect::<Vec<_>>()
                .join(",");
            let order_id = owners
                .iter()
                .map(|(order_id, _)| order_id.as_str())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join(",");
            violations.push(ProductionWorkflowAuditViolation::new(
                "duplicate_qr_payload",
                &order_id,
                qr_payload,
                &format!("duplicate progress QR is used by batches: {batches}"),
            ));
        }

        for ((apparatus, order_id), sessions) in active_sessions {
            if sessions.len() <= 1 {
                continue;
            }
            violations.push(ProductionWorkflowAuditViolation::new(
                "duplicate_active_order_session",
                &order_id,
                &apparatus,
                &format!(
                    "more than one active or paused session exists: {}",
                    sessions.join(",")
                ),
            ));
        }

        Ok(ProductionWorkflowAuditReport {
            ok: violations.is_empty(),
            checked_order_count: known_orders.len(),
            checked_batch_count: batches_by_id.len(),
            checked_session_count,
            violations,
        })
    }
}

fn audit_progress_batch(
    known_orders: &BTreeSet<String>,
    batch: &OrderProgressBatch,
    violations: &mut Vec<ProductionWorkflowAuditViolation>,
) {
    let order_id = batch.order_id.trim();
    let batch_id = batch.batch_id.trim();
    if !known_orders.contains(order_id) {
        violations.push(ProductionWorkflowAuditViolation::new(
            "unknown_order_progress_batch",
            order_id,
            batch_id,
            "progress batch references an order that is not present in production maps",
        ));
    }
    if batch.wip_status == OrderProgressBatchWipStatus::InUse
        && (batch.used_by_session_id.trim().is_empty() || batch.used_by_apparatus.trim().is_empty())
    {
        violations.push(ProductionWorkflowAuditViolation::new(
            "in_use_wip_missing_usage",
            order_id,
            batch_id,
            "in-use WIP must record used_by_session_id and used_by_apparatus",
        ));
    }
    if batch.wip_status == OrderProgressBatchWipStatus::Processed
        && (batch.processed_by_session_id.trim().is_empty()
            || batch.processed_by_apparatus.trim().is_empty())
    {
        violations.push(ProductionWorkflowAuditViolation::new(
            "processed_wip_missing_processor",
            order_id,
            batch_id,
            "processed WIP must record processed_by_session_id and processed_by_apparatus",
        ));
    }
    if batch.wip_status == OrderProgressBatchWipStatus::Processed
        && batch
            .processed_by_apparatus
            .trim()
            .to_ascii_lowercase()
            .starts_with("warehouse:")
        && batch
            .payload_json
            .get("finished_goods_stock_id")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .trim()
            .is_empty()
    {
        violations.push(ProductionWorkflowAuditViolation::new(
            "accepted_wip_missing_stock_id",
            order_id,
            batch_id,
            "warehouse-accepted WIP must reference finished_goods_stock_id",
        ));
    }
}
