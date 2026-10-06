use super::*;
use std::collections::BTreeSet;

const ORDER: &str = "qr-scoped-lineage";

fn batch(id: &str, apparatus: &str, parent: &str) -> OrderProgressBatch {
    let mut batch = test_progress_batch(
        id,
        ORDER,
        apparatus,
        &format!("qr-{id}"),
        OrderProgressBatchWipStatus::Waiting,
        parent,
    );
    batch.payload_json = serde_json::json!({"source_input_links": []});
    if !parent.is_empty() {
        set_sources(&mut batch, &[parent]);
    }
    batch
}

fn set_sources(batch: &mut OrderProgressBatch, ids: &[&str]) {
    batch.payload_json["source_input_links"] = serde_json::json!(
        ids.iter()
            .enumerate()
            .map(|(index, id)| ProgressBatchInputLink {
                input_batch_id: (*id).to_string(),
                input_qr_payload: format!("qr-{id}"),
                source_apparatus: FLOW_PECHAT_ID.to_string(),
                source_kind: OrderRunInputSourceKind::ProgressBatch,
                sequence_no: index as u32 + 1,
            })
            .collect::<Vec<_>>()
    );
}

fn session(batch: &OrderProgressBatch, input: &str) -> OrderRunSession {
    let mut session = OrderRunSession {
        session_id: batch.session_id.clone(),
        apparatus: batch.apparatus.clone(),
        order_id: ORDER.to_string(),
        stage_node_id: format!("stage-{}", batch.batch_id),
        status: OrderRunStatus::Completed,
        worker_role: batch.worker_role.clone(),
        worker_ref: batch.worker_ref.clone(),
        worker_display_name: batch.worker_display_name.clone(),
        started_at_unix: 1,
        updated_at_unix: 2,
        payload_json: serde_json::json!({"input_progress_batch_id": input, "rezka_output_report":[{"batch_id":"unrelated"}],"gross_qty":9999}),
    };
    crate::core::production_map::stage_execution::stamp_work_report(
        &mut session,
        &format!("report-{}", batch.batch_id),
        1,
        &QueueActionActor::default(),
        2,
    );
    session
}

async fn fixture(
    batches: Vec<OrderProgressBatch>,
    sessions: Vec<OrderRunSession>,
) -> (ProductionMapService, Arc<MemoryProductionMapStore>) {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    service
        .upsert_map(two_stage_map(ORDER, FLOW_PECHAT_ID, LAMINATION_1_ID))
        .await
        .expect("map");
    for session in sessions {
        store.put_order_run_session(session).await.expect("session");
    }
    for batch in batches {
        store.put_order_progress_batch(batch).await.expect("batch");
    }
    (service, store)
}

fn ids(batches: &[OrderProgressBatch]) -> BTreeSet<&str> {
    batches
        .iter()
        .map(|batch| batch.batch_id.as_str())
        .collect()
}

#[tokio::test]
async fn progress_qr_report_excludes_split_siblings_and_same_order_histories() {
    let root = batch("root", FLOW_PECHAT_ID, "");
    let scanned = batch("scanned", LAMINATION_1_ID, "root");
    let sibling = batch("sibling", LAMINATION_1_ID, "root");
    let final_output = batch("final", FLOW_PECHAT_ID, "scanned"); // repeated apparatus is a separate occurrence
    let unrelated = batch("unrelated", FLOW_PECHAT_ID, "");
    let batches = vec![root, scanned, sibling, final_output, unrelated];
    let sessions = batches
        .iter()
        .map(|batch| session(batch, &batch.parent_batch_id))
        .collect();
    let (service, store) = fixture(batches, sessions).await;
    for id in ["root", "scanned", "sibling", "final", "unrelated"] {
        service
            .correct_progress_batch(
                serde_json::from_value(serde_json::json!({
                    "batch_id": id, "expected_revision": 1, "produced_qty": 2.0,
                    "uom": "kg", "reason": "fixture"
                }))
                .expect("correction input"),
                &QueueActionActor {
                    role: "aparatchi".to_string(),
                    ref_: "worker".to_string(),
                    ..Default::default()
                },
            )
            .await
            .expect("correction");
        store
            .append_apparatus_queue_action_event(ApparatusQueueActionEvent {
                event_id: format!("report-{id}"),
                apparatus: FLOW_PECHAT_ID.to_string(),
                order_id: ORDER.to_string(),
                stage_node_id: String::new(),
                action: queue_state::ApparatusQueueAction::Complete,
                from_state: queue_state::ApparatusQueueOrderState::InProgress,
                to_state: queue_state::ApparatusQueueOrderState::Completed,
                policy: ApparatusQueuePolicy::FreePick,
                actor: QueueActionActor::default(),
                assigned_apparatus: vec![],
                payload_json: serde_json::json!({}),
            })
            .await
            .expect("log");
    }
    let before = store
        .progress_batches_for_order(ORDER)
        .await
        .expect("before");
    let report = service
        .progress_qr_report("scanned", "")
        .await
        .expect("report");
    assert_eq!(
        ids(&report.progress_batches),
        BTreeSet::from(["root", "scanned", "final"])
    );
    assert_eq!(ids(&report.current_batches), BTreeSet::from(["final"]));
    assert!(report.lineage_complete);
    assert_eq!(report.lineage_edges.len(), 2);
    assert_eq!(report.corrections.len(), 3);
    assert_eq!(report.logs.len(), 3);
    assert_eq!(report.run_sessions.len(), 3);
    assert_eq!(
        report
            .run_sessions
            .iter()
            .filter(|session| session.apparatus == FLOW_PECHAT_ID)
            .count(),
        2
    );
    assert!(report.run_sessions.iter().all(|session| {
        session.payload_json.get("rezka_output_report").is_none()
            && session.payload_json.get("gross_qty").is_none()
    }));
    assert_eq!(
        store
            .progress_batches_for_order(ORDER)
            .await
            .expect("after"),
        before,
        "report is read only"
    );
    let old_qr_report = service
        .progress_qr_report("root", "")
        .await
        .expect("old QR");
    assert_eq!(
        ids(&old_qr_report.current_batches),
        BTreeSet::from(["sibling", "final"])
    );
    assert!(old_qr_report.current_batch.is_none());
    assert!(old_qr_report.is_stale);
}

#[tokio::test]
async fn progress_qr_report_includes_real_merge_contributors_without_their_other_outputs() {
    let a = batch("a", FLOW_PECHAT_ID, "");
    let b = batch("b", FLOW_PECHAT_ID, "");
    let mut merged = batch("merged", LAMINATION_1_ID, "b");
    set_sources(&mut merged, &["a", "b"]);
    let sibling = batch("b-other-output", LAMINATION_1_ID, "b");
    // The last session input/nominal parent can be b while this output used only a.
    let mut a_only = batch("a-only", LAMINATION_1_ID, "b");
    set_sources(&mut a_only, &["a"]);
    let batches = vec![a, b, merged, sibling, a_only];
    let sessions = batches
        .iter()
        .map(|batch| session(batch, &batch.parent_batch_id))
        .collect();
    let (service, _) = fixture(batches, sessions).await;
    let report = service.progress_qr_report("a", "").await.expect("report");
    assert_eq!(
        ids(&report.progress_batches),
        BTreeSet::from(["a", "b", "merged", "a-only"])
    );
    assert_eq!(
        ids(&report.current_batches),
        BTreeSet::from(["merged", "a-only"])
    );
    assert_eq!(report.lineage_edges.len(), 3);
    assert!(report.current_batch.is_none());
    assert!(report.lineage_complete);
    let b_report = service.progress_qr_report("b", "").await.expect("b report");
    assert!(
        !ids(&b_report.progress_batches).contains("a-only"),
        "nominal last parent must not override measured output sources"
    );
}

#[tokio::test]
async fn progress_qr_report_never_infers_legacy_merge_outputs_from_shared_session() {
    let a = batch("a", FLOW_PECHAT_ID, "");
    let b = batch("b", FLOW_PECHAT_ID, "");
    let mut ambiguous = batch("ambiguous", LAMINATION_1_ID, "");
    ambiguous.payload_json = serde_json::json!({});
    let mut merge_session = session(&ambiguous, "b");
    merge_session.payload_json["input_lineage"] = serde_json::json!([
        {"input_batch_id":"a","input_qr_payload":"qr-a","source_apparatus":FLOW_PECHAT_ID,"source_kind":"progress_batch","stage_node_id":"stage","sequence_no":1,"status":"processed","linked_at_unix":1,"processed_at_unix":2},
        {"input_batch_id":"b","input_qr_payload":"qr-b","source_apparatus":FLOW_PECHAT_ID,"source_kind":"progress_batch","stage_node_id":"stage","sequence_no":2,"status":"processed","linked_at_unix":1,"processed_at_unix":2}
    ]);
    let (service, _) = fixture(
        vec![a.clone(), b.clone(), ambiguous],
        vec![session(&a, ""), session(&b, ""), merge_session],
    )
    .await;
    let report = service
        .progress_qr_report("ambiguous", "")
        .await
        .expect("report");
    assert_eq!(ids(&report.progress_batches), BTreeSet::from(["ambiguous"]));
    assert!(!report.lineage_complete);
    assert!(report.lineage_edges.is_empty());
    let report = service
        .progress_qr_report("a", "")
        .await
        .expect("input report");
    assert_eq!(ids(&report.progress_batches), BTreeSet::from(["a"]));
    assert_eq!(report.run_sessions.len(), 2);
    assert!(!report.lineage_complete);
    assert!(
        !report
            .run_sessions
            .iter()
            .any(|session| session.payload_json.to_string().contains("qr-b"))
    );
}

#[tokio::test]
async fn progress_qr_report_marks_missing_legacy_provenance_without_order_fallback() {
    let mut legacy = batch("legacy", LAMINATION_1_ID, "missing-parent");
    legacy.payload_json = serde_json::json!({});
    let unrelated = batch("unrelated", FLOW_PECHAT_ID, "");
    let (service, _) = fixture(
        vec![legacy, unrelated.clone()],
        vec![session(&unrelated, "")],
    )
    .await;
    let report = service
        .progress_qr_report("legacy", "")
        .await
        .expect("report");
    assert_eq!(ids(&report.progress_batches), BTreeSet::from(["legacy"]));
    assert_eq!(report.history_scope, "batch_lineage");
    assert!(!report.lineage_complete);
    assert!(report.run_sessions.is_empty());
    assert!(report.logs.is_empty());
}

#[tokio::test]
async fn progress_qr_report_accepts_single_session_input_evidence_and_flags_cycles() {
    let a = batch("a", FLOW_PECHAT_ID, "");
    let mut b = batch("b", LAMINATION_1_ID, "");
    b.payload_json = serde_json::json!({});
    let (service, store) = fixture(
        vec![a.clone(), b.clone()],
        vec![session(&a, ""), session(&b, "a")],
    )
    .await;
    let report = service
        .progress_qr_report("b", "")
        .await
        .expect("session provenance");
    assert_eq!(ids(&report.progress_batches), BTreeSet::from(["a", "b"]));
    assert!(report.lineage_complete);
    let mut cycle_a = a;
    set_sources(&mut cycle_a, &["b"]);
    store
        .put_order_progress_batch(cycle_a)
        .await
        .expect("cycle fixture");
    let report = service
        .progress_qr_report("b", "")
        .await
        .expect("cycle report");
    assert!(!report.lineage_complete);
    assert!(report.current_batches.is_empty());
    assert!(report.current_batch.is_none());
}

fn raw_material(barcode: &str, name: &str) -> ProductionQrRawMaterial {
    ProductionQrRawMaterial {
        barcode: barcode.to_string(),
        item_code: format!("item-{barcode}"),
        item_name: name.to_string(),
        stock_id: None,
        source_qty: None,
        uom: None,
    }
}

#[tokio::test]
async fn qr_resources_follow_exact_merge_sessions_without_other_outputs_or_live_assignments() {
    let a = batch("a", FLOW_PECHAT_ID, "");
    let b = batch("b", FLOW_PECHAT_ID, "");
    let unrelated = batch("unrelated", FLOW_PECHAT_ID, "");
    let mut merged = batch("merged", LAMINATION_1_ID, "b");
    set_sources(&mut merged, &["a", "b"]);
    let mut sessions: Vec<_> = [&a, &b, &unrelated, &merged]
        .into_iter()
        .map(|batch| session(batch, &batch.parent_batch_id))
        .collect();
    for (index, name) in ["Original film A", "Original film B", "Unrelated film"]
        .into_iter()
        .enumerate()
    {
        let resources = ProductionQrSessionResources {
            session_id: sessions[index].session_id.clone(),
            raw_materials: vec![raw_material("reused-barcode", name)],
            raw_materials_available: true,
            qolip_codes: vec![format!("plate-{index}")],
            qolip_available: true,
        };
        resources.write_to_session(&mut sessions[index]);
    }
    // An inherited plate code on a later apparatus is roll context, not usage.
    sessions[3].payload_json["qolip_codes"] = serde_json::json!(["plate-0", "plate-1"]);
    let (service, store) = fixture(vec![a, b, unrelated, merged], sessions).await;
    store
        .put_raw_material_assignment(RawMaterialAssignment {
            order_id: ORDER.to_string(),
            apparatus_id: ApparatusId::new(FLOW_PECHAT_ID).unwrap(),
            apparatus: FLOW_PECHAT_ID.to_string(),
            barcode: "reused-barcode".to_string(),
            item_code: "changed-plan".to_string(),
            item_name: "Live order assignment must not replace history".to_string(),
            item_group: String::new(),
            assigned_by_role: String::new(),
            assigned_by_ref: String::new(),
            assigned_by_display_name: String::new(),
            assigned_at: String::new(),
        })
        .await
        .unwrap();
    let report = service.progress_qr_report("merged", "").await.unwrap();
    assert_eq!(report.session_resources.len(), 3);
    let names = report
        .session_resources
        .iter()
        .flat_map(|resources| &resources.raw_materials)
        .map(|material| material.item_name.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        names,
        BTreeSet::from(["Original film A", "Original film B"])
    );
    let downstream = report
        .session_resources
        .iter()
        .find(|resources| resources.session_id == "session-merged")
        .unwrap();
    assert!(!downstream.raw_materials_available);
    assert!(!downstream.qolip_available);
    assert!(downstream.qolip_codes.is_empty());
}

#[tokio::test]
async fn qr_resources_mark_legacy_materials_unavailable_and_reject_wrong_session_snapshot() {
    let root = batch("legacy-root", FLOW_PECHAT_ID, "");
    let output = batch("output", LAMINATION_1_ID, "legacy-root");
    let mut root_session = session(&root, "");
    root_session.payload_json["qolip_code"] = serde_json::json!("ACTUAL-PLATE");
    let mut output_session = session(&output, "legacy-root");
    ProductionQrSessionResources {
        session_id: "different-session".to_string(),
        raw_materials: vec![raw_material("wrong-roll", "Must not leak")],
        raw_materials_available: true,
        qolip_codes: vec!["WRONG-PLATE".to_string()],
        qolip_available: true,
    }
    .write_to_session(&mut output_session);
    let (service, _) = fixture(vec![root, output], vec![root_session, output_session]).await;
    let report = service.progress_qr_report("output", "").await.unwrap();
    let root = report
        .session_resources
        .iter()
        .find(|resources| resources.session_id == "session-legacy-root")
        .unwrap();
    assert!(!root.raw_materials_available);
    assert_eq!(root.qolip_codes, ["ACTUAL-PLATE"]);
    assert!(root.qolip_available);
    let output = report
        .session_resources
        .iter()
        .find(|resources| resources.session_id == "session-output")
        .unwrap();
    assert!(!output.raw_materials_available);
    assert!(!output.qolip_available);
    assert!(output.raw_materials.is_empty());
    assert!(output.qolip_codes.is_empty());
}

#[tokio::test]
async fn qr_resources_survive_validated_start_pause_resume_and_complete() {
    use queue_state::ApparatusQueueAction as Action;
    const ORDER: &str = "zakaz-qr-resources";
    let store = Arc::new(MemoryProductionMapStore::new());
    let (service, apparatus_service) =
        service_with_apparatus_store(store.clone(), &[(FLOW_PECHAT_ID, "Print")]).await;
    service
        .upsert_map(two_stage_map(ORDER, FLOW_PECHAT_ID, LAMINATION_1_ID))
        .await
        .unwrap();
    set_test_material_rule(
        &apparatus_service,
        ApparatusMaterialRuleUpsert {
            apparatus: FLOW_PECHAT_ID.to_string(),
            requires_material: true,
            start_policy: RawMaterialStartPolicy::StateAll,
            item_groups: vec!["Film".to_string()],
            requirement_groups: Vec::new(),
        },
    )
    .await
    .unwrap();
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-resources".to_string(),
        ..Default::default()
    };
    for barcode in ["ACTUAL-RAW", "UNSCANNED-RAW"] {
        service
            .assign_raw_material_to_order(
                RawMaterialAssignmentInput {
                    order_id: ORDER.to_string(),
                    apparatus: FLOW_PECHAT_ID.to_string(),
                    barcode: barcode.to_string(),
                    item_code: "FILM".to_string(),
                    item_name: "Actual film".to_string(),
                    item_group: "Film".to_string(),
                    item_group_path: Vec::new(),
                },
                &actor,
            )
            .await
            .unwrap();
    }
    let assigned = [FLOW_PECHAT_ID.to_string()];
    let scans = ["ACTUAL-RAW".to_string()];
    let plate_codes = ["ACTUAL-PLATE".to_string()];
    let mut prepared = service
        .prepare_apparatus_queue_action_with_material_scan_and_progress(
            MaterialScanProgressAction {
                apparatus: FLOW_PECHAT_ID,
                order_id: ORDER,
                action: Action::Start,
                assigned_apparatus: &assigned,
                actor: actor.clone(),
                material_barcodes: &scans,
                state_material_barcodes: &scans,
                progress: QueueProgressInput::default(),
                qolip_validation: TrustedQolipStartValidation::from_preparations(
                    &ApparatusId::new(FLOW_PECHAT_ID).unwrap(),
                    ORDER,
                    &[QolipOrderStartPreparation {
                        spec: QolipProductSpec {
                            qolip_set_id: String::new(),
                            qolip_code: plate_codes[0].clone(),
                            ..Default::default()
                        },
                        checkout: None,
                    }],
                ),
            },
        )
        .await
        .unwrap();
    prepared.attach_qolip_codes(&plate_codes);
    let started = service
        .commit_prepared_queue_action(prepared)
        .await
        .unwrap()
        .session
        .unwrap();
    let expected = ProductionQrSessionResources::recorded(&started).unwrap();
    assert_eq!(expected.raw_materials.len(), 1);
    assert_eq!(expected.raw_materials[0].barcode, "ACTUAL-RAW");
    assert_eq!(expected.raw_materials[0].item_code, "FILM");
    assert_eq!(expected.raw_materials[0].item_name, "Actual film");
    assert!(expected.raw_materials_available);
    assert!(expected.qolip_available);
    assert_eq!(expected.qolip_codes, plate_codes);
    let before_assignments = store
        .raw_material_assignments_for_order(ORDER)
        .await
        .unwrap();
    let mut last_batch = None;
    for action in [Action::Pause, Action::Resume, Action::Complete] {
        let progress = if action == Action::Resume {
            QueueProgressInput::default()
        } else {
            QueueProgressInput {
                produced_qty: Some(12.0),
                uom: "kg".to_string(),
                return_ink_kg: Some(0.1),
                total_waste: Some(0.1),
                finished_goods_kg: Some(12.0),
                finished_goods_meter: Some(120.0),
                ..Default::default()
            }
        };
        let result = service
            .apply_apparatus_queue_action_with_progress(
                FLOW_PECHAT_ID,
                ORDER,
                action,
                &assigned,
                actor.clone(),
                progress,
            )
            .await
            .unwrap();
        let current = result.session.unwrap();
        assert_eq!(current.session_id, started.session_id);
        assert_eq!(
            ProductionQrSessionResources::recorded(&current),
            Some(expected.clone())
        );
        let stored = store
            .order_run_sessions_for_order(ORDER)
            .await
            .unwrap()
            .into_iter()
            .find(|session| session.session_id == started.session_id)
            .unwrap();
        assert_eq!(
            ProductionQrSessionResources::recorded(&stored),
            Some(expected.clone())
        );
        if result.progress_batch.is_some() {
            last_batch = result.progress_batch;
        }
    }
    let report = service
        .progress_qr_report(&last_batch.unwrap().batch_id, "")
        .await
        .unwrap();
    assert_eq!(report.session_resources, vec![expected]);
    assert_eq!(
        store
            .raw_material_assignments_for_order(ORDER)
            .await
            .unwrap(),
        before_assignments,
        "reading history never changes materials"
    );
}
