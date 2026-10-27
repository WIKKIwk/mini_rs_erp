//! Synthetic, database-free research over the actual audit validators.
//!
//! Each timed call builds the order/session lookup indexes and active-session
//! groups. Batch payload loading and its owned index are prepared once, outside
//! timing. SQL, queue/sequence scans, QR grouping, transfers and capacity checks
//! are excluded: this is a loaded-validation phase, not an endpoint benchmark.

use super::*;

const APPARATUS: &str = "apparatus:default:bosma_7";

struct AuditFixture {
    maps: Vec<ProductionMapDefinition>,
    known_orders: BTreeSet<String>,
    sessions: Vec<OrderRunSession>,
    batches: AuditMap<String, OrderProgressBatch>,
    queue_states: BTreeMap<String, BTreeMap<String, String>>,
}

impl AuditFixture {
    fn new(size: usize, mixed: bool) -> Self {
        let mut maps = Vec::with_capacity(size);
        let mut sessions = Vec::with_capacity(size);
        let mut batches = AuditMap::new();
        let mut states = BTreeMap::new();
        for index in permuted_indices(size) {
            let order_id = format!("zakaz-audit-research-{index:08}");
            let session_id = format!("session-audit-research-{index:08}");
            maps.push(fixture_map(&order_id));
            let mut session = fixture_session(&session_id, &order_id);
            if index % 16 == 0 {
                session.status = OrderRunStatus::Paused;
                states.insert(order_id.clone(), "paused".to_string());
            }
            let mut batch = fixture_batch(&format!("batch-{index:08}"), &order_id);
            batch.session_id = session_id;
            if mixed && index % 11 == 0 {
                batch.session_id = "session-missing".to_string();
            }
            if mixed && index % 13 == 0 {
                batch.parent_batch_id = "parent-missing".to_string();
            }
            batches.insert(batch.batch_id.clone(), batch);
            sessions.push(session);
        }
        let known_orders = maps.iter().map(|map| map.id.clone()).collect();
        Self {
            maps,
            known_orders,
            sessions,
            batches,
            queue_states: BTreeMap::from([(APPARATUS.to_string(), states)]),
        }
    }

    fn validate(&self) -> ProductionWorkflowAuditReport {
        let maps_by_id = self
            .maps
            .iter()
            .map(|map| (map.id.clone(), map))
            .collect::<AuditMap<_, _>>();
        let sessions_by_id = self
            .sessions
            .iter()
            .map(|session| (session.session_id.clone(), session))
            .collect::<AuditMap<_, _>>();
        let mut active_sessions = AuditMap::new();
        let mut violations = Vec::new();
        for session in &self.sessions {
            audit_session(
                &self.known_orders,
                &maps_by_id,
                &self.queue_states,
                session,
                &mut active_sessions,
                &mut violations,
            );
        }
        for batch in self.batches.values() {
            audit_progress_batch(
                &self.known_orders,
                &maps_by_id,
                &sessions_by_id,
                &self.batches,
                batch,
                &mut violations,
            );
        }
        audit_paused_session_progress(&self.sessions, &self.batches, &mut violations);
        ProductionWorkflowAuditReport {
            ok: violations.is_empty(),
            checked_order_count: self.known_orders.len(),
            checked_batch_count: self.batches.len(),
            checked_session_count: self.sessions.len(),
            violations,
        }
    }
}

fn permuted_indices(size: usize) -> Vec<usize> {
    let mut indices = (0..size).collect::<Vec<_>>();
    // Multiplication by an odd constant gives unique, dispersed ordering keys.
    // This is a fixed permutation, with no RNG, wall time or external input.
    indices.sort_unstable_by_key(|index| (*index as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15));
    indices
}

fn fixture_map(order_id: &str) -> ProductionMapDefinition {
    serde_json::from_value(serde_json::json!({
        "id": order_id, "product_code": "AUDIT-RESEARCH", "title": "Synthetic audit",
        "nodes": [
            {"id": "start", "kind": "start", "title": "Start"},
            {"id": "print", "kind": "apparatus", "title": "Print", "apparatus_id": APPARATUS},
            {"id": "end", "kind": "end", "title": "End"}
        ],
        "edges": [{"from": "start", "to": "print"}, {"from": "print", "to": "end"}]
    }))
    .expect("synthetic audit map")
}

fn fixture_session(session_id: &str, order_id: &str) -> OrderRunSession {
    OrderRunSession {
        session_id: session_id.to_string(),
        apparatus: APPARATUS.to_string(),
        order_id: order_id.to_string(),
        stage_node_id: "print".to_string(),
        status: OrderRunStatus::Completed,
        worker_role: "aparatchi".to_string(),
        worker_ref: "synthetic-worker".to_string(),
        worker_display_name: "Synthetic Worker".to_string(),
        started_at_unix: 1,
        updated_at_unix: 2,
        payload_json: serde_json::json!({}),
    }
}

fn fixture_batch(batch_id: &str, order_id: &str) -> OrderProgressBatch {
    serde_json::from_value(serde_json::json!({
        "batch_id": batch_id, "session_id": format!("session-{batch_id}"),
        "started_at_unix": 1, "completed_at_unix": 2,
        "apparatus": APPARATUS, "order_id": order_id,
        "action": "pause", "status": "paused", "produced_qty": 1.0, "uom": "m",
        "qr_payload": format!("qr-{batch_id}"), "label_item_code": "AUDIT-RESEARCH",
        "label_item_name": "Synthetic audit", "executor_name": "Synthetic Worker",
        "worker_role": "aparatchi", "worker_ref": "synthetic-worker",
        "worker_display_name": "Synthetic Worker", "wip_status": "waiting",
        "current_apparatus": APPARATUS, "current_location": APPARATUS,
        "payload_json": {}
    }))
    .expect("synthetic audit batch")
}

#[cfg(feature = "bbtree-bench")]
struct AuditWorkload {
    fixture: AuditFixture,
    mixed: bool,
    last: Option<ProductionWorkflowAuditReport>,
}

#[cfg(feature = "bbtree-bench")]
impl super::super::bbtree_research::Workload for AuditWorkload {
    fn name(&self) -> &'static str {
        if self.mixed {
            "audit_loaded_validation_mixed"
        } else {
            "audit_loaded_validation_clean"
        }
    }

    fn size(&self) -> usize {
        self.fixture.maps.len()
    }

    fn run(&mut self) {
        self.last = Some(std::hint::black_box(self.fixture.validate()));
    }

    fn fingerprint(&self) -> String {
        use sha2::{Digest, Sha256};
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&self.last).expect("audit fingerprint"))
        )
    }
}

#[cfg(feature = "bbtree-bench")]
pub(super) fn workloads() -> Vec<Box<dyn super::super::bbtree_research::Workload>> {
    let mut workloads: Vec<Box<dyn super::super::bbtree_research::Workload>> = Vec::new();
    for size in [100, 1_000, 5_000] {
        for mixed in [false, true] {
            workloads.push(Box::new(AuditWorkload {
                fixture: AuditFixture::new(size, mixed),
                mixed,
                last: None,
            }));
        }
        let ordered = AuditFixture::new(size, false)
            .batches
            .into_values()
            .collect::<Vec<_>>();
        let batches = permuted_indices(size)
            .into_iter()
            .map(|index| ordered[index].clone())
            .collect();
        workloads.push(Box::new(AuditIndexBuildWorkload {
            batches,
            last: AuditMap::new(),
        }));
    }
    workloads
}

#[cfg(feature = "bbtree-bench")]
struct AuditIndexBuildWorkload {
    batches: Vec<OrderProgressBatch>,
    last: AuditMap<String, OrderProgressBatch>,
}

#[cfg(feature = "bbtree-bench")]
impl super::super::bbtree_research::Workload for AuditIndexBuildWorkload {
    fn name(&self) -> &'static str {
        "audit_batch_index_build_clone"
    }

    fn size(&self) -> usize {
        self.batches.len()
    }

    fn run(&mut self) {
        let mut index = AuditMap::new();
        // Include input cloning explicitly: production receives owned rows from
        // its store. Insert incrementally as production does, not via collect's
        // potentially different sorted bulk builder. Replacing last drops the
        // preceding index during the timed call as well.
        for batch in self.batches.iter().cloned() {
            let batch_id = batch.batch_id.trim().to_string();
            if !batch_id.is_empty() {
                index.insert(batch_id, batch);
            }
        }
        self.last = std::hint::black_box(index);
    }

    fn fingerprint(&self) -> String {
        use sha2::{Digest, Sha256};
        let ordered = self.last.iter().collect::<Vec<_>>();
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&ordered).expect("audit index fingerprint"))
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bbtree_audit_valid_loaded_records_keep_empty_report() {
        let report = AuditFixture::new(32, false).validate();
        assert_eq!(
            report,
            ProductionWorkflowAuditReport {
                ok: true,
                checked_order_count: 32,
                checked_batch_count: 32,
                checked_session_count: 32,
                violations: Vec::new(),
            }
        );
    }

    #[test]
    fn bbtree_audit_missing_session_and_parent_keep_violation_order() {
        let fixture = AuditFixture::new(1, true);
        let report = fixture.validate();
        assert_eq!(
            report
                .violations
                .iter()
                .map(|item| item.code.as_str())
                .collect::<Vec<_>>(),
            [
                "progress_batch_session_not_found",
                "progress_batch_parent_not_found",
                "paused_session_progress_mismatch",
            ]
        );
        assert!(!report.ok);
    }

    #[test]
    fn bbtree_audit_rejects_cross_order_session_and_parent() {
        let mut fixture = AuditFixture::new(2, false);
        let parent = fixture.batches.get("batch-00000000").unwrap().clone();
        let batch = fixture.batches.get_mut("batch-00000001").unwrap();
        batch.session_id = parent.session_id.clone();
        batch.parent_batch_id = parent.batch_id;
        let report = fixture.validate();
        assert_eq!(
            report
                .violations
                .iter()
                .map(|item| item.code.as_str())
                .collect::<Vec<_>>(),
            [
                "progress_batch_session_order_mismatch",
                "progress_batch_parent_order_mismatch",
            ]
        );
    }

    #[test]
    fn bbtree_audit_paused_requeue_and_handoff_exemptions_survive() {
        let mut session = fixture_session("paused", "order");
        session.status = OrderRunStatus::Paused;
        let batches = AuditMap::new();
        let mut violations = Vec::new();
        audit_paused_session_progress(&[session.clone()], &batches, &mut violations);
        assert_eq!(violations[0].code, "paused_session_progress_mismatch");
        for exemption in [
            "requeued_at_tail",
            "worker_handoff",
            "roll_removed_from_apparatus",
        ] {
            session.payload_json = serde_json::json!({(exemption): true});
            let mut violations = Vec::new();
            audit_paused_session_progress(&[session.clone()], &batches, &mut violations);
            assert!(violations.is_empty(), "{exemption}");
        }
    }

    #[tokio::test]
    async fn bbtree_audit_complete_report_preserves_duplicate_qr_and_session_order() {
        use std::sync::Arc;
        let store = Arc::new(MemoryProductionMapStore::new());
        let service = ProductionMapService::new_for_test(store.clone());
        let order_id = "zakaz-audit-research-duplicates";
        store.put_map(fixture_map(order_id)).await.unwrap();
        store
            .put_apparatus_queue_states(
                APPARATUS,
                BTreeMap::from([(order_id.to_string(), "in_progress".to_string())]),
            )
            .await
            .unwrap();
        for index in 0..2 {
            let session_id = format!("session-{index}");
            let mut session = fixture_session(&session_id, order_id);
            session.status = OrderRunStatus::Active;
            store.put_order_run_session(session).await.unwrap();
            let mut batch = fixture_batch(&format!("batch-{index}"), order_id);
            batch.session_id = session_id;
            batch.qr_payload = if index == 0 {
                "DUPLICATE-QR"
            } else {
                "duplicate-qr"
            }
            .into();
            store.put_order_progress_batch(batch).await.unwrap();
        }
        let report = service.audit_production_workflow().await.unwrap();
        assert_eq!(
            (
                report.checked_order_count,
                report.checked_batch_count,
                report.checked_session_count
            ),
            (1, 2, 2)
        );
        assert_eq!(
            report
                .violations
                .iter()
                .map(|item| item.code.as_str())
                .collect::<Vec<_>>(),
            ["duplicate_qr_payload", "duplicate_active_order_session"]
        );
        assert_eq!(report.violations[0].subject, "DUPLICATE-QR");
        assert_eq!(
            report.violations[0].detail,
            "duplicate progress QR is used by batches: batch-0,batch-1"
        );
        assert_eq!(report, service.audit_production_workflow().await.unwrap());
    }
}
