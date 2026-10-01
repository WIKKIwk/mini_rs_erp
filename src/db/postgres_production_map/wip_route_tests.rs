use super::*;
use std::{collections::BTreeMap, sync::Arc, time::Duration};

use crate::core::apparatus_standard::{
    ApparatusId,
    service::CanonicalApparatusService,
    test_support::{TestApparatusSpec, canonical_draft, standard_revisions},
};
use crate::db::{
    postgres::{
        apply_foundation_migration, apply_postgres_migrations_through_version,
        postgres_test_database_options,
    },
    postgres_canonical_apparatus::PostgresCanonicalApparatusRepository,
    postgres_production_map::PostgresProductionMapStore,
};

const ORDER: &str = "zakaz-route-0004";
const LAM: &str = "apparatus:default:asset-007";
const CUT: &str = "apparatus:default:asset-010";
const CUT2: &str = "apparatus:test:route-cut2";
const QR: &str = "400118DA2F17C3617F59DDC6";

async fn fixture() -> (PgPool, Arc<PostgresProductionMapStore>, PgPool, String) {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .expect("an explicitly isolated PostgreSQL test URL is required");
    let db_name = format!("mini_rs_erp_test_wip_route_{:08x}", rand::random::<u32>());
    let admin = PgPool::connect(&admin_url).await.unwrap();
    sqlx::query(&format!("CREATE DATABASE \"{db_name}\""))
        .execute(&admin)
        .await
        .unwrap();
    let pool = PgPool::connect_with(postgres_test_database_options(&admin_url, &db_name))
        .await
        .unwrap();
    apply_postgres_migrations_through_version(&pool, "0121")
        .await
        .unwrap();
    let canonical = CanonicalApparatusService::new(Arc::new(
        PostgresCanonicalApparatusRepository::new(pool.clone()),
    ));
    for revision in standard_revisions() {
        canonical
            .seed_for_test(revision.apparatus_id.clone(), revision.to_draft())
            .await
            .unwrap();
    }
    canonical
        .seed_for_test(
            ApparatusId::new(CUT2).unwrap(),
            canonical_draft(&TestApparatusSpec::cut(CUT2, "Rezka 2")),
        )
        .await
        .unwrap();
    apply_foundation_migration(&pool).await.unwrap();
    let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
    (pool, store, admin, db_name)
}

async fn cleanup(
    pool: PgPool,
    store: Arc<PostgresProductionMapStore>,
    admin: PgPool,
    name: String,
) {
    drop(store);
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE \"{name}\" WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}

fn old_map() -> ProductionMapDefinition {
    serde_json::from_value(serde_json::json!({
        "id":ORDER,"product_code":"P","title":"Lazer guruch uzun don 2 kg",
        "nodes":[
            {"id":"start","kind":"start","title":"Start"},
            {"id":"lamination_1","kind":"apparatus","title":"Laminatsiya 1","apparatus_id":LAM},
            {"id":"rezka_5","kind":"apparatus","title":"Rezka","apparatus_id":CUT,"rezka_kadr_count":1},
            {"id":"end","kind":"end","title":"End"}
        ],"edges":[
            {"from":"start","to":"lamination_1"},
            {"from":"lamination_1","to":"rezka_5"},
            {"from":"rezka_5","to":"end"}
        ]
    })).unwrap()
}

fn alternative_map() -> ProductionMapDefinition {
    let mut map = old_map();
    map.nodes[2].id = "apparatus_6".into();
    map.nodes[2].alternative_group_id = "alt_cut_6".into();
    let mut sibling = map.nodes[2].clone();
    sibling.id = "apparatus_7".into();
    sibling.apparatus_id = CUT2.into();
    sibling.title = "Rezka 2".into();
    map.nodes.insert(3, sibling);
    map.edges = serde_json::from_value(serde_json::json!([
        {"from":"start","to":"lamination_1"},
        {"from":"lamination_1","to":"apparatus_6"},
        {"from":"lamination_1","to":"apparatus_7"},
        {"from":"apparatus_6","to":"end"},
        {"from":"apparatus_7","to":"end"}
    ]))
    .unwrap();
    map
}

fn original_batch() -> OrderProgressBatch {
    let mut batch: OrderProgressBatch = serde_json::from_value(serde_json::json!({
        "batch_id":"reported-route-roll","session_id":"lamination-output-session",
        "started_at_unix":0,"completed_at_unix":0,"apparatus":LAM,"order_id":ORDER,
        "action":"detach_roll","status":"roll_detached","produced_qty":6170,"uom":"m",
        "qr_payload":QR,"label_item_code":"P","label_item_name":"Lazer guruch uzun don 2 kg",
        "executor_name":"Worker","worker_role":"aparatchi","worker_ref":"route-worker",
        "worker_display_name":"Worker","wip_status":"waiting","current_apparatus":LAM,
        "current_location":"lamination output","next_apparatus":CUT,
        "payload_json":{"stage_node_id":"lamination_1","next_stage_node_id":"rezka_5"}
    }))
    .unwrap();
    batch.refresh_status_detail();
    batch
}

fn claim_write(
    map: &ProductionMapDefinition,
    source: &OrderProgressBatch,
) -> QueueActionProgressWrite {
    let mut route = resolve_wip_input_route(map, &normalized_route_batch(source)).unwrap();
    route.stage_node_id = "apparatus_7".into();
    let mut claimed = normalized_route_batch(source);
    claimed.wip_status = OrderProgressBatchWipStatus::InUse;
    claimed.current_apparatus = CUT2.into();
    claimed.current_location = CUT2.into();
    claimed.used_by_apparatus = CUT2.into();
    claimed.used_by_session_id = "cut2-input-session".into();
    claimed.payload_json["wip_route_binding"] = serde_json::to_value(route).unwrap();
    claimed.payload_json["wip_in_use_at_unix"] = serde_json::json!(100);
    claimed.refresh_status_detail();
    QueueActionProgressWrite {
        apparatus: CUT2.into(),
        map_update: None,
        states: BTreeMap::from([(ORDER.into(), "in_progress".into())]),
        sequence_updates: BTreeMap::new(),
        event: ApparatusQueueActionEvent {
            event_id: "route-start-event".into(),
            apparatus: CUT2.into(),
            order_id: ORDER.into(),
            stage_node_id: "apparatus_7".into(),
            action: queue_state::ApparatusQueueAction::Start,
            from_state: queue_state::ApparatusQueueOrderState::Pending,
            to_state: queue_state::ApparatusQueueOrderState::InProgress,
            policy: ApparatusQueuePolicy::StrictSequence,
            actor: QueueActionActor {
                role: "aparatchi".into(),
                ref_: "route-worker".into(),
                display_name: "Worker".into(),
            },
            assigned_apparatus: vec![CUT2.into()],
            payload_json: serde_json::json!({
                "wip_input_map_fingerprint":production_map_fingerprint(map),
                "wip_input_expected_batch":source
            }),
        },
        session: Some(OrderRunSession {
            session_id: "cut2-input-session".into(),
            apparatus: CUT2.into(),
            order_id: ORDER.into(),
            stage_node_id: "apparatus_7".into(),
            status: OrderRunStatus::Active,
            worker_role: "aparatchi".into(),
            worker_ref: "route-worker".into(),
            worker_display_name: "Worker".into(),
            started_at_unix: 100,
            updated_at_unix: 100,
            payload_json: serde_json::json!({"input_progress_batch_id":source.batch_id,"input_progress_qr_payload":QR}),
        }),
        progress_event: None,
        progress_batch: None,
        progress_batches: vec![],
        progress_batch_updates: vec![claimed],
        opening_wip_batch_updates: vec![],
        raw_material_stock_transitions: vec![],
        qolip_checkouts: vec![],
        returned_paint_report: None,
        order_control_update: None,
        schedule_reservation_status: None,
        print_preflight_hold_id: None,
        print_preflight_cancel_hold_id: None,
    }
}

#[tokio::test]
async fn postgres_route_save_preserves_old_qr_and_rejects_destructive_edits() {
    let (pool, store, admin, name) = fixture().await;
    store.put_map(old_map()).await.unwrap();
    store
        .put_order_progress_batch(original_batch())
        .await
        .unwrap();
    let original = store
        .progress_batch("reported-route-roll")
        .await
        .unwrap()
        .unwrap();
    let replacement = alternative_map();
    store.put_map(replacement.clone()).await.unwrap();
    assert_eq!(
        store
            .progress_batch(&original.batch_id)
            .await
            .unwrap()
            .unwrap(),
        original,
        "map edits must not rewrite a physical roll or its printed QR"
    );
    let mut tx = pool.begin().await.unwrap();
    let inputs = inputs_tx(&mut tx, ORDER).await.unwrap();
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].target_node, "apparatus_6");
    tx.rollback().await.unwrap();
    for case in ["producer_deleted", "destination_unanchored", "ambiguous"] {
        let mut unsafe_map = replacement.clone();
        match case {
            "producer_deleted" => unsafe_map.nodes.retain(|n| n.id != "lamination_1"),
            "destination_unanchored" => {
                unsafe_map.nodes[2].apparatus_id = "apparatus:default:paket".into()
            }
            "ambiguous" => {
                let mut other = unsafe_map.nodes[2].clone();
                other.id = "different_operation".into();
                other.alternative_group_id.clear();
                other.apparatus_id = "apparatus:default:paket".into();
                unsafe_map.nodes.push(other);
                unsafe_map.edges.push(ProductionMapEdge {
                    from: "lamination_1".into(),
                    to: "different_operation".into(),
                    branch: String::new(),
                });
                unsafe_map.edges.push(ProductionMapEdge {
                    from: "different_operation".into(),
                    to: "end".into(),
                    branch: String::new(),
                });
            }
            _ => unreachable!(),
        }
        assert!(
            store.put_map(unsafe_map).await.is_err(),
            "{case} must fail closed"
        );
        assert_eq!(store.map_by_id(ORDER).await.unwrap().unwrap(), replacement);
    }
    let write = claim_write(&replacement, &original);
    store
        .put_apparatus_queue_states_with_event_and_progress(&write)
        .await
        .unwrap();
    let mut deleted_pin = replacement.clone();
    deleted_pin.nodes.retain(|n| n.id != "apparatus_7");
    deleted_pin
        .edges
        .retain(|e| e.from != "apparatus_7" && e.to != "apparatus_7");
    assert_eq!(
        store.put_map(deleted_pin).await,
        Err(ProductionMapError::WipRouteDestinationUnresolved)
    );
    cleanup(pool, store, admin, name).await;
}

#[tokio::test]
async fn postgres_route_save_preserves_the_actual_legacy_unpinned_consumer() {
    let (pool, store, admin, name) = fixture().await;
    let map = alternative_map();
    store.put_map(map.clone()).await.unwrap();
    let mut legacy = original_batch();
    legacy.payload_json["next_stage_node_id"] = serde_json::json!("apparatus_6");
    legacy.wip_status = OrderProgressBatchWipStatus::InUse;
    legacy.used_by_apparatus = CUT2.into();
    legacy.used_by_session_id = "legacy-cut-session".into();
    legacy.current_apparatus = CUT2.into();
    legacy.current_location = CUT2.into();
    legacy.refresh_status_detail();
    store.put_order_progress_batch(legacy).await.unwrap();
    let before = store
        .progress_batch("reported-route-roll")
        .await
        .unwrap()
        .unwrap();
    assert!(before.payload_json.get("wip_route_binding").is_none());
    let mut without_cut2 = map.clone();
    without_cut2.nodes.retain(|node| node.id != "apparatus_7");
    without_cut2
        .edges
        .retain(|edge| edge.from != "apparatus_7" && edge.to != "apparatus_7");
    assert_eq!(
        store.put_map(without_cut2.clone()).await,
        Err(ProductionMapError::WipRouteDestinationUnresolved),
        "retaining the original Cut1 target must not allow deleting active Cut2"
    );
    assert_eq!(store.map_by_id(ORDER).await.unwrap().unwrap(), map);
    assert_eq!(
        store
            .progress_batch(&before.batch_id)
            .await
            .unwrap()
            .unwrap(),
        before
    );
    let mut renamed_cut2 = map.clone();
    renamed_cut2
        .nodes
        .iter_mut()
        .find(|node| node.id == "apparatus_7")
        .unwrap()
        .id = "replacement_cut2".into();
    for edge in &mut renamed_cut2.edges {
        if edge.from == "apparatus_7" {
            edge.from = "replacement_cut2".into();
        }
        if edge.to == "apparatus_7" {
            edge.to = "replacement_cut2".into();
        }
    }
    assert_eq!(
        store.put_map(renamed_cut2).await,
        Err(ProductionMapError::WipRouteDestinationUnresolved),
        "the same canonical machine and alternative group cannot replace the active stage occurrence"
    );
    assert_eq!(store.map_by_id(ORDER).await.unwrap().unwrap(), map);
    assert_eq!(
        store
            .progress_batch(&before.batch_id)
            .await
            .unwrap()
            .unwrap(),
        before,
        "rejecting a canonical-equivalent replacement must preserve all stored roll facts"
    );

    let mut processing_on_cut1 = before.clone();
    processing_on_cut1.used_by_apparatus = CUT.into();
    processing_on_cut1.current_apparatus = CUT.into();
    processing_on_cut1.current_location = CUT.into();
    processing_on_cut1.refresh_status_detail();
    store
        .put_order_progress_batch(processing_on_cut1)
        .await
        .unwrap();
    let positive_before = store
        .progress_batch(&before.batch_id)
        .await
        .unwrap()
        .unwrap();
    store.put_map(without_cut2.clone()).await.unwrap();
    assert_eq!(
        store.map_by_id(ORDER).await.unwrap().unwrap(),
        without_cut2,
        "an unused alternative can be removed while the actual Cut1 occurrence remains"
    );
    assert_eq!(
        store
            .progress_batch(&before.batch_id)
            .await
            .unwrap()
            .unwrap(),
        positive_before,
        "neither accepted nor rejected map saves may rewrite legacy roll ownership or QR"
    );
    cleanup(pool, store, admin, name).await;
}

#[tokio::test]
async fn postgres_route_claim_revalidates_batch_and_binding_without_partial_writes() {
    let (pool, store, admin, name) = fixture().await;
    let map = alternative_map();
    // Seed the already-broken historic shape directly into the isolated fixture:
    // the current map has no rezka_5, while the waiting roll still does.
    store.put_map(map.clone()).await.unwrap();
    store
        .put_order_progress_batch(original_batch())
        .await
        .unwrap();
    let original = store
        .progress_batch("reported-route-roll")
        .await
        .unwrap()
        .unwrap();
    let write = claim_write(&map, &original);
    for case in [
        "missing_metadata",
        "wrong_qr",
        "wrong_binding",
        "wrong_receiver",
        "same_apparatus_metadata_bypass",
    ] {
        let mut invalid = write.clone();
        match case {
            "missing_metadata" => invalid.event.payload_json = serde_json::json!({}),
            "wrong_qr" => invalid.progress_batch_updates[0].qr_payload = "replacement-qr".into(),
            "wrong_binding" => {
                invalid.progress_batch_updates[0].payload_json["wip_route_binding"]["stage_node_id"] =
                    serde_json::json!("apparatus_6")
            }
            "wrong_receiver" => invalid.progress_batch_updates[0].used_by_apparatus = CUT.into(),
            "same_apparatus_metadata_bypass" => {
                invalid.apparatus = LAM.into();
                invalid.event.apparatus = LAM.into();
                invalid.event.assigned_apparatus = vec![LAM.into()];
                invalid.event.payload_json = serde_json::json!({});
                invalid.session.as_mut().unwrap().apparatus = LAM.into();
                invalid.progress_batch_updates[0].used_by_apparatus = LAM.into();
            }
            _ => unreachable!(),
        }
        assert_eq!(
            store
                .put_apparatus_queue_states_with_event_and_progress(&invalid)
                .await,
            Err(ProductionMapError::WipRouteChanged),
            "{case}"
        );
        assert_eq!(
            store
                .progress_batch(&original.batch_id)
                .await
                .unwrap()
                .unwrap(),
            original
        );
        assert!(
            store
                .order_run_sessions_for_order(ORDER)
                .await
                .unwrap()
                .is_empty()
        );
    }
    let mut self_consumed = original.clone();
    self_consumed.wip_status = OrderProgressBatchWipStatus::Processed;
    self_consumed.used_by_apparatus = LAM.into();
    self_consumed.used_by_session_id = original.session_id.clone();
    self_consumed.processed_by_apparatus = LAM.into();
    self_consumed.processed_by_session_id = original.session_id.clone();
    self_consumed.payload_json["wip_processed_at_unix"] = serde_json::json!(90);
    self_consumed.refresh_status_detail();
    store.put_order_progress_batch(self_consumed).await.unwrap();
    let recovered_source = store
        .progress_batch(&original.batch_id)
        .await
        .unwrap()
        .unwrap();
    let recovered_claim = claim_write(&map, &recovered_source);
    let mut recovery_tx = pool.begin().await.unwrap();
    super::super::transaction_locks::lock_order_tx(&mut recovery_tx, ORDER)
        .await
        .unwrap();
    validate_input_claims_tx(&mut recovery_tx, &recovered_claim)
        .await
        .unwrap();
    recovery_tx.rollback().await.unwrap();
    assert_eq!(
        store
            .progress_batch(&original.batch_id)
            .await
            .unwrap()
            .unwrap(),
        recovered_source,
        "legacy normalization validation must not rewrite the stored source"
    );
    store
        .put_order_progress_batch(original.clone())
        .await
        .unwrap();
    let correction: ProgressBatchCorrectionInput = serde_json::from_value(serde_json::json!({
        "batch_id":original.batch_id,"expected_revision":original.revision,
        "produced_qty":6000,"uom":"m","reason":"correct fixture meter reading"
    }))
    .unwrap();
    store
        .correct_progress_batch(original.clone(), correction, write.event.actor.clone())
        .await
        .unwrap();
    assert_eq!(
        store
            .put_apparatus_queue_states_with_event_and_progress(&write)
            .await,
        Err(ProductionMapError::WipRouteChanged),
        "stale batch must not overwrite correction"
    );
    let current = store
        .progress_batch(&original.batch_id)
        .await
        .unwrap()
        .unwrap();
    let fresh_write = claim_write(&map, &current);
    store
        .put_apparatus_queue_states_with_event_and_progress(&fresh_write)
        .await
        .unwrap();
    let claimed = store
        .progress_batch(&original.batch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed.qr_payload, QR);
    assert_eq!(claimed.produced_qty, 6000.0);
    assert_eq!(claimed.used_by_apparatus, CUT2);
    assert_eq!(claimed.payload_json["next_stage_node_id"], "rezka_5");
    assert_eq!(
        claimed.payload_json["wip_route_binding"]["stage_node_id"],
        "apparatus_7"
    );
    let mut renamed = map.clone();
    renamed.title.push_str(" (display edit)");
    store.put_map(renamed).await.unwrap();
    store
        .put_apparatus_queue_states_with_event_and_progress(&fresh_write)
        .await
        .unwrap();
    assert_eq!(
        store
            .progress_batch(&original.batch_id)
            .await
            .unwrap()
            .unwrap(),
        claimed,
        "the identical committed event stays idempotent across subsequent map edits"
    );
    // An established roll removed during handoff can resume at the same pinned
    // occurrence without adopting a route from the later display-only edit.
    let mut removed = claimed.clone();
    removed.wip_status = OrderProgressBatchWipStatus::Waiting;
    removed.used_by_apparatus.clear();
    removed.used_by_session_id.clear();
    removed.payload_json["roll_removed_from_apparatus"] = serde_json::json!(true);
    removed.refresh_status_detail();
    store.put_order_progress_batch(removed).await.unwrap();
    store
        .put_apparatus_queue_states(CUT2, BTreeMap::from([(ORDER.into(), "paused".into())]))
        .await
        .unwrap();
    let mut resume = fresh_write.clone();
    resume.event.event_id = "route-resume-event".into();
    resume.event.action = queue_state::ApparatusQueueAction::Resume;
    resume.event.from_state = queue_state::ApparatusQueueOrderState::Paused;
    resume.event.payload_json = serde_json::json!({});
    resume.progress_batch_updates[0] = claimed.clone();
    resume.progress_batch_updates[0].payload_json["roll_removed_from_apparatus"] =
        serde_json::json!(false);
    let mut replaced_pin = resume.clone();
    replaced_pin.progress_batch_updates[0].payload_json["wip_route_binding"]["stage_node_id"] =
        serde_json::json!("apparatus_6");
    assert_eq!(
        store
            .put_apparatus_queue_states_with_event_and_progress(&replaced_pin)
            .await,
        Err(ProductionMapError::WipRouteChanged),
        "Resume must not choose a new occurrence"
    );
    store
        .put_apparatus_queue_states_with_event_and_progress(&resume)
        .await
        .unwrap();
    let resumed = store
        .progress_batch(&original.batch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resumed.qr_payload, QR);
    assert_eq!(resumed.used_by_session_id, claimed.used_by_session_id);
    assert_eq!(
        resumed.payload_json["wip_route_binding"],
        claimed.payload_json["wip_route_binding"]
    );
    cleanup(pool, store, admin, name).await;
}

#[tokio::test]
async fn postgres_route_start_waits_for_map_lock_then_rejects_stale_preview() {
    let (pool, store, admin, name) = fixture().await;
    let map = alternative_map();
    store.put_map(map.clone()).await.unwrap();
    store
        .put_order_progress_batch(original_batch())
        .await
        .unwrap();
    let original = store
        .progress_batch("reported-route-roll")
        .await
        .unwrap()
        .unwrap();
    let write = claim_write(&map, &original);
    let mut edit_tx = pool.begin().await.unwrap();
    super::super::transaction_locks::lock_order_tx(&mut edit_tx, ORDER)
        .await
        .unwrap();
    let mut changed = map.clone();
    changed.title.push_str(" concurrently edited");
    super::super::map_helpers::put_map_inner_tx(&mut edit_tx, &changed)
        .await
        .unwrap();
    let claimant = store.clone();
    let mut pending = tokio::spawn(async move {
        claimant
            .put_apparatus_queue_states_with_event_and_progress(&write)
            .await
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(80), &mut pending)
            .await
            .is_err(),
        "Start must serialize behind the map edit's order lock"
    );
    edit_tx.commit().await.unwrap();
    assert_eq!(
        pending.await.unwrap(),
        Err(ProductionMapError::WipRouteChanged)
    );
    assert_eq!(
        store
            .progress_batch(&original.batch_id)
            .await
            .unwrap()
            .unwrap(),
        original
    );
    assert!(
        store
            .order_run_sessions_for_order(ORDER)
            .await
            .unwrap()
            .is_empty()
    );
    let events: i64 =
        sqlx::query_scalar("SELECT count(*) FROM mini_queue_action_events WHERE order_id=$1")
            .bind(ORDER)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        events, 0,
        "no queue event may survive the rejected stale claim"
    );
    cleanup(pool, store, admin, name).await;
}
