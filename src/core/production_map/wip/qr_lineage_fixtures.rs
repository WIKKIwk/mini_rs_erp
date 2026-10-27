//! Deterministic, database-free inputs for the real lineage implementation.
//! The same expectations run with std collections and with `bbtree-wip`.
use super::*;
use serde_json::{Value, json};

const ORDER: &str = "zakaz-bbtree-lineage";
const APPARATUS: &str = "apparatus:bbtree:fixture";

struct LineageFixture {
    scanned: OrderProgressBatch,
    batches: Vec<OrderProgressBatch>,
    sessions: Vec<OrderRunSession>,
}

fn batch(id: &str, session: &str, parents: &[&str]) -> OrderProgressBatch {
    let links = parents
        .iter()
        .enumerate()
        .map(|(index, id)| {
            json!({
                "input_batch_id": id, "input_qr_payload": format!("qr:{id}"),
                "source_apparatus": APPARATUS, "source_kind": "progress_batch",
                "sequence_no": index + 1,
            })
        })
        .collect::<Vec<_>>();
    serde_json::from_value(json!({
        "batch_id": id, "session_id": session, "started_at_unix": 1,
        "completed_at_unix": 2, "apparatus": APPARATUS, "order_id": ORDER,
        "action": "complete", "status": "completed", "produced_qty": 10.0,
        "uom": "m", "qr_payload": format!("qr:{id}"), "label_item_code": "FIXTURE",
        "label_item_name": "Synthetic fixture", "executor_name": "Fixture",
        "worker_role": "aparatchi", "worker_ref": "fixture", "worker_display_name": "Fixture",
        "wip_status": "waiting", "parent_batch_id": parents.last().copied().unwrap_or_default(),
        "payload_json": {"source_input_links": links}
    }))
    .expect("synthetic lineage batch must decode")
}

fn session(id: &str, parents: &[&str]) -> OrderRunSession {
    let links = parents
        .iter()
        .enumerate()
        .map(|(index, id)| {
            json!({
                "input_batch_id": id, "input_qr_payload": format!("qr:{id}"),
                "source_apparatus": APPARATUS, "source_kind": "progress_batch",
                "stage_node_id": "fixture-stage", "sequence_no": index + 1,
                "status": "processed", "linked_at_unix": 1, "processed_at_unix": 2,
            })
        })
        .collect::<Vec<_>>();
    OrderRunSession {
        session_id: id.into(),
        apparatus: APPARATUS.into(),
        order_id: ORDER.into(),
        stage_node_id: "fixture-stage".into(),
        status: OrderRunStatus::Completed,
        worker_role: "aparatchi".into(),
        worker_ref: "fixture".into(),
        worker_display_name: "Fixture".into(),
        started_at_unix: 1,
        updated_at_unix: 2,
        payload_json: json!({"input_lineage": links}),
    }
}

fn consume(batch: &mut OrderProgressBatch, consumer: &str) {
    batch.wip_status = OrderProgressBatchWipStatus::Processed;
    batch.processed_by_session_id = consumer.into();
    batch.processed_by_apparatus = APPARATUS.into();
}

impl LineageFixture {
    fn chain(size: usize) -> Self {
        assert!(size > 0);
        let mut batches = Vec::with_capacity(size);
        let mut sessions = Vec::with_capacity(size);
        for index in 0..size {
            let id = format!("batch-{index:08}");
            let session_id = format!("session-{index:08}");
            let previous = format!("batch-{:08}", index.saturating_sub(1));
            let parents = if index == 0 {
                Vec::new()
            } else {
                vec![previous.as_str()]
            };
            if let Some(previous) = batches.last_mut() {
                consume(previous, &session_id);
            }
            batches.push(batch(&id, &session_id, &parents));
            sessions.push(session(&session_id, &parents));
        }
        Self {
            scanned: batches[0].clone(),
            batches,
            sessions,
        }
    }

    fn split_merge(rounds: usize) -> Self {
        let mut fixture = Self::chain(1);
        for round in 0..rounds {
            let source = fixture.batches.last().unwrap().batch_id.clone();
            let left = format!("split-{round:08}-left");
            let right = format!("split-{round:08}-right");
            let merged = format!("split-{round:08}-merged");
            let split_session = format!("split-session-{round:08}");
            let merge_session = format!("merge-session-{round:08}");
            consume(fixture.batches.last_mut().unwrap(), &split_session);
            fixture.sessions.push(session(&split_session, &[&source]));
            let mut left_batch = batch(&left, &split_session, &[&source]);
            let mut right_batch = batch(&right, &split_session, &[&source]);
            consume(&mut left_batch, &merge_session);
            consume(&mut right_batch, &merge_session);
            fixture.batches.extend([left_batch, right_batch]);
            fixture
                .sessions
                .push(session(&merge_session, &[&left, &right]));
            fixture
                .batches
                .push(batch(&merged, &merge_session, &[&left, &right]));
        }
        fixture.scanned = fixture.batches[0].clone();
        fixture
    }

    fn calculate(&self) -> BatchLineage {
        batch_lineage(&self.scanned, &self.batches, &self.sessions)
    }

    fn reverse_rows(&mut self) {
        self.batches.reverse();
        self.sessions.reverse();
    }

    fn permute_rows(&mut self) {
        fn permute<T>(rows: &mut [T], mut state: u64) {
            for index in (1..rows.len()).rev() {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                rows.swap(index, (state as usize) % (index + 1));
            }
        }
        permute(&mut self.batches, 0xa58f_6317_93b2_4d0c);
        permute(&mut self.sessions, 0x6b8b_4567_327b_23c6);
    }
}

fn summary(lineage: &BatchLineage) -> Value {
    json!({
        "batch_ids": &lineage.batch_ids,
        "descendant_ids": &lineage.descendant_ids,
        "frontier_ids": &lineage.frontier_ids,
        "session_ids": &lineage.session_ids,
        "edges": &lineage.edges,
        "complete": lineage.complete,
    })
}

#[cfg(feature = "bbtree-bench")]
struct LineageWorkload {
    name: &'static str,
    fixture: LineageFixture,
    last: Option<BatchLineage>,
}

#[cfg(feature = "bbtree-bench")]
impl crate::core::production_map::bbtree_research::Workload for LineageWorkload {
    fn name(&self) -> &'static str {
        self.name
    }
    fn size(&self) -> usize {
        self.fixture.batches.len()
    }
    fn run(&mut self) {
        self.last = Some(std::hint::black_box(self.fixture.calculate()));
    }
    fn fingerprint(&self) -> String {
        use sha2::{Digest, Sha256};
        let result = self
            .last
            .as_ref()
            .expect("workload must run before fingerprinting");
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&summary(result)).unwrap())
        )
    }
}

#[cfg(feature = "bbtree-bench")]
pub(super) fn research_workloads()
-> Vec<Box<dyn crate::core::production_map::bbtree_research::Workload>> {
    let mut workloads: Vec<Box<dyn crate::core::production_map::bbtree_research::Workload>> =
        Vec::new();
    for size in [64, 512, 2048] {
        workloads.push(Box::new(LineageWorkload {
            name: "wip_lineage_chain",
            fixture: LineageFixture::chain(size),
            last: None,
        }));
        // PostgreSQL readers order rows by update/creation time rather than
        // ascending identity. Reverse order is prepared before measurement.
        let mut reverse = LineageFixture::chain(size);
        reverse.reverse_rows();
        workloads.push(Box::new(LineageWorkload {
            name: "wip_lineage_chain_reverse",
            fixture: reverse,
            last: None,
        }));
    }
    for rounds in [16, 128, 512] {
        workloads.push(Box::new(LineageWorkload {
            name: "wip_lineage_split_merge",
            fixture: LineageFixture::split_merge(rounds),
            last: None,
        }));
        let mut permuted = LineageFixture::split_merge(rounds);
        permuted.permute_rows();
        workloads.push(Box::new(LineageWorkload {
            name: "wip_lineage_split_merge_permuted",
            fixture: permuted,
            last: None,
        }));
    }
    workloads
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bbtree_lineage_chain_keeps_complete_ordered_history() {
        let fixture = LineageFixture::chain(64);
        let result = fixture.calculate();
        assert!(result.complete);
        assert_eq!(result.batch_ids.len(), 64);
        assert_eq!(result.descendant_ids, result.batch_ids);
        assert_eq!(result.session_ids.len(), 64);
        assert_eq!(
            result.frontier_ids,
            BTreeSet::from(["batch-00000063".into()])
        );
        let expected_edges = (1..64)
            .map(|index| ProductionQrLineageEdge {
                parent_batch_id: format!("batch-{:08}", index - 1),
                child_batch_id: format!("batch-{index:08}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(result.edges, expected_edges);
    }

    #[test]
    fn bbtree_lineage_scanned_split_child_excludes_unrelated_sibling() {
        let mut fixture = LineageFixture::chain(1);
        fixture.sessions.push(session("split", &["batch-00000000"]));
        fixture
            .batches
            .push(batch("left", "split", &["batch-00000000"]));
        fixture
            .batches
            .push(batch("right", "split", &["batch-00000000"]));
        fixture.scanned = fixture.batches[1].clone();
        let result = fixture.calculate();
        assert!(result.complete);
        assert_eq!(
            result.batch_ids,
            BTreeSet::from(["batch-00000000".into(), "left".into()])
        );
        assert_eq!(result.descendant_ids, BTreeSet::from(["left".into()]));
        assert_eq!(result.frontier_ids, BTreeSet::from(["left".into()]));
        assert_eq!(
            result.edges,
            vec![ProductionQrLineageEdge {
                parent_batch_id: "batch-00000000".into(),
                child_batch_id: "left".into(),
            }]
        );
    }

    #[test]
    fn bbtree_lineage_merge_retains_every_proven_input() {
        let mut fixture = LineageFixture::split_merge(1);
        fixture.scanned = fixture.batches.last().unwrap().clone();
        let result = fixture.calculate();
        assert!(result.complete);
        assert_eq!(result.batch_ids.len(), 4);
        assert_eq!(result.session_ids.len(), 3);
        assert_eq!(
            result.descendant_ids,
            BTreeSet::from(["split-00000000-merged".into()])
        );
        assert_eq!(result.frontier_ids, result.descendant_ids);
        assert_eq!(result.edges.len(), 4);
    }

    #[test]
    fn bbtree_lineage_layer_remainder_uses_immediate_physical_parent() {
        let mut fixture = LineageFixture::chain(3);
        fixture.scanned = fixture.batches[2].clone();
        fixture.batches[2].payload_json["origin"] = json!("layer_remainder");
        fixture.batches[2].payload_json["source_input_links"] =
            batch("unused", "unused", &["batch-00000000"]).payload_json["source_input_links"]
                .clone();
        let result = fixture.calculate();
        assert!(result.complete);
        assert_eq!(
            result.edges,
            vec![
                ProductionQrLineageEdge {
                    parent_batch_id: "batch-00000000".into(),
                    child_batch_id: "batch-00000001".into()
                },
                ProductionQrLineageEdge {
                    parent_batch_id: "batch-00000001".into(),
                    child_batch_id: "batch-00000002".into()
                },
            ]
        );
    }

    #[test]
    fn bbtree_lineage_missing_producer_or_input_is_incomplete() {
        let mut fixture = LineageFixture::chain(3);
        fixture.sessions.remove(1);
        assert!(!fixture.calculate().complete);
        let mut fixture = LineageFixture::chain(3);
        fixture.batches[1].payload_json["source_input_links"][0]["input_batch_id"] =
            json!("missing");
        assert!(!fixture.calculate().complete);
    }

    #[test]
    fn bbtree_lineage_corrupt_input_payload_is_incomplete() {
        let mut fixture = LineageFixture::chain(3);
        fixture.sessions[1].payload_json["input_lineage"] = json!({"invalid": true});
        assert!(!fixture.calculate().complete);
        let mut fixture = LineageFixture::chain(3);
        fixture.batches[1].payload_json["source_input_links"] = json!({"invalid": true});
        assert!(!fixture.calculate().complete);
    }

    #[test]
    fn bbtree_lineage_cycle_never_reports_a_complete_current_roll() {
        let mut fixture = LineageFixture::chain(3);
        fixture.batches[0].payload_json["source_input_links"] =
            batch("unused", "unused", &["batch-00000002"]).payload_json["source_input_links"]
                .clone();
        let result = fixture.calculate();
        assert!(!result.complete);
        assert_eq!(result.batch_ids.len(), 3);
        assert!(result.frontier_ids.is_empty());
    }

    #[test]
    fn bbtree_lineage_warehouse_receipt_is_a_valid_trace_boundary() {
        let mut fixture = LineageFixture::chain(1);
        consume(&mut fixture.batches[0], "warehouse-receipt");
        fixture.batches[0].processed_by_apparatus = "warehouse:finished".into();
        fixture.scanned = fixture.batches[0].clone();
        let result = fixture.calculate();
        assert!(result.complete);
        assert_eq!(result.frontier_ids, result.batch_ids);
        assert!(!result.session_ids.contains("warehouse-receipt"));
    }

    #[test]
    fn bbtree_lineage_output_is_stable_under_permutation_and_foreign_rows() {
        let mut fixture = LineageFixture::split_merge(16);
        let expected = summary(&fixture.calculate());
        fixture.reverse_rows();
        assert_eq!(summary(&fixture.calculate()), expected);
        fixture.permute_rows();
        let mut foreign_batch = fixture.batches[0].clone();
        foreign_batch.order_id = "foreign-order".into();
        fixture.batches.push(foreign_batch);
        let mut foreign_session = fixture.sessions[0].clone();
        foreign_session.order_id = "foreign-order".into();
        fixture.sessions.push(foreign_session);
        assert_eq!(summary(&fixture.calculate()), expected);
    }

    #[test]
    fn bbtree_lineage_duplicate_id_keeps_last_source_row() {
        let mut fixture = LineageFixture::chain(3);
        let mut replacement = fixture.batches[1].clone();
        replacement.session_id = "missing-producer".into();
        fixture.batches.push(replacement);
        assert!(!fixture.calculate().complete);
    }

    #[test]
    fn bbtree_lineage_duplicate_session_id_keeps_last_source_row() {
        let mut fixture = LineageFixture::chain(3);
        let valid = fixture.sessions[1].clone();
        let mut corrupt = valid.clone();
        corrupt.payload_json["input_lineage"] = json!({"invalid": true});
        fixture.sessions.push(corrupt);
        assert!(!fixture.calculate().complete);
        fixture.sessions.push(valid);
        assert!(fixture.calculate().complete);
    }

    #[test]
    fn bbtree_lineage_trimmed_batch_id_collision_keeps_last_source_row() {
        let mut fixture = LineageFixture::chain(3);
        let expected = summary(&fixture.calculate());
        let valid = fixture.batches[1].clone();
        let mut corrupt = valid.clone();
        corrupt.batch_id = format!(" \t{}  ", corrupt.batch_id);
        corrupt.session_id = "missing-producer".into();
        fixture.batches.push(corrupt);
        assert!(!fixture.calculate().complete);
        fixture.batches.push(valid);
        assert_eq!(summary(&fixture.calculate()), expected);
    }

    #[test]
    fn bbtree_lineage_trimmed_session_id_collision_keeps_last_source_row() {
        let mut fixture = LineageFixture::chain(3);
        let expected = summary(&fixture.calculate());
        let valid = fixture.sessions[1].clone();
        let mut corrupt = valid.clone();
        corrupt.session_id = format!(" \t{}  ", corrupt.session_id);
        corrupt.payload_json["input_lineage"] = json!({"invalid": true});
        fixture.sessions.push(corrupt);
        assert!(!fixture.calculate().complete);
        fixture.sessions.push(valid);
        assert_eq!(summary(&fixture.calculate()), expected);
    }
}
