use super::*;
use crate::core::apparatus_standard::test_support::runtime_configuration;
use serde_json::json;

const CUT2: &str = "apparatus:test:route-cut2";
// Worker execution and its returned order-status view use the real production
// order namespace; arbitrary map IDs are not production-order queue rows.
const ORDER: &str = "zakaz-0004-wip-route";
const QR: &str = "400118DA2F17C3617F59DDC6";

fn route_map(edited: bool) -> ProductionMapDefinition {
    let mut map: ProductionMapDefinition = serde_json::from_value(json!({
        "id": ORDER, "product_code": "lazer", "title": "Lazer guruch uzun don 2 kg",
        "nodes": [
            {"id": "start", "kind": "start", "title": "Start"},
            {"id": "lam1", "kind": "apparatus", "title": "Laminatsiya1", "apparatus_id": LAMINATION_1_ID},
            {"id": "rezka_5", "kind": "apparatus", "title": "Rezka", "apparatus_id": REZKA_ID, "rezka_kadr_count": 1},
            {"id": "end", "kind": "end", "title": "End"}
        ],
        "edges": [{"from": "start", "to": "lam1"}, {"from": "lam1", "to": "rezka_5"}, {"from": "rezka_5", "to": "end"}]
    })).unwrap();
    if edited {
        map.nodes[2].id = "apparatus_6".into();
        map.nodes[2].alternative_group_id = "alt_cut_6".into();
        let mut peer = map.nodes[2].clone();
        peer.id = "apparatus_7".into();
        peer.apparatus_id = CUT2.into();
        peer.title = "Rezka2".into();
        map.nodes.insert(3, peer);
        map.edges = vec![
            edge("start", "lam1"),
            edge("lam1", "apparatus_6"),
            edge("lam1", "apparatus_7"),
            edge("apparatus_6", "end"),
            edge("apparatus_7", "end"),
        ];
    }
    map
}

fn edge(from: &str, to: &str) -> ProductionMapEdge {
    ProductionMapEdge {
        from: from.into(),
        to: to.into(),
        branch: String::new(),
    }
}

fn repeated_cut_map() -> ProductionMapDefinition {
    let mut map = route_map(true);
    let mut first_cut = map.nodes[2].clone();
    first_cut.id = "rezka_before_lam".into();
    first_cut.alternative_group_id.clear();
    first_cut.title = "Rezka before lamination".into();
    map.nodes.insert(1, first_cut);
    map.edges
        .retain(|edge| !(edge.from == "start" && edge.to == "lam1"));
    map.edges.extend([
        edge("start", "rezka_before_lam"),
        edge("rezka_before_lam", "lam1"),
    ]);
    map
}

fn repeated_lamination_source_map() -> ProductionMapDefinition {
    let mut map = route_map(true);
    let mut earlier_lam1 = map.nodes[1].clone();
    earlier_lam1.id = "lam1_before".into();
    earlier_lam1.title = "Earlier Lam1 occurrence".into();
    let mut earlier_lam2 = earlier_lam1.clone();
    earlier_lam2.id = "lam2_before".into();
    earlier_lam2.apparatus_id = LAMINATION_2_ID.into();
    earlier_lam2.title = "Earlier Lam2 occurrence".into();
    map.nodes[1].alternative_group_id = "later_lamination".into();
    let mut later_lam2 = map.nodes[1].clone();
    later_lam2.id = "lam2".into();
    later_lam2.apparatus_id = LAMINATION_2_ID.into();
    later_lam2.title = "Later Lam2 occurrence".into();
    map.nodes.insert(1, earlier_lam1);
    map.nodes.insert(2, earlier_lam2);
    map.nodes.insert(4, later_lam2);
    map.edges = vec![
        edge("start", "lam1_before"),
        edge("lam1_before", "lam2_before"),
        edge("lam2_before", "lam1"),
        edge("lam2_before", "lam2"),
        edge("lam1", "apparatus_6"),
        edge("lam1", "apparatus_7"),
        edge("lam2", "apparatus_6"),
        edge("lam2", "apparatus_7"),
        edge("apparatus_6", "end"),
        edge("apparatus_7", "end"),
    ];
    map
}

fn repeated_lamination_target_map() -> ProductionMapDefinition {
    serde_json::from_value(json!({
        "id": ORDER, "product_code": "lazer", "title": "Repeated lamination target",
        "nodes": [
            {"id":"start", "kind":"start", "title":"Start"},
            {"id":"lam1_before", "kind":"apparatus", "title":"Earlier Lam1", "apparatus_id":LAMINATION_1_ID},
            {"id":"lam2_before", "kind":"apparatus", "title":"Earlier Lam2", "apparatus_id":LAMINATION_2_ID},
            {"id":"print", "kind":"apparatus", "title":"Flow print", "apparatus_id":FLOW_PECHAT_ID},
            {"id":"later_lam1", "kind":"apparatus", "title":"Later Lam1", "apparatus_id":LAMINATION_1_ID, "alternative_group_id":"later_lamination"},
            {"id":"later_lam2", "kind":"apparatus", "title":"Later Lam2", "apparatus_id":LAMINATION_2_ID, "alternative_group_id":"later_lamination"},
            {"id":"end", "kind":"end", "title":"End"}
        ],
        "edges": [{"from":"start", "to":"lam1_before"}, {"from":"lam1_before", "to":"lam2_before"},
            {"from":"lam2_before", "to":"print"}, {"from":"print", "to":"later_lam1"},
            {"from":"print", "to":"later_lam2"}, {"from":"later_lam1", "to":"end"}, {"from":"later_lam2", "to":"end"}]
    })).unwrap()
}

fn worker(id: &str) -> QueueActionActor {
    QueueActionActor {
        role: "aparatchi".into(),
        ref_: id.into(),
        display_name: id.into(),
    }
}

fn scan(batch: &OrderProgressBatch) -> QueueProgressInput {
    QueueProgressInput {
        progress_batch_id: batch.batch_id.clone(),
        qr_payload: batch.qr_payload.clone(),
        ..QueueProgressInput::default()
    }
}

async fn setup(
    discipline: QueueDiscipline,
) -> (
    ProductionMapService,
    Arc<MemoryProductionMapStore>,
    OrderProgressBatch,
) {
    setup_with_map(discipline, route_map(false)).await
}

async fn setup_with_map(
    discipline: QueueDiscipline,
    map: ProductionMapDefinition,
) -> (
    ProductionMapService,
    Arc<MemoryProductionMapStore>,
    OrderProgressBatch,
) {
    setup_with_source(discipline, map, LAMINATION_1_ID, "lam1").await
}

async fn setup_with_source(
    discipline: QueueDiscipline,
    map: ProductionMapDefinition,
    producer: &str,
    source_node: &str,
) -> (
    ProductionMapService,
    Arc<MemoryProductionMapStore>,
    OrderProgressBatch,
) {
    let store = Arc::new(MemoryProductionMapStore::new());
    let lam = runtime_configuration(TestApparatusSpec::laminate(LAMINATION_1_ID, "Laminatsiya1"));
    let lam2 = runtime_configuration(TestApparatusSpec::laminate(LAMINATION_2_ID, "Laminatsiya2"));
    let print = runtime_configuration(TestApparatusSpec::print(
        FLOW_PECHAT_ID,
        "Flow print",
        ProcessTechnology::Rotogravure,
        Some(7),
    ));
    let mut cut = runtime_configuration(TestApparatusSpec::cut(REZKA_ID, "Rezka"));
    let mut cut2 = runtime_configuration(TestApparatusSpec::cut(CUT2, "Rezka2"));
    cut.queue.discipline = discipline;
    cut2.queue.discipline = discipline;
    let service = ProductionMapService::new(
        store.clone(),
        Arc::new(TestCanonicalApparatusResolver::new([
            lam, lam2, print, cut, cut2,
        ])),
    );
    service.upsert_map(map).await.expect("fixture map");
    let mut batch = test_progress_batch(
        "physical-lam1-roll",
        ORDER,
        producer,
        QR,
        OrderProgressBatchWipStatus::Waiting,
        "",
    );
    batch.session_id = format!("active-{source_node}-session");
    batch.action = queue_state::ApparatusQueueAction::DetachRoll;
    batch.produced_qty = 6170.0;
    batch.uom = "m".into();
    batch.next_apparatus = REZKA_ID.into();
    batch.payload_json = json!({"stage_node_id": source_node, "next_stage_node_id": "rezka_5"});
    store
        .put_order_progress_batch(batch.clone())
        .await
        .expect("existing printed waiting roll");
    store
        .put_order_run_session(OrderRunSession {
            session_id: batch.session_id.clone(),
            apparatus: producer.into(),
            order_id: ORDER.into(),
            stage_node_id: source_node.into(),
            status: OrderRunStatus::Active,
            worker_role: "aparatchi".into(),
            worker_ref: "lam-worker".into(),
            worker_display_name: "Lam worker".into(),
            started_at_unix: 1,
            updated_at_unix: 2,
            payload_json: json!({}),
        })
        .await
        .expect("producer still active");
    store
        .put_apparatus_queue_states(
            producer,
            BTreeMap::from([(ORDER.into(), "in_progress".into())]),
        )
        .await
        .unwrap();
    (service, store, batch)
}

#[tokio::test]
async fn repeated_lamination_target_keeps_exact_pin_through_handoff_remove_and_resume() {
    let (service, store, mut original) = setup_with_source(
        QueueDiscipline::FreePick,
        repeated_lamination_target_map(),
        FLOW_PECHAT_ID,
        "print",
    )
    .await;
    original.next_apparatus = LAMINATION_1_ID.into();
    original.payload_json["next_stage_node_id"] = json!("lamination_old");
    store
        .put_order_progress_batch(original.clone())
        .await
        .unwrap();
    let actor = worker("later-lam2-worker");
    let assigned = [LAMINATION_2_ID.into()];
    let started = service
        .apply_apparatus_queue_action_with_progress(
            LAMINATION_2_ID,
            ORDER,
            queue_state::ApparatusQueueAction::Start,
            &assigned,
            actor.clone(),
            scan(&original),
        )
        .await
        .expect("printed old QR starts exact later Lam2 alternative");
    let session = started.session.unwrap();
    assert_eq!(session.stage_node_id, "later_lam2");
    let handoff = service
        .apply_apparatus_queue_action_with_progress(
            LAMINATION_2_ID,
            ORDER,
            queue_state::ApparatusQueueAction::Pause,
            &assigned,
            actor.clone(),
            QueueProgressInput {
                worker_handoff: true,
                lamination_print_leftover_rolls: Some(0.0),
                lamination_film_leftover_rolls: Some(0.0),
                total_waste: Some(0.0),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("handoff uses the exact pinned later consumer occurrence");
    assert_eq!(
        handoff.states.get(ORDER).map(String::as_str),
        Some("paused")
    );
    assert_eq!(handoff.session.unwrap().stage_node_id, "later_lam2");
    let handed_off = store
        .progress_batch(&original.batch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(handed_off.wip_status, OrderProgressBatchWipStatus::InUse);
    assert_eq!(handed_off.used_by_session_id, session.session_id);
    assert_eq!(handed_off.payload_json["worker_handoff"], true);
    assert_eq!(
        handed_off.payload_json["wip_route_binding"]["stage_node_id"],
        "later_lam2"
    );
    let removed = service
        .apply_apparatus_queue_action_with_progress(
            LAMINATION_2_ID,
            ORDER,
            queue_state::ApparatusQueueAction::DetachRoll,
            &assigned,
            actor.clone(),
            QueueProgressInput {
                remove_roll_from_apparatus: true,
                finished_goods_meter: Some(320.0),
                finished_goods_kg: Some(12.0),
                bobina_kg: Some(0.5),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("remove preserves exact route while releasing ownership");
    assert_eq!(
        removed.states.get(ORDER).map(String::as_str),
        Some("paused")
    );
    assert_eq!(removed.session.unwrap().stage_node_id, "later_lam2");
    let waiting = store
        .progress_batch(&original.batch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(waiting.wip_status, OrderProgressBatchWipStatus::Waiting);
    assert!(waiting.used_by_apparatus.is_empty());
    assert!(waiting.used_by_session_id.is_empty());
    assert_eq!(waiting.payload_json["roll_removed_from_apparatus"], true);
    assert_eq!(
        waiting.payload_json["wip_route_binding"]["stage_node_id"],
        "later_lam2"
    );
    assert_eq!(waiting.qr_payload, QR);
    let resumed = service
        .apply_apparatus_queue_action_with_progress(
            LAMINATION_2_ID,
            ORDER,
            queue_state::ApparatusQueueAction::Resume,
            &assigned,
            actor,
            QueueProgressInput::default(),
        )
        .await
        .expect("resume reclaims the same QR at the same later Lam2 occurrence");
    assert_eq!(
        resumed.states.get(ORDER).map(String::as_str),
        Some("in_progress")
    );
    let resumed_session = resumed.session.unwrap();
    assert_eq!(resumed_session.stage_node_id, "later_lam2");
    assert_eq!(resumed_session.session_id, session.session_id);
    let reclaimed = store
        .progress_batch(&original.batch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reclaimed.wip_status, OrderProgressBatchWipStatus::InUse);
    assert_eq!(reclaimed.used_by_apparatus, LAMINATION_2_ID);
    assert_eq!(reclaimed.used_by_session_id, session.session_id);
    assert_eq!(reclaimed.payload_json["worker_handoff"], false);
    assert_eq!(reclaimed.payload_json["stage_node_id"], "print");
    assert_eq!(
        reclaimed.payload_json["next_stage_node_id"],
        "lamination_old"
    );
    assert_eq!(
        reclaimed.payload_json["wip_route_binding"]["stage_node_id"],
        "later_lam2"
    );
    assert_eq!(reclaimed.qr_payload, QR);
}

#[tokio::test]
async fn repeated_laminator_source_controls_start_and_linked_completion_use_the_exact_later_occurrence()
 {
    let (service, store, original) = setup_with_source(
        QueueDiscipline::FreePick,
        repeated_lamination_source_map(),
        LAMINATION_2_ID,
        "lam2",
    )
    .await;
    let accepted = service
        .start_input_for_qr(CUT2, ORDER, "", QR)
        .await
        .unwrap();
    let route = service.progress_batch_input_route(&accepted).await.unwrap();
    assert_eq!(route.source_stage_node_id, "lam2");
    assert_eq!(route.stage_node_id, "apparatus_6");
    let controls = service.queue_action_controls().await.unwrap();
    let control = &controls[CUT2][ORDER];
    assert!(
        !control.previous_stage_ready,
        "the actual later Lam2 producer is still active"
    );
    assert!(
        control
            .allowed_actions
            .contains(&queue_state::ApparatusQueueAction::Start),
        "earlier distinct Lam1/Lam2 occurrences must not hide eligible later-group WIP"
    );
    let actor = worker("later-source-cut2-worker");
    let started = service
        .apply_apparatus_queue_action_with_progress(
            CUT2,
            ORDER,
            queue_state::ApparatusQueueAction::Start,
            &[CUT2.into()],
            actor.clone(),
            scan(&original),
        )
        .await
        .expect("start the exact later Lam2 output at the cutting peer");
    let session = started.session.unwrap();
    assert_eq!(session.stage_node_id, "apparatus_7");
    assert_eq!(
        session.payload_json["input_progress_apparatus"],
        LAMINATION_2_ID
    );
    let complete = QueueProgressInput {
        produced_qty: Some(6170.0),
        uom: "m".into(),
        finished_goods_kg: Some(100.0),
        finished_goods_meter: Some(6170.0),
        total_waste: Some(0.5),
        diameter: Some(40.0),
        ..QueueProgressInput::default()
    };
    // No explicit QR: this is the ordinary linked-input output path, distinct
    // from the explicit QR validation exercised by the other continuity test.
    let completed = service
        .apply_apparatus_queue_action_with_progress(
            CUT2,
            ORDER,
            queue_state::ApparatusQueueAction::Complete,
            &[CUT2.into()],
            actor,
            complete,
        )
        .await
        .expect("linked completion must use the pinned later producer occurrence");
    assert!(!completed.progress_batches.is_empty() || completed.progress_batch.is_some());
    let processed = store
        .progress_batch(&original.batch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(processed.wip_status, OrderProgressBatchWipStatus::Processed);
    assert_eq!(processed.apparatus, LAMINATION_2_ID);
    assert_eq!(processed.processed_by_apparatus, CUT2);
    assert_eq!(processed.processed_by_session_id, session.session_id);
    assert_eq!(processed.payload_json["stage_node_id"], "lam2");
    assert_eq!(processed.payload_json["next_stage_node_id"], "rezka_5");
    assert_eq!(
        processed.payload_json["wip_route_binding"]["source_stage_node_id"],
        "lam2"
    );
    assert_eq!(
        processed.payload_json["wip_route_binding"]["stage_node_id"],
        "apparatus_7"
    );
    assert_eq!(processed.qr_payload, QR);
}

#[tokio::test]
async fn repeated_cut_apparatus_old_qr_starts_only_at_the_later_alternative_occurrence() {
    let (service, store, original) =
        setup_with_map(QueueDiscipline::FreePick, repeated_cut_map()).await;
    let accepted = service
        .start_input_for_qr(CUT2, ORDER, "", QR)
        .await
        .unwrap();
    let route = service.progress_batch_input_route(&accepted).await.unwrap();
    assert_eq!(route.source_stage_node_id, "lam1");
    assert_eq!(route.stage_node_id, "apparatus_6");
    assert_eq!(
        route.consumer_apparatus_ids,
        vec![REZKA_ID.to_string(), CUT2.to_string()]
    );
    assert!(route.remapped);
    let started = service
        .apply_apparatus_queue_action_with_progress(
            CUT2,
            ORDER,
            queue_state::ApparatusQueueAction::Start,
            &[CUT2.into()],
            worker("later-cut2-worker"),
            scan(&original),
        )
        .await
        .expect("the earlier canonical Cut occurrence must not reject its later alternative peer");
    let session = started.session.unwrap();
    assert_eq!(session.stage_node_id, "apparatus_7");
    assert_eq!(
        session.payload_json["input_progress_batch_id"],
        original.batch_id
    );
    let claimed = store
        .progress_batch(&original.batch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed.payload_json["stage_node_id"], "lam1");
    assert_eq!(claimed.payload_json["next_stage_node_id"], "rezka_5");
    assert_eq!(
        claimed.payload_json["wip_route_binding"]["source_stage_node_id"],
        "lam1"
    );
    assert_eq!(
        claimed.payload_json["wip_route_binding"]["stage_node_id"],
        "apparatus_7"
    );
    assert_eq!(claimed.used_by_apparatus, CUT2);
    assert_eq!(claimed.qr_payload, QR);
    assert!(
        store
            .order_run_sessions_for_order(ORDER)
            .await
            .unwrap()
            .iter()
            .all(|session| session.stage_node_id != "rezka_before_lam"),
        "scanning a later roll must not start an earlier cutting occurrence"
    );
}

#[tokio::test]
async fn repeated_cut_apparatus_old_qr_merges_into_the_pinned_later_alternative_occurrence() {
    let (service, store, original) =
        setup_with_map(QueueDiscipline::FreePick, repeated_cut_map()).await;
    let actor = worker("later-cut2-worker");
    let started = service
        .apply_apparatus_queue_action_with_progress(
            CUT2,
            ORDER,
            queue_state::ApparatusQueueAction::Start,
            &[CUT2.into()],
            actor.clone(),
            scan(&original),
        )
        .await
        .expect("start exact later cutting occurrence");
    let first_session = started.session.unwrap();
    let mut next = original.clone();
    next.batch_id = "physical-lam1-second-roll".into();
    next.qr_payload = "400118DA2F17C3617F59DDC7".into();
    store.put_order_progress_batch(next.clone()).await.unwrap();
    let merged = service
        .apply_apparatus_queue_action_with_progress(
            CUT2,
            ORDER,
            queue_state::ApparatusQueueAction::Merge,
            &[CUT2.into()],
            actor,
            scan(&next),
        )
        .await
        .expect(
            "merge exact later-group input despite earlier occurrence of the canonical destination",
        );
    let session = merged.session.unwrap();
    assert_eq!(session.session_id, first_session.session_id);
    assert_eq!(session.stage_node_id, "apparatus_7");
    assert_eq!(
        session.payload_json["input_progress_batch_id"],
        next.batch_id
    );
    assert_eq!(
        session.payload_json["merge_from_input_batch_id"],
        original.batch_id
    );
    let old_input = store
        .progress_batch(&original.batch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(old_input.wip_status, OrderProgressBatchWipStatus::Processed);
    assert_eq!(
        old_input.payload_json["wip_route_binding"]["stage_node_id"],
        "apparatus_7"
    );
    let claimed = store.progress_batch(&next.batch_id).await.unwrap().unwrap();
    assert_eq!(claimed.wip_status, OrderProgressBatchWipStatus::InUse);
    assert_eq!(claimed.used_by_session_id, first_session.session_id);
    assert_eq!(claimed.used_by_apparatus, CUT2);
    assert_eq!(claimed.payload_json["next_stage_node_id"], "rezka_5");
    assert_eq!(
        claimed.payload_json["wip_route_binding"]["source_stage_node_id"],
        "lam1"
    );
    assert_eq!(
        claimed.payload_json["wip_route_binding"]["stage_node_id"],
        "apparatus_7"
    );
    assert_eq!(claimed.qr_payload, next.qr_payload);
}

#[tokio::test]
async fn edited_destination_old_physical_qr_previews_repeatedly_and_completes_at_pinned_cut2() {
    let (service, store, original) = setup(QueueDiscipline::FreePick).await;
    service
        .upsert_map(route_map(true))
        .await
        .expect("safe future-stage replacement");
    for _ in 0..3 {
        let accepted = service
            .start_input_for_qr(CUT2, ORDER, "", QR)
            .await
            .expect("same physical QR");
        assert_eq!(accepted.batch_id, original.batch_id);
        let route = service.progress_batch_input_route(&accepted).await.unwrap();
        assert!(route.remapped);
        assert_eq!(route.source_stage_node_id, "lam1");
        assert_eq!(
            route.consumer_apparatus_ids,
            vec![REZKA_ID.to_string(), CUT2.to_string()]
        );
    }
    assert_eq!(
        store
            .progress_batch(&original.batch_id)
            .await
            .unwrap()
            .unwrap(),
        original,
        "scan projection must never repair or rewrite historical output"
    );
    let controls = service.queue_action_controls().await.unwrap();
    let control = &controls[CUT2][ORDER];
    assert!(!control.previous_stage_ready, "lamination remains active");
    assert!(
        control
            .allowed_actions
            .contains(&queue_state::ApparatusQueueAction::Start)
    );

    let actor = worker("cut2-worker");
    let started = service
        .apply_apparatus_queue_action_with_progress(
            CUT2,
            ORDER,
            queue_state::ApparatusQueueAction::Start,
            &[CUT2.into()],
            actor.clone(),
            scan(&original),
        )
        .await
        .expect("Cut2 starts old printed roll");
    let session = started.session.expect("cutting session");
    assert_eq!(session.stage_node_id, "apparatus_7");
    let claimed = store
        .progress_batch(&original.batch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed.batch_id, original.batch_id);
    assert_eq!(claimed.qr_payload, QR);
    assert_eq!(claimed.produced_qty, 6170.0);
    assert_eq!(claimed.payload_json["stage_node_id"], "lam1");
    assert_eq!(claimed.payload_json["next_stage_node_id"], "rezka_5");
    assert_eq!(
        claimed.payload_json["wip_route_binding"]["stage_node_id"],
        "apparatus_7"
    );
    assert_eq!(claimed.used_by_session_id, session.session_id);
    assert_eq!(claimed.used_by_apparatus, CUT2);
    assert!(
        service
            .apply_apparatus_queue_action_with_progress(
                REZKA_ID,
                ORDER,
                queue_state::ApparatusQueueAction::Start,
                &[REZKA_ID.into()],
                worker("second-worker"),
                scan(&original)
            )
            .await
            .is_err(),
        "a second machine may not consume the same physical roll"
    );

    let mut complete = scan(&original);
    complete.produced_qty = Some(6170.0);
    complete.uom = "m".into();
    complete.finished_goods_kg = Some(100.0);
    complete.finished_goods_meter = Some(6170.0);
    // Reuse the ordinary cutting completion contract: one positive total
    // waste report satisfies this fixture without supplying invalid zero
    // values to the separately validated legacy waste measurements.
    complete.total_waste = Some(0.5);
    complete.diameter = Some(40.0);
    let completed = service
        .apply_apparatus_queue_action_with_progress(
            CUT2,
            ORDER,
            queue_state::ApparatusQueueAction::Complete,
            &[CUT2.into()],
            actor,
            complete,
        )
        .await
        .expect("explicit old QR validates against the pinned active route");
    assert!(!completed.progress_batches.is_empty() || completed.progress_batch.is_some());
    let processed = store
        .progress_batch(&original.batch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(processed.wip_status, OrderProgressBatchWipStatus::Processed);
    assert_eq!(processed.processed_by_apparatus, CUT2);
    assert_eq!(processed.qr_payload, QR);
    assert_eq!(processed.payload_json["next_stage_node_id"], "rezka_5");
    assert_eq!(
        processed.payload_json["wip_route_binding"]["stage_node_id"],
        "apparatus_7"
    );
}

#[tokio::test]
async fn old_destination_two_alternative_workers_cannot_claim_the_same_roll() {
    let (service, store, original) = setup(QueueDiscipline::FreePick).await;
    service.upsert_map(route_map(true)).await.unwrap();
    let cut_assignment = [REZKA_ID.into()];
    let cut2_assignment = [CUT2.into()];
    let (first, second) = tokio::join!(
        service.apply_apparatus_queue_action_with_progress(
            REZKA_ID,
            ORDER,
            queue_state::ApparatusQueueAction::Start,
            &cut_assignment,
            worker("cut-worker"),
            scan(&original)
        ),
        service.apply_apparatus_queue_action_with_progress(
            CUT2,
            ORDER,
            queue_state::ApparatusQueueAction::Start,
            &cut2_assignment,
            worker("cut2-worker"),
            scan(&original)
        ),
    );
    assert_eq!(
        usize::from(first.is_ok()) + usize::from(second.is_ok()),
        1,
        "claim is exclusive across alternative workers: {first:?}, {second:?}"
    );
    let claimed = store
        .progress_batch(&original.batch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed.wip_status, OrderProgressBatchWipStatus::InUse);
    let expected = if first.is_ok() {
        "apparatus_6"
    } else {
        "apparatus_7"
    };
    assert_eq!(
        claimed.payload_json["wip_route_binding"]["stage_node_id"],
        expected
    );
    assert_eq!(claimed.qr_payload, QR);
}

#[tokio::test]
async fn alternative_apparatus_on_same_order_starts_without_waiting_for_peer_roll() {
    let (service, store, original) = setup(QueueDiscipline::FreePick).await;
    service.upsert_map(route_map(true)).await.unwrap();
    let mut other = original.clone();
    other.batch_id = "physical-independent-lam1-roll".into();
    other.qr_payload = "400118DA2F17C3617F59DDC7".into();
    store.put_order_progress_batch(other.clone()).await.unwrap();
    let peer_guard = service.queue_progress_action_guard(
        REZKA_ID, queue_state::ApparatusQueueAction::Start, &scan(&original), &[], &[],
    ).await.unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_millis(500),
        service.apply_apparatus_queue_action_with_progress(
            CUT2, ORDER, queue_state::ApparatusQueueAction::Start,
            &[CUT2.into()], worker("independent-cut-worker"), scan(&other),
        ),
    ).await.expect("same group/order with a different apparatus and roll must proceed")
        .expect("independent cut start");
    assert_eq!(result.session.unwrap().apparatus, CUT2);
    assert_eq!(store.progress_batch(&original.batch_id).await.unwrap().unwrap().wip_status,
        OrderProgressBatchWipStatus::Waiting);
    assert_eq!(store.progress_batch(&other.batch_id).await.unwrap().unwrap().used_by_apparatus, CUT2);
    drop(peer_guard);
}

#[tokio::test]
async fn qr_only_and_explicit_batch_claims_share_the_same_roll_guard() {
    let (service, _, original) = setup(QueueDiscipline::FreePick).await;
    service.upsert_map(route_map(true)).await.unwrap();
    let explicit = service.queue_progress_action_guard(
        REZKA_ID, queue_state::ApparatusQueueAction::Start, &scan(&original), &[], &[],
    ).await.unwrap();
    let qr_only = QueueProgressInput { qr_payload: original.qr_payload.clone(), ..Default::default() };
    assert!(tokio::time::timeout(std::time::Duration::from_millis(30),
        service.queue_progress_action_guard(CUT2, queue_state::ApparatusQueueAction::Start,
            &qr_only, &[], &[]),
    ).await.is_err(), "the QR alias must resolve to the exclusive physical roll lock");
    drop(explicit);
    service.queue_progress_action_guard(CUT2, queue_state::ApparatusQueueAction::Start,
        &qr_only, &[], &[]).await.unwrap();
}

#[tokio::test]
async fn map_save_protects_waiting_route_from_ambiguous_or_deleted_producer_edit() {
    let (service, store, original) = setup(QueueDiscipline::FreePick).await;
    let before = service.raw_map(ORDER).await.unwrap().unwrap();
    let mut ambiguous = route_map(true);
    ambiguous
        .nodes
        .iter_mut()
        .find(|n| n.id == "apparatus_7")
        .unwrap()
        .alternative_group_id = "distinct-cut-occurrence".into();
    assert_eq!(
        service.upsert_map(ambiguous).await,
        Err(ProductionMapError::WipRouteAmbiguous)
    );
    let mut deleted_source = route_map(true);
    deleted_source
        .nodes
        .iter_mut()
        .find(|n| n.id == "lam1")
        .unwrap()
        .id = "replacement-lam".into();
    for edge in &mut deleted_source.edges {
        if edge.from == "lam1" {
            edge.from = "replacement-lam".into();
        }
        if edge.to == "lam1" {
            edge.to = "replacement-lam".into();
        }
    }
    assert_eq!(
        service.upsert_map(deleted_source).await,
        Err(ProductionMapError::WipRouteSourceUnresolved)
    );
    assert_eq!(service.raw_map(ORDER).await.unwrap().unwrap(), before);
    assert_eq!(
        store
            .progress_batch(&original.batch_id)
            .await
            .unwrap()
            .unwrap(),
        original
    );
    service
        .upsert_map(route_map(true))
        .await
        .expect("safe same-source alternative edit stays permitted");
}

#[tokio::test]
async fn already_edited_legacy_map_with_deleted_producer_reports_precise_route_error() {
    let (service, store, original) = setup(QueueDiscipline::FreePick).await;
    let mut broken = route_map(true);
    broken.nodes.iter_mut().find(|n| n.id == "lam1").unwrap().id = "replacement-lam".into();
    for edge in &mut broken.edges {
        if edge.from == "lam1" {
            edge.from = "replacement-lam".into();
        }
        if edge.to == "lam1" {
            edge.to = "replacement-lam".into();
        }
    }
    // Existing legacy storage may predate the save guard. Reads must fail closed.
    store.put_map(broken).await.unwrap();
    assert_eq!(
        service.start_input_for_qr(CUT2, ORDER, "", QR).await,
        Err(ProductionMapError::WipRouteSourceUnresolved)
    );
    assert_eq!(
        store
            .progress_batch(&original.batch_id)
            .await
            .unwrap()
            .unwrap(),
        original
    );
}

#[tokio::test]
async fn valid_explicit_destination_is_not_broadened_to_an_unrelated_cutting_branch() {
    let (service, store, original) = setup(QueueDiscipline::FreePick).await;
    let mut expanded = route_map(false);
    let mut other = expanded.nodes[2].clone();
    other.id = "unrelated-cut2-occurrence".into();
    other.apparatus_id = CUT2.into();
    other.title = "Rezka2".into();
    other.alternative_group_id = "separate-work".into();
    expanded.nodes.insert(3, other);
    expanded
        .edges
        .push(edge("lam1", "unrelated-cut2-occurrence"));
    expanded
        .edges
        .push(edge("unrelated-cut2-occurrence", "end"));
    service
        .upsert_map(expanded)
        .await
        .expect("explicit original destination remains valid");
    let accepted = service
        .start_input_for_qr(REZKA_ID, ORDER, "", QR)
        .await
        .unwrap();
    let route = service.progress_batch_input_route(&accepted).await.unwrap();
    assert_eq!(route.stage_node_id, "rezka_5");
    assert_eq!(route.consumer_apparatus_ids, vec![REZKA_ID.to_string()]);
    assert!(!route.remapped);
    assert_eq!(
        service.start_input_for_qr(CUT2, ORDER, "", QR).await,
        Err(ProductionMapError::ProgressBatchNotAccepted)
    );
    assert_eq!(
        service
            .apply_apparatus_queue_action_with_progress(
                CUT2,
                ORDER,
                queue_state::ApparatusQueueAction::Start,
                &[CUT2.into()],
                worker("unrelated-worker"),
                scan(&original)
            )
            .await,
        Err(ProductionMapError::ProgressBatchNotAccepted)
    );
    assert_eq!(
        store
            .progress_batch(&original.batch_id)
            .await
            .unwrap()
            .unwrap(),
        original
    );
}

#[tokio::test]
async fn stale_destination_cannot_recover_an_already_owned_or_processed_physical_roll() {
    let (service, store, original) = setup(QueueDiscipline::FreePick).await;
    service.upsert_map(route_map(true)).await.unwrap();
    for status in [
        OrderProgressBatchWipStatus::InUse,
        OrderProgressBatchWipStatus::Processed,
    ] {
        let mut batch = original.clone();
        batch.wip_status = status;
        batch.used_by_apparatus = REZKA_ID.into();
        batch.used_by_session_id = "existing-cut-owner".into();
        batch.current_apparatus = REZKA_ID.into();
        if status == OrderProgressBatchWipStatus::Processed {
            batch.processed_by_apparatus = REZKA_ID.into();
            batch.processed_by_session_id = "existing-cut-owner".into();
        }
        store.put_order_progress_batch(batch.clone()).await.unwrap();
        assert!(
            service
                .start_input_for_qr(CUT2, ORDER, "", QR)
                .await
                .is_err()
        );
        assert!(
            service
                .apply_apparatus_queue_action_with_progress(
                    CUT2,
                    ORDER,
                    queue_state::ApparatusQueueAction::Start,
                    &[CUT2.into()],
                    worker("new-worker"),
                    scan(&original)
                )
                .await
                .is_err()
        );
        assert_eq!(
            store
                .progress_batch(&batch.batch_id)
                .await
                .unwrap()
                .unwrap(),
            batch,
            "rejecting an old QR must preserve prior ownership and processing"
        );
    }
}

#[tokio::test]
async fn remapped_wip_keeps_worker_assignment_and_strict_sequence_restrictions() {
    for discipline in [QueueDiscipline::StrictSequence, QueueDiscipline::FreePick] {
        let (service, store, original) = setup(discipline).await;
        service.upsert_map(route_map(true)).await.unwrap();
        let head = "zakaz-route-cut2-head";
        let mut head_map = canonical_apparatus_stage_map(head, CUT2, "Rezka2");
        head_map
            .nodes
            .iter_mut()
            .find(|n| n.id == "apparatus")
            .unwrap()
            .rezka_kadr_count = Some(1);
        service.upsert_map(head_map).await.unwrap();
        service
            .set_apparatus_sequence(CUT2, vec![head.into(), ORDER.into()])
            .await
            .unwrap();
        assert_eq!(
            service
                .apply_apparatus_queue_action_with_progress(
                    CUT2,
                    ORDER,
                    queue_state::ApparatusQueueAction::Start,
                    &[REZKA_ID.into()],
                    worker("unassigned"),
                    scan(&original)
                )
                .await,
            Err(ProductionMapError::ApparatusNotAssigned)
        );
        let controls = service.queue_action_controls().await.unwrap();
        let control = &controls[CUT2][ORDER];
        let free = discipline == QueueDiscipline::FreePick;
        assert_eq!(
            control
                .allowed_actions
                .contains(&queue_state::ApparatusQueueAction::Start),
            free
        );
        assert_eq!(
            control.interaction.blocking_reason_code,
            if free { "" } else { "waiting_sequence" }
        );
        let started = service
            .apply_apparatus_queue_action_with_progress(
                CUT2,
                ORDER,
                queue_state::ApparatusQueueAction::Start,
                &[CUT2.into()],
                worker("assigned"),
                scan(&original),
            )
            .await;
        if free {
            assert!(
                started.is_ok(),
                "free-pick has eligible individual WIP: {started:?}"
            );
        } else {
            assert_eq!(started, Err(ProductionMapError::QueueActionNotAllowed));
        }
        assert_eq!(
            store
                .progress_batch(&original.batch_id)
                .await
                .unwrap()
                .unwrap()
                .wip_status,
            if free {
                OrderProgressBatchWipStatus::InUse
            } else {
                OrderProgressBatchWipStatus::Waiting
            }
        );
    }
}
