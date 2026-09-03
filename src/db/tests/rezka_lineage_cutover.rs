use std::sync::Arc;

use crate::core::production_map::{
    OrderProgressBatch, OrderProgressBatchStatus, OrderProgressBatchStatusDetail,
    OrderProgressBatchWipStatus, OrderRunInputLink, OrderRunInputSourceKind, OrderRunInputStatus,
    OrderRunSession, OrderRunStatus, ProductionMapDefinition, ProductionMapEdge, ProductionMapNode,
    ProductionMapNodeKind, ProductionMapStorePort, ProgressBatchInputLink, RezkaActivePartialRoll,
    RezkaPartialRollStatus, order_run_input_links_from_payload,
    progress_batch_input_links_from_payload, queue_state, rezka_active_partial_rolls_from_payload,
    rezka_merge_state_is_consistent,
};
use crate::db::postgres::{
    apply_foundation_migration, apply_postgres_migrations_through, postgres_test_database_options,
};
use crate::db::postgres_production_map::PostgresProductionMapStore;

use super::seed_standard_canonical_apparatus;

const APPARATUS: &str = "apparatus:default:asset-010";
const APPARATUS_2: &str = "apparatus:default:asset-007";
const APPARATUS_3: &str = "apparatus:default:bosma_7";
const APPARATUS_4: &str = "apparatus:default:asset-008";
const APPARATUS_5: &str = "apparatus:default:bosma_8";
const APPARATUS_6: &str = "apparatus:default:bosma_9";
const APPARATUS_7: &str = "apparatus:default:paket";
const APPARATUS_8: &str = "apparatus:default:flexo_pechat";
const ORDER: &str = "order-cutover-1";

// Fixed seed instants (UTC) and their epoch seconds.
const T_START: &str = "2026-03-01 08:00:00+00";
const T_MERGE: &str = "2026-03-01 09:30:00+00";
const T_SCALAR: &str = "2026-03-02 10:15:00+00";
const EPOCH_START: i64 = 1772352000;
const EPOCH_MERGE: i64 = 1772357400;
const EPOCH_SCALAR: i64 = 1772446500;

#[tokio::test]
async fn rezka_lineage_payload_cutover_backfills_and_drops_mirrors() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://superuser@127.0.0.1:5432/postgres".to_string());
    let db_name = format!(
        "mini_rs_erp_test_rezka_lineage_cutover_{}",
        std::process::id()
    );
    let admin_pool = sqlx::PgPool::connect(&admin_url).await.expect("admin db");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop test db");
    sqlx::query(&format!(r#"CREATE DATABASE "{db_name}""#))
        .execute(&admin_pool)
        .await
        .expect("create test db");
    admin_pool.close().await;

    let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&admin_url, &db_name))
        .await
        .expect("test db");

    // Pre-cutover schema: migrations 1..=91, typed lineage mirrors exist.
    apply_postgres_migrations_through(&pool, 91)
        .await
        .expect("apply migrations up to 0091");
    seed_standard_canonical_apparatus(&pool).await;

    seed_legacy_lineage(&pool).await;

    // Apply the cutover (0092) plus the rest of the registry.
    apply_foundation_migration(&pool)
        .await
        .expect("apply cutover migration");

    // A. Typed session lineage -> payload input_lineage (order + content exact).
    let lineage: serde_json::Value = sqlx::query_scalar(
        "SELECT payload_json->'input_lineage'
         FROM mini_order_run_sessions WHERE session_id = 'run-legacy-typed'",
    )
    .fetch_one(&pool)
    .await
    .expect("migrated session lineage");
    assert_eq!(
        lineage,
        serde_json::json!([
            {
                "input_batch_id": "wip-parent-0",
                "input_qr_payload": "qr:wip-parent-0",
                "source_apparatus": APPARATUS,
                "source_kind": "progress_batch",
                "stage_node_id": "rezka-stage",
                "sequence_no": 1,
                "status": "processed",
                "linked_at_unix": EPOCH_START,
                "processed_at_unix": EPOCH_MERGE,
            },
            {
                "input_batch_id": "wip-parent-1",
                "input_qr_payload": "qr:wip-parent-1",
                "source_apparatus": APPARATUS,
                "source_kind": "opening_wip",
                "stage_node_id": "rezka-stage",
                "sequence_no": 2,
                "status": "in_use",
                "linked_at_unix": EPOCH_MERGE,
            },
        ])
    );
    let parsed = order_run_input_links_from_payload(
        &sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT payload_json FROM mini_order_run_sessions WHERE session_id = 'run-legacy-typed'",
        )
        .fetch_one(&pool)
        .await
        .expect("migrated session payload"),
    )
    .expect("migrated lineage parses with the current parser");
    assert_eq!(
        parsed,
        vec![
            OrderRunInputLink {
                input_batch_id: "wip-parent-0".to_string(),
                input_qr_payload: "qr:wip-parent-0".to_string(),
                source_apparatus: APPARATUS.to_string(),
                source_kind: OrderRunInputSourceKind::ProgressBatch,
                stage_node_id: "rezka-stage".to_string(),
                sequence_no: 1,
                status: OrderRunInputStatus::Processed,
                linked_at_unix: EPOCH_START,
                processed_at_unix: Some(EPOCH_MERGE),
            },
            OrderRunInputLink {
                input_batch_id: "wip-parent-1".to_string(),
                input_qr_payload: "qr:wip-parent-1".to_string(),
                source_apparatus: APPARATUS.to_string(),
                source_kind: OrderRunInputSourceKind::OpeningWip,
                stage_node_id: "rezka-stage".to_string(),
                sequence_no: 2,
                status: OrderRunInputStatus::InUse,
                linked_at_unix: EPOCH_MERGE,
                processed_at_unix: None,
            },
        ]
    );

    // B. Typed active rolls -> payload rezka_active_partial_rolls.
    let rolls: serde_json::Value = sqlx::query_scalar(
        "SELECT payload_json->'rezka_active_partial_rolls'
         FROM mini_order_run_sessions WHERE session_id = 'run-legacy-typed'",
    )
    .fetch_one(&pool)
    .await
    .expect("migrated active rolls");
    assert_eq!(
        rolls,
        serde_json::json!([
            {
                "slot_index": 1,
                "generation": 2,
                "contained_kadr_count": 4,
                "status": "active",
                "source_input_batch_ids": ["wip-parent-0", "wip-parent-1"],
                "started_at_unix": EPOCH_START,
                "updated_at_unix": EPOCH_MERGE,
            },
        ])
    );
    let parsed_rolls = rezka_active_partial_rolls_from_payload(
        &sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT payload_json FROM mini_order_run_sessions WHERE session_id = 'run-legacy-typed'",
        )
        .fetch_one(&pool)
        .await
        .expect("migrated session payload"),
    )
    .expect("migrated rolls parse with the current parser");
    assert_eq!(
        parsed_rolls,
        vec![RezkaActivePartialRoll {
            slot_index: 1,
            generation: 2,
            contained_kadr_count: 4,
            status: RezkaPartialRollStatus::Active,
            source_input_batch_ids: vec!["wip-parent-0".to_string(), "wip-parent-1".to_string(),],
            started_at_unix: EPOCH_START,
            updated_at_unix: EPOCH_MERGE,
        }]
    );
    assert!(rezka_merge_state_is_consistent(&parsed, &parsed_rolls));

    // Legacy scalar-only session -> canonical single link (old fallback semantics:
    // completed sessions link as processed at updated_at).
    let scalar_lineage: serde_json::Value = sqlx::query_scalar(
        "SELECT payload_json->'input_lineage'
         FROM mini_order_run_sessions WHERE session_id = 'run-legacy-scalar'",
    )
    .fetch_one(&pool)
    .await
    .expect("migrated scalar session lineage");
    assert_eq!(
        scalar_lineage,
        serde_json::json!([
            {
                "input_batch_id": "wip-parent-1",
                "input_qr_payload": "qr:wip-parent-1",
                "source_apparatus": APPARATUS,
                "source_kind": "progress_batch",
                "stage_node_id": "stage-scalar",
                "sequence_no": 1,
                "status": "processed",
                "linked_at_unix": EPOCH_SCALAR,
                "processed_at_unix": EPOCH_SCALAR,
            },
        ])
    );

    // C. Typed output links -> payload source_input_links.
    let output_links: serde_json::Value = sqlx::query_scalar(
        "SELECT payload_json->'source_input_links'
         FROM mini_progress_batches WHERE batch_id = 'rezka-output-typed'",
    )
    .fetch_one(&pool)
    .await
    .expect("migrated output lineage");
    assert_eq!(
        output_links,
        serde_json::json!([
            {
                "input_batch_id": "wip-parent-0",
                "input_qr_payload": "qr:wip-parent-0",
                "source_apparatus": APPARATUS,
                "source_kind": "progress_batch",
                "sequence_no": 1,
            },
            {
                "input_batch_id": "wip-parent-1",
                "input_qr_payload": "qr:wip-parent-1",
                "source_apparatus": APPARATUS,
                "source_kind": "progress_batch",
                "sequence_no": 2,
            },
        ])
    );
    let parsed_output = progress_batch_input_links_from_payload(
        &sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT payload_json FROM mini_progress_batches WHERE batch_id = 'rezka-output-typed'",
        )
        .fetch_one(&pool)
        .await
        .expect("migrated output payload"),
    )
    .expect("migrated output lineage parses with the current parser");
    assert_eq!(
        parsed_output,
        vec![
            ProgressBatchInputLink {
                input_batch_id: "wip-parent-0".to_string(),
                input_qr_payload: "qr:wip-parent-0".to_string(),
                source_apparatus: APPARATUS.to_string(),
                source_kind: OrderRunInputSourceKind::ProgressBatch,
                sequence_no: 1,
            },
            ProgressBatchInputLink {
                input_batch_id: "wip-parent-1".to_string(),
                input_qr_payload: "qr:wip-parent-1".to_string(),
                source_apparatus: APPARATUS.to_string(),
                source_kind: OrderRunInputSourceKind::ProgressBatch,
                sequence_no: 2,
            },
        ]
    );

    // D. Legacy parent_batch_id column -> canonical single output link.
    let scalar_output: serde_json::Value = sqlx::query_scalar(
        "SELECT payload_json->'source_input_links'
         FROM mini_progress_batches WHERE batch_id = 'rezka-output-scalar'",
    )
    .fetch_one(&pool)
    .await
    .expect("migrated scalar output lineage");
    assert_eq!(
        scalar_output,
        serde_json::json!([
            {
                "input_batch_id": "wip-parent-1",
                "input_qr_payload": "",
                "source_apparatus": "",
                "source_kind": "progress_batch",
                "sequence_no": 1,
            },
        ])
    );

    // E. Existing canonical payload wins over conflicting typed mirrors.
    let kept_lineage: serde_json::Value = sqlx::query_scalar(
        "SELECT payload_json->'input_lineage'
         FROM mini_order_run_sessions WHERE session_id = 'run-canonical-wins'",
    )
    .fetch_one(&pool)
    .await
    .expect("kept session lineage");
    assert_eq!(
        kept_lineage,
        serde_json::json!([
            {
                "input_batch_id": "wip-canonical",
                "input_qr_payload": "qr:wip-canonical",
                "source_apparatus": APPARATUS_2,
                "source_kind": "progress_batch",
                "stage_node_id": "rezka-stage",
                "sequence_no": 1,
                "status": "in_use",
                "linked_at_unix": EPOCH_START,
            },
        ])
    );
    let kept_output: serde_json::Value = sqlx::query_scalar(
        "SELECT payload_json->'source_input_links'
         FROM mini_progress_batches WHERE batch_id = 'batch-canonical-wins'",
    )
    .fetch_one(&pool)
    .await
    .expect("kept output lineage");
    assert_eq!(
        kept_output,
        serde_json::json!([
            {
                "input_batch_id": "wip-canonical",
                "input_qr_payload": "qr:wip-canonical",
                "source_apparatus": APPARATUS,
                "source_kind": "progress_batch",
                "sequence_no": 1,
            },
        ])
    );

    // Adversarial: malformed canonical payload is replaced from valid mirrors.
    // A. `[{}]` input_lineage is recovered from the typed mirror.
    let recovered_lineage: serde_json::Value = sqlx::query_scalar(
        "SELECT payload_json->'input_lineage'
         FROM mini_order_run_sessions WHERE session_id = 'run-malformed-lineage'",
    )
    .fetch_one(&pool)
    .await
    .expect("recovered session lineage");
    assert_eq!(
        recovered_lineage,
        serde_json::json!([
            {
                "input_batch_id": "wip-parent-0",
                "input_qr_payload": "qr:wip-parent-0",
                "source_apparatus": "apparatus:default:asset-010",
                "source_kind": "progress_batch",
                "stage_node_id": "rezka-stage",
                "sequence_no": 1,
                "status": "in_use",
                "linked_at_unix": EPOCH_START,
            },
        ])
    );
    order_run_input_links_from_payload(
        &sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT payload_json FROM mini_order_run_sessions
             WHERE session_id = 'run-malformed-lineage'",
        )
        .fetch_one(&pool)
        .await
        .expect("recovered session payload"),
    )
    .expect("recovered lineage parses with the current parser");

    // B. Blank-id output lineage is recovered from the typed mirror.
    let recovered_output: serde_json::Value = sqlx::query_scalar(
        "SELECT payload_json->'source_input_links'
         FROM mini_progress_batches WHERE batch_id = 'batch-malformed-links'",
    )
    .fetch_one(&pool)
    .await
    .expect("recovered output lineage");
    assert_eq!(
        recovered_output,
        serde_json::json!([
            {
                "input_batch_id": "wip-parent-1",
                "input_qr_payload": "qr:wip-parent-1",
                "source_apparatus": "apparatus:default:asset-010",
                "source_kind": "progress_batch",
                "sequence_no": 1,
            },
        ])
    );

    // C. Zero-slot rolls are recovered; the valid lineage is preserved.
    let recovered_rolls: serde_json::Value = sqlx::query_scalar(
        "SELECT payload_json->'rezka_active_partial_rolls'
         FROM mini_order_run_sessions WHERE session_id = 'run-malformed-rolls'",
    )
    .fetch_one(&pool)
    .await
    .expect("recovered rolls");
    assert_eq!(
        recovered_rolls,
        serde_json::json!([
            {
                "slot_index": 1,
                "generation": 1,
                "contained_kadr_count": 1,
                "status": "active",
                "source_input_batch_ids": ["wip-parent-0"],
                "started_at_unix": EPOCH_START,
                "updated_at_unix": EPOCH_START,
            },
        ])
    );
    let rolls_payload: serde_json::Value = sqlx::query_scalar(
        "SELECT payload_json FROM mini_order_run_sessions
         WHERE session_id = 'run-malformed-rolls'",
    )
    .fetch_one(&pool)
    .await
    .expect("recovered rolls payload");
    let rolls_links =
        order_run_input_links_from_payload(&rolls_payload).expect("kept lineage still parses");
    let rolls_parsed =
        rezka_active_partial_rolls_from_payload(&rolls_payload).expect("recovered rolls parse");
    assert_eq!(rolls_links.len(), 1);
    assert!(rezka_merge_state_is_consistent(&rolls_links, &rolls_parsed));

    // F/G. Duplicate sequence numbers and invalid enums are not canonical:
    // both are replaced from their valid mirrors.
    let deduped: serde_json::Value = sqlx::query_scalar(
        "SELECT payload_json->'input_lineage'
         FROM mini_order_run_sessions WHERE session_id = 'run-dup-seq'",
    )
    .fetch_one(&pool)
    .await
    .expect("deduplicated lineage");
    assert_eq!(
        deduped,
        serde_json::json!([
            {
                "input_batch_id": "wip-parent-0",
                "input_qr_payload": "qr:wip-parent-0",
                "source_apparatus": "apparatus:default:asset-010",
                "source_kind": "progress_batch",
                "stage_node_id": "rezka-stage",
                "sequence_no": 1,
                "status": "in_use",
                "linked_at_unix": EPOCH_START,
            },
        ])
    );
    let rekindled: serde_json::Value = sqlx::query_scalar(
        "SELECT payload_json->'source_input_links'
         FROM mini_progress_batches WHERE batch_id = 'batch-bad-kind'",
    )
    .fetch_one(&pool)
    .await
    .expect("rekindled output lineage");
    assert_eq!(
        rekindled,
        serde_json::json!([
            {
                "input_batch_id": "wip-parent-0",
                "input_qr_payload": "qr:wip-parent-0",
                "source_apparatus": "apparatus:default:asset-010",
                "source_kind": "progress_batch",
                "sequence_no": 1,
            },
        ])
    );

    // Explicit `[]` is valid canonical lineage: stale mirrors must not
    // overwrite it, matching the Rust parser and the completion flow.
    let kept_empty: serde_json::Value = sqlx::query_scalar(
        "SELECT payload_json FROM mini_order_run_sessions
         WHERE session_id = 'run-empty-kept'",
    )
    .fetch_one(&pool)
    .await
    .expect("kept empty payload");
    assert_eq!(kept_empty["input_lineage"], serde_json::json!([]));
    assert_eq!(
        kept_empty["rezka_active_partial_rolls"],
        serde_json::json!([])
    );
    assert_eq!(
        order_run_input_links_from_payload(&kept_empty).expect("empty lineage parses"),
        Vec::<OrderRunInputLink>::new()
    );
    assert_eq!(
        rezka_active_partial_rolls_from_payload(&kept_empty).expect("empty rolls parse"),
        Vec::<RezkaActivePartialRoll>::new()
    );
    let kept_empty_output: serde_json::Value = sqlx::query_scalar(
        "SELECT payload_json->'source_input_links'
         FROM mini_progress_batches WHERE batch_id = 'batch-empty-kept'",
    )
    .fetch_one(&pool)
    .await
    .expect("kept empty output lineage");
    assert_eq!(kept_empty_output, serde_json::json!([]));

    // A JSON number input_batch_id is rejected and recovered from the mirror.
    let renumbered: serde_json::Value = sqlx::query_scalar(
        "SELECT payload_json->'input_lineage'
         FROM mini_order_run_sessions WHERE session_id = 'run-numeric-id'",
    )
    .fetch_one(&pool)
    .await
    .expect("renumbered lineage");
    assert_eq!(
        renumbered,
        serde_json::json!([
            {
                "input_batch_id": "wip-parent-1",
                "input_qr_payload": "qr:wip-parent-1",
                "source_apparatus": "apparatus:default:asset-010",
                "source_kind": "progress_batch",
                "stage_node_id": "rezka-stage",
                "sequence_no": 1,
                "status": "in_use",
                "linked_at_unix": EPOCH_START,
            },
        ])
    );

    // F. Mirror tables are physically gone.
    for table in [
        "mini_order_run_input_links",
        "mini_rezka_active_partial_rolls",
        "mini_progress_batch_input_links",
    ] {
        let exists: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(table)
            .fetch_one(&pool)
            .await
            .expect("mirror table existence probe");
        assert!(!exists, "{table} must be dropped by the cutover");
    }

    // G. Normal live flow after the cutover: real store paths, no mirrors.
    // (Canonical apparatus and the ORDER map were seeded before the cutover.)
    let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));

    let live_links = vec![
        OrderRunInputLink {
            input_batch_id: "wip-live-0".to_string(),
            input_qr_payload: "qr:wip-live-0".to_string(),
            source_apparatus: APPARATUS_3.to_string(),
            source_kind: OrderRunInputSourceKind::ProgressBatch,
            stage_node_id: "rezka-stage".to_string(),
            sequence_no: 1,
            status: OrderRunInputStatus::Processed,
            linked_at_unix: EPOCH_START,
            processed_at_unix: Some(EPOCH_MERGE),
        },
        OrderRunInputLink {
            input_batch_id: "wip-live-1".to_string(),
            input_qr_payload: "qr:wip-live-1".to_string(),
            source_apparatus: APPARATUS_3.to_string(),
            source_kind: OrderRunInputSourceKind::ProgressBatch,
            stage_node_id: "rezka-stage".to_string(),
            sequence_no: 2,
            status: OrderRunInputStatus::InUse,
            linked_at_unix: EPOCH_MERGE,
            processed_at_unix: None,
        },
    ];
    let live_rolls = vec![RezkaActivePartialRoll {
        slot_index: 1,
        generation: 1,
        contained_kadr_count: 2,
        status: RezkaPartialRollStatus::Active,
        source_input_batch_ids: vec!["wip-live-0".to_string(), "wip-live-1".to_string()],
        started_at_unix: EPOCH_START,
        updated_at_unix: EPOCH_MERGE,
    }];
    let mut live_payload = serde_json::json!({});
    crate::core::production_map::write_order_run_input_links(&mut live_payload, &live_links);
    crate::core::production_map::write_rezka_active_partial_rolls(&mut live_payload, &live_rolls);
    store
        .put_order_run_session(OrderRunSession {
            session_id: "run-live-1".to_string(),
            apparatus: APPARATUS_3.to_string(),
            order_id: ORDER.to_string(),
            stage_node_id: "rezka-stage".to_string(),
            status: OrderRunStatus::Active,
            worker_role: "aparatchi".to_string(),
            worker_ref: "worker-1".to_string(),
            worker_display_name: "Worker".to_string(),
            started_at_unix: EPOCH_START,
            updated_at_unix: EPOCH_MERGE,
            payload_json: live_payload,
        })
        .await
        .expect("live session write without mirrors");
    let live_sessions = store
        .order_run_sessions_for_order(ORDER)
        .await
        .expect("live session read");
    let live_session = live_sessions
        .iter()
        .find(|session| session.session_id == "run-live-1")
        .expect("live session present");
    assert_eq!(
        order_run_input_links_from_payload(&live_session.payload_json)
            .expect("live lineage parses"),
        live_links
    );
    assert_eq!(
        rezka_active_partial_rolls_from_payload(&live_session.payload_json)
            .expect("live rolls parse"),
        live_rolls
    );

    let live_output = vec![ProgressBatchInputLink {
        input_batch_id: "wip-live-1".to_string(),
        input_qr_payload: "qr:wip-live-1".to_string(),
        source_apparatus: APPARATUS_3.to_string(),
        source_kind: OrderRunInputSourceKind::ProgressBatch,
        sequence_no: 1,
    }];
    let mut live_batch_payload = serde_json::json!({});
    crate::core::production_map::write_progress_batch_input_links(
        &mut live_batch_payload,
        &live_output,
    );
    store
        .put_order_progress_batch(OrderProgressBatch {
            batch_id: "rezka-output-live".to_string(),
            revision: 1,
            session_id: "run-live-1".to_string(),
            started_at_unix: EPOCH_START,
            completed_at_unix: EPOCH_MERGE,
            apparatus: APPARATUS_3.to_string(),
            order_id: ORDER.to_string(),
            action: queue_state::ApparatusQueueAction::Pause,
            status: OrderProgressBatchStatus::Paused,
            produced_qty: 10.0,
            uom: "kg".to_string(),
            qr_payload: "qr:rezka-output-live".to_string(),
            label_item_code: ORDER.to_string(),
            label_item_name: "Cutover live output".to_string(),
            executor_name: "Worker".to_string(),
            worker_role: "aparatchi".to_string(),
            worker_ref: "worker-1".to_string(),
            worker_display_name: "Worker".to_string(),
            wip_status: OrderProgressBatchWipStatus::Waiting,
            status_detail: OrderProgressBatchStatusDetail::default(),
            current_apparatus: APPARATUS_3.to_string(),
            current_location: "cell-1".to_string(),
            next_apparatus: String::new(),
            parent_batch_id: "wip-live-1".to_string(),
            used_by_session_id: String::new(),
            used_by_apparatus: String::new(),
            processed_by_session_id: String::new(),
            processed_by_apparatus: String::new(),
            return_ink_kg: None,
            lamination_print_leftover_rolls: None,
            lamination_film_leftover_rolls: None,
            rezka_bosma_waste: None,
            rezka_lamination_waste: None,
            rezka_edge_waste: None,
            total_waste: None,
            finished_goods_kg: None,
            bobina_kg: None,
            finished_goods_meter: None,
            diameter: None,
            description: String::new(),
            payload_json: live_batch_payload,
        })
        .await
        .expect("live batch write without mirrors");
    let live_batch = store
        .progress_batch("rezka-output-live")
        .await
        .expect("live batch read")
        .expect("live batch present");
    assert_eq!(
        progress_batch_input_links_from_payload(&live_batch.payload_json)
            .expect("live output lineage parses"),
        live_output
    );

    // Completion-shaped lineage edit still validates through the same parsers:
    // the in-use input becomes processed and mounted rolls clear.
    let mut resumed_links = live_links.clone();
    if let Some(link) = resumed_links
        .iter_mut()
        .find(|link| link.status == OrderRunInputStatus::InUse)
    {
        link.status = OrderRunInputStatus::Processed;
        link.processed_at_unix = Some(EPOCH_MERGE);
    }
    assert!(rezka_merge_state_is_consistent(&resumed_links, &[]));

    // Cleanup test database.
    pool.close().await;
    let admin_pool = sqlx::PgPool::connect(&admin_url)
        .await
        .expect("admin cleanup");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop test db");
}

fn cutover_live_map() -> ProductionMapDefinition {
    fn node(
        id: &str,
        kind: ProductionMapNodeKind,
        apparatus_id: &str,
        y: f64,
    ) -> ProductionMapNode {
        ProductionMapNode {
            id: id.to_string(),
            kind,
            title: id.to_string(),
            apparatus_id: apparatus_id.to_string(),
            formula: None,
            role_code: String::new(),
            item_code: String::new(),
            qty_formula: String::new(),
            from_location: String::new(),
            to_location: String::new(),
            alternative_group_id: String::new(),
            alternative_group_label: String::new(),
            alternative_assigned_title: String::new(),
            alternative_assigned_apparatus_id: String::new(),
            rezka_kadr_count: None,
            rezka_frame_groups: Vec::new(),
            rezka_label_length: None,
            x: 0.0,
            y,
        }
    }

    ProductionMapDefinition {
        id: ORDER.to_string(),
        product_code: "CUTOVER".to_string(),
        title: "Cutover live map".to_string(),
        code: "CUTOVER-1".to_string(),
        order_number: "CUTOVER-1".to_string(),
        customer_name: String::new(),
        image_id: String::new(),
        roll_count: None,
        width_mm: None,
        order_kg: None,
        base_length: None,
        nodes: vec![
            node("start", ProductionMapNodeKind::Start, "", 0.0),
            node(
                "rezka",
                ProductionMapNodeKind::Apparatus,
                APPARATUS_3,
                120.0,
            ),
            node("end", ProductionMapNodeKind::End, "", 240.0),
        ],
        edges: vec![
            ProductionMapEdge {
                from: "start".to_string(),
                to: "rezka".to_string(),
                branch: String::new(),
            },
            ProductionMapEdge {
                from: "rezka".to_string(),
                to: "end".to_string(),
                branch: String::new(),
            },
        ],
    }
}

/// Pre-cutover legacy rows: typed mirrors plus scalar-only shapes, no canonical
/// payload lineage. Mirrors the shapes the old dual-write runtime produced.
async fn seed_legacy_lineage(pool: &sqlx::PgPool) {
    // The live-flow order needs a deserializable map document: lifecycle
    // refresh fails closed on invalid map_json.
    let live_map = cutover_live_map();
    sqlx::query(
        "INSERT INTO mini_production_maps (id, product_code, title, map_json)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(ORDER)
    .bind(&live_map.product_code)
    .bind(&live_map.title)
    .bind(serde_json::to_value(&live_map).expect("valid live map json"))
    .execute(pool)
    .await
    .expect("seed legacy order map");

    // Parent WIP batches owned by mini_progress_batches (unambiguous owners).
    for (batch_id, qr) in [
        ("wip-parent-0", "qr:wip-parent-0"),
        ("wip-parent-1", "qr:wip-parent-1"),
    ] {
        sqlx::query(
            "INSERT INTO mini_progress_batches (
                 batch_id, session_id, apparatus, canonical_apparatus_id,
                 order_id, action, status,
                 produced_qty, uom, qr_payload, label_item_code, label_item_name,
                 worker_role, worker_ref, worker_display_name, wip_status, payload_json
             ) VALUES (
                 $1, 'seed-session', $2, $2, $3, 'pause', 'paused',
                 5, 'kg', $4, 'CUTOVER', 'Cutover parent',
                 'aparatchi', 'worker-1', 'Worker', 'processed', '{}'::jsonb
             )",
        )
        .bind(batch_id)
        .bind(APPARATUS)
        .bind(ORDER)
        .bind(qr)
        .execute(pool)
        .await
        .expect("seed parent progress batch");
    }

    // A. Typed session lineage (two links => splice order matters).
    sqlx::query(
        "INSERT INTO mini_order_run_sessions (
             session_id, apparatus, canonical_apparatus_id, order_id, status, stage_node_id,
             worker_role, worker_ref, worker_display_name,
             started_at, updated_at, payload_json
         ) VALUES (
             'run-legacy-typed', $1, $1, $2, 'active', 'rezka-stage',
             'aparatchi', 'worker-1', 'Worker',
             $3::timestamptz, $4::timestamptz,
             '{\"input_progress_batch_id\": \"wip-parent-1\"}'::jsonb
         )",
    )
    .bind(APPARATUS)
    .bind(ORDER)
    .bind(T_START)
    .bind(T_MERGE)
    .execute(pool)
    .await
    .expect("seed typed session");
    sqlx::query(
        "INSERT INTO mini_order_run_input_links (
             session_id, order_id, target_apparatus,
             input_batch_id, input_qr_payload, source_apparatus, source_kind,
             stage_node_id, sequence_no, status, linked_at, processed_at
         ) VALUES
             ('run-legacy-typed', $1, $2,
              'wip-parent-0', 'qr:wip-parent-0', $2, 'progress_batch',
              'rezka-stage', 1, 'processed', $3::timestamptz, $4::timestamptz),
             ('run-legacy-typed', $1, $2,
              'wip-parent-1', 'qr:wip-parent-1', $2, 'opening_wip',
              'rezka-stage', 2, 'in_use', $4::timestamptz, NULL)",
    )
    .bind(ORDER)
    .bind(APPARATUS)
    .bind(T_START)
    .bind(T_MERGE)
    .execute(pool)
    .await
    .expect("seed typed session links");

    // B. Typed active partial rolls, canonical payload field missing.
    sqlx::query(
        "INSERT INTO mini_rezka_active_partial_rolls (
             session_id, order_id, apparatus, slot_index, generation,
             contained_kadr_count, status, source_input_batch_ids,
             started_at, updated_at
         ) VALUES (
             'run-legacy-typed', $1, $2, 1, 2,
             4, 'active', ARRAY['wip-parent-0', 'wip-parent-1'],
             $3::timestamptz, $4::timestamptz
         )",
    )
    .bind(ORDER)
    .bind(APPARATUS)
    .bind(T_START)
    .bind(T_MERGE)
    .execute(pool)
    .await
    .expect("seed typed active rolls");

    // Legacy scalar-only session: no typed rows, unambiguous progress parent.
    // Completed status exercises the processed-at branch of the old fallback.
    sqlx::query(
        "INSERT INTO mini_order_run_sessions (
             session_id, apparatus, canonical_apparatus_id, order_id, status, stage_node_id,
             worker_role, worker_ref, worker_display_name,
             started_at, updated_at, payload_json
         ) VALUES (
             'run-legacy-scalar', $1, $1, $2, 'completed', 'stage-scalar',
             'aparatchi', 'worker-1', 'Worker',
             $3::timestamptz, $3::timestamptz,
             '{\"input_progress_batch_id\": \"wip-parent-1\",
                \"input_progress_qr_payload\": \"qr:wip-parent-1\",
                \"input_progress_apparatus\": \"apparatus:default:asset-010\"}'::jsonb
         )",
    )
    .bind(APPARATUS)
    .bind(ORDER)
    .bind(T_SCALAR)
    .execute(pool)
    .await
    .expect("seed scalar session");

    // C. Typed output lineage, canonical payload field missing.
    sqlx::query(
        "INSERT INTO mini_progress_batches (
             batch_id, session_id, apparatus, canonical_apparatus_id, order_id,
             action, status,
             produced_qty, uom, qr_payload, label_item_code, label_item_name,
             worker_role, worker_ref, worker_display_name, wip_status, payload_json
         ) VALUES (
             'rezka-output-typed', 'run-legacy-typed', $1, $1, $2, 'complete', 'completed',
             10, 'kg', 'qr:rezka-output-typed', 'CUTOVER', 'Cutover output',
             'aparatchi', 'worker-1', 'Worker', 'waiting', '{}'::jsonb
         )",
    )
    .bind(APPARATUS)
    .bind(ORDER)
    .execute(pool)
    .await
    .expect("seed typed output batch");
    sqlx::query(
        "INSERT INTO mini_progress_batch_input_links (
             output_batch_id, session_id, order_id,
             input_batch_id, input_qr_payload, source_apparatus, source_kind, sequence_no
         ) VALUES
             ('rezka-output-typed', 'run-legacy-typed', $1,
              'wip-parent-0', 'qr:wip-parent-0', $2, 'progress_batch', 1),
             ('rezka-output-typed', 'run-legacy-typed', $1,
              'wip-parent-1', 'qr:wip-parent-1', $2, 'progress_batch', 2)",
    )
    .bind(ORDER)
    .bind(APPARATUS)
    .execute(pool)
    .await
    .expect("seed typed output links");

    // D. Legacy scalar-only output: parent_batch_id column, no typed row.
    sqlx::query(
        "INSERT INTO mini_progress_batches (
             batch_id, session_id, apparatus, canonical_apparatus_id, order_id,
             action, status,
             produced_qty, uom, qr_payload, label_item_code, label_item_name,
             worker_role, worker_ref, worker_display_name, wip_status,
             parent_batch_id, payload_json
         ) VALUES (
             'rezka-output-scalar', 'run-legacy-typed', $1, $1, $2, 'complete', 'completed',
             10, 'kg', 'qr:rezka-output-scalar', 'CUTOVER', 'Cutover scalar output',
             'aparatchi', 'worker-1', 'Worker', 'waiting',
             'wip-parent-1', '{}'::jsonb
         )",
    )
    .bind(APPARATUS)
    .bind(ORDER)
    .execute(pool)
    .await
    .expect("seed scalar output batch");

    // E. Canonical payload plus conflicting typed mirrors: payload must win.
    // (Separate apparatus: only one open session per apparatus+order is allowed.)
    sqlx::query(
        "INSERT INTO mini_order_run_sessions (
             session_id, apparatus, canonical_apparatus_id, order_id, status, stage_node_id,
             worker_role, worker_ref, worker_display_name,
             started_at, updated_at, payload_json
         ) VALUES (
             'run-canonical-wins', $1, $1, $2, 'active', 'rezka-stage',
             'aparatchi', 'worker-1', 'Worker',
             $3::timestamptz, $3::timestamptz,
             '{\"input_lineage\": [{\"input_batch_id\": \"wip-canonical\",
                \"input_qr_payload\": \"qr:wip-canonical\",
                \"source_apparatus\": \"apparatus:default:asset-007\",
                \"source_kind\": \"progress_batch\",
                \"stage_node_id\": \"rezka-stage\",
                \"sequence_no\": 1, \"status\": \"in_use\",
                \"linked_at_unix\": 1772352000}]}'::jsonb
         )",
    )
    .bind(APPARATUS_2)
    .bind(ORDER)
    .bind(T_START)
    .execute(pool)
    .await
    .expect("seed canonical session");
    sqlx::query(
        "INSERT INTO mini_order_run_input_links (
             session_id, order_id, target_apparatus,
             input_batch_id, input_qr_payload, source_apparatus, source_kind,
             stage_node_id, sequence_no, status, linked_at, processed_at
         ) VALUES (
             'run-canonical-wins', $1, $2,
             'wip-stale-mirror', 'qr:stale', $2, 'opening_wip',
             'rezka-stage', 1, 'in_use', $3::timestamptz, NULL)",
    )
    .bind(ORDER)
    .bind(APPARATUS_2)
    .bind(T_START)
    .execute(pool)
    .await
    .expect("seed stale session mirror");
    sqlx::query(
        "INSERT INTO mini_progress_batches (
             batch_id, session_id, apparatus, canonical_apparatus_id, order_id,
             action, status,
             produced_qty, uom, qr_payload, label_item_code, label_item_name,
             worker_role, worker_ref, worker_display_name, wip_status, payload_json
         ) VALUES (
             'batch-canonical-wins', 'run-canonical-wins', $1, $1, $2, 'complete', 'completed',
             10, 'kg', 'qr:batch-canonical-wins', 'CUTOVER', 'Canonical output',
             'aparatchi', 'worker-1', 'Worker', 'waiting',
             '{\"source_input_links\": [{\"input_batch_id\": \"wip-canonical\",
                \"input_qr_payload\": \"qr:wip-canonical\",
                \"source_apparatus\": \"apparatus:default:asset-010\",
                \"source_kind\": \"progress_batch\", \"sequence_no\": 1}]}'::jsonb
         )",
    )
    .bind(APPARATUS)
    .bind(ORDER)
    .execute(pool)
    .await
    .expect("seed canonical output batch");
    sqlx::query(
        "INSERT INTO mini_progress_batch_input_links (
             output_batch_id, session_id, order_id,
             input_batch_id, input_qr_payload, source_apparatus, source_kind, sequence_no
         ) VALUES (
             'batch-canonical-wins', 'run-canonical-wins', $1,
             'wip-stale-mirror', 'qr:stale', $2, 'opening_wip', 1)",
    )
    .bind(ORDER)
    .bind(APPARATUS)
    .execute(pool)
    .await
    .expect("seed stale output mirror");

    // Adversarial: malformed canonical payload must lose to valid mirrors.
    // (Each active session uses a distinct apparatus: one open session per
    // apparatus+order is enforced by a unique index. Payload contents stay on
    // asset-010 to prove verbatim recovery.)
    sqlx::query(
        "INSERT INTO mini_order_run_sessions (
             session_id, apparatus, canonical_apparatus_id, order_id, status, stage_node_id,
             worker_role, worker_ref, worker_display_name,
             started_at, updated_at, payload_json
         ) VALUES (
             'run-malformed-lineage', $1, $1, $2, 'active', 'rezka-stage',
             'aparatchi', 'worker-1', 'Worker',
             $3::timestamptz, $3::timestamptz,
             '{\"input_lineage\": [{}]}'::jsonb
         )",
    )
    .bind(APPARATUS_4)
    .bind(ORDER)
    .bind(T_START)
    .execute(pool)
    .await
    .expect("seed malformed lineage session");
    sqlx::query(
        "INSERT INTO mini_order_run_input_links (
             session_id, order_id, target_apparatus,
             input_batch_id, input_qr_payload, source_apparatus, source_kind,
             stage_node_id, sequence_no, status, linked_at, processed_at
         ) VALUES (
             'run-malformed-lineage', $1, $2,
             'wip-parent-0', 'qr:wip-parent-0', 'apparatus:default:asset-010', 'progress_batch',
             'rezka-stage', 1, 'in_use', $3::timestamptz, NULL)",
    )
    .bind(ORDER)
    .bind(APPARATUS_4)
    .bind(T_START)
    .execute(pool)
    .await
    .expect("seed recovery mirror link");
    sqlx::query(
        "INSERT INTO mini_progress_batches (
             batch_id, session_id, apparatus, canonical_apparatus_id, order_id,
             action, status,
             produced_qty, uom, qr_payload, label_item_code, label_item_name,
             worker_role, worker_ref, worker_display_name, wip_status, payload_json
         ) VALUES (
             'batch-malformed-links', 'run-malformed-lineage', $1, $1, $2,
             'complete', 'completed',
             10, 'kg', 'qr:batch-malformed-links', 'CUTOVER', 'Malformed output',
             'aparatchi', 'worker-1', 'Worker', 'waiting',
             '{\"source_input_links\": [{\"input_batch_id\": \"\"}]}'::jsonb
         )",
    )
    .bind(APPARATUS)
    .bind(ORDER)
    .execute(pool)
    .await
    .expect("seed malformed output batch");
    sqlx::query(
        "INSERT INTO mini_progress_batch_input_links (
             output_batch_id, session_id, order_id,
             input_batch_id, input_qr_payload, source_apparatus, source_kind, sequence_no
         ) VALUES (
             'batch-malformed-links', 'run-malformed-lineage', $1,
             'wip-parent-1', 'qr:wip-parent-1', $2, 'progress_batch', 1)",
    )
    .bind(ORDER)
    .bind(APPARATUS)
    .execute(pool)
    .await
    .expect("seed recovery output mirror");
    sqlx::query(
        "INSERT INTO mini_order_run_sessions (
             session_id, apparatus, canonical_apparatus_id, order_id, status, stage_node_id,
             worker_role, worker_ref, worker_display_name,
             started_at, updated_at, payload_json
         ) VALUES (
             'run-malformed-rolls', $1, $1, $2, 'active', 'rezka-stage',
             'aparatchi', 'worker-1', 'Worker',
             $3::timestamptz, $3::timestamptz,
             '{\"input_lineage\": [{\"input_batch_id\": \"wip-parent-0\",
                \"input_qr_payload\": \"qr:wip-parent-0\",
                \"source_apparatus\": \"apparatus:default:asset-010\",
                \"source_kind\": \"progress_batch\",
                \"stage_node_id\": \"rezka-stage\",
                \"sequence_no\": 1, \"status\": \"in_use\",
                \"linked_at_unix\": 1772352000}],
               \"rezka_active_partial_rolls\": [{\"slot_index\": 0,
                \"generation\": 1, \"contained_kadr_count\": 1, \"status\": \"active\",
                \"source_input_batch_ids\": [\"wip-parent-0\"],
                \"started_at_unix\": 1772352000, \"updated_at_unix\": 1772352000}]}'::jsonb
         )",
    )
    .bind(APPARATUS_5)
    .bind(ORDER)
    .bind(T_START)
    .execute(pool)
    .await
    .expect("seed malformed rolls session");
    sqlx::query(
        "INSERT INTO mini_rezka_active_partial_rolls (
             session_id, order_id, apparatus, slot_index, generation,
             contained_kadr_count, status, source_input_batch_ids,
             started_at, updated_at
         ) VALUES (
             'run-malformed-rolls', $1, $2, 1, 1,
             1, 'active', ARRAY['wip-parent-0'],
             $3::timestamptz, $3::timestamptz
         )",
    )
    .bind(ORDER)
    .bind(APPARATUS_5)
    .bind(T_START)
    .execute(pool)
    .await
    .expect("seed recovery roll mirror");
    sqlx::query(
        "INSERT INTO mini_order_run_sessions (
             session_id, apparatus, canonical_apparatus_id, order_id, status, stage_node_id,
             worker_role, worker_ref, worker_display_name,
             started_at, updated_at, payload_json
         ) VALUES (
             'run-dup-seq', $1, $1, $2, 'active', 'rezka-stage',
             'aparatchi', 'worker-1', 'Worker',
             $3::timestamptz, $3::timestamptz,
             '{\"input_lineage\": [
                {\"input_batch_id\": \"wip-parent-0\",
                 \"input_qr_payload\": \"qr:wip-parent-0\",
                 \"source_apparatus\": \"apparatus:default:asset-010\",
                 \"source_kind\": \"progress_batch\",
                 \"stage_node_id\": \"rezka-stage\",
                 \"sequence_no\": 1, \"status\": \"processed\",
                 \"linked_at_unix\": 1772352000, \"processed_at_unix\": 1772357400},
                {\"input_batch_id\": \"wip-parent-1\",
                 \"input_qr_payload\": \"qr:wip-parent-1\",
                 \"source_apparatus\": \"apparatus:default:asset-010\",
                 \"source_kind\": \"progress_batch\",
                 \"stage_node_id\": \"rezka-stage\",
                 \"sequence_no\": 1, \"status\": \"in_use\",
                 \"linked_at_unix\": 1772357400}]}'::jsonb
         )",
    )
    .bind(APPARATUS_6)
    .bind(ORDER)
    .bind(T_START)
    .execute(pool)
    .await
    .expect("seed duplicate-sequence session");
    sqlx::query(
        "INSERT INTO mini_order_run_input_links (
             session_id, order_id, target_apparatus,
             input_batch_id, input_qr_payload, source_apparatus, source_kind,
             stage_node_id, sequence_no, status, linked_at, processed_at
         ) VALUES (
             'run-dup-seq', $1, $2,
             'wip-parent-0', 'qr:wip-parent-0', 'apparatus:default:asset-010', 'progress_batch',
             'rezka-stage', 1, 'in_use', $3::timestamptz, NULL)",
    )
    .bind(ORDER)
    .bind(APPARATUS_6)
    .bind(T_START)
    .execute(pool)
    .await
    .expect("seed duplicate-sequence recovery mirror");
    sqlx::query(
        "INSERT INTO mini_progress_batches (
             batch_id, session_id, apparatus, canonical_apparatus_id, order_id,
             action, status,
             produced_qty, uom, qr_payload, label_item_code, label_item_name,
             worker_role, worker_ref, worker_display_name, wip_status, payload_json
         ) VALUES (
             'batch-bad-kind', 'run-malformed-lineage', $1, $1, $2,
             'complete', 'completed',
             10, 'kg', 'qr:batch-bad-kind', 'CUTOVER', 'Bad kind output',
             'aparatchi', 'worker-1', 'Worker', 'waiting',
             '{\"source_input_links\": [{\"input_batch_id\": \"wip-parent-0\",
                \"input_qr_payload\": \"qr:wip-parent-0\",
                \"source_apparatus\": \"apparatus:default:asset-010\",
                \"source_kind\": \"mystery\", \"sequence_no\": 1}]}'::jsonb
         )",
    )
    .bind(APPARATUS)
    .bind(ORDER)
    .execute(pool)
    .await
    .expect("seed bad-kind output batch");
    sqlx::query(
        "INSERT INTO mini_progress_batch_input_links (
             output_batch_id, session_id, order_id,
             input_batch_id, input_qr_payload, source_apparatus, source_kind, sequence_no
         ) VALUES (
             'batch-bad-kind', 'run-malformed-lineage', $1,
             'wip-parent-0', 'qr:wip-parent-0', $2, 'progress_batch', 1)",
    )
    .bind(ORDER)
    .bind(APPARATUS)
    .execute(pool)
    .await
    .expect("seed bad-kind recovery mirror");

    // Explicit empty arrays are valid canonical lineage and must be kept even
    // when stale mirrors exist (the completion flow writes `[]` deliberately).
    sqlx::query(
        "INSERT INTO mini_order_run_sessions (
             session_id, apparatus, canonical_apparatus_id, order_id, status, stage_node_id,
             worker_role, worker_ref, worker_display_name,
             started_at, updated_at, payload_json
         ) VALUES (
             'run-empty-kept', $1, $1, $2, 'active', 'rezka-stage',
             'aparatchi', 'worker-1', 'Worker',
             $3::timestamptz, $3::timestamptz,
             '{\"input_lineage\": [], \"rezka_active_partial_rolls\": []}'::jsonb
         )",
    )
    .bind(APPARATUS_7)
    .bind(ORDER)
    .bind(T_START)
    .execute(pool)
    .await
    .expect("seed empty-lineage session");
    sqlx::query(
        "INSERT INTO mini_order_run_input_links (
             session_id, order_id, target_apparatus,
             input_batch_id, input_qr_payload, source_apparatus, source_kind,
             stage_node_id, sequence_no, status, linked_at, processed_at
         ) VALUES (
             'run-empty-kept', $1, $2,
             'wip-stale-mirror', 'qr:stale', $2, 'progress_batch',
             'rezka-stage', 1, 'in_use', $3::timestamptz, NULL)",
    )
    .bind(ORDER)
    .bind(APPARATUS_7)
    .bind(T_START)
    .execute(pool)
    .await
    .expect("seed empty-lineage stale mirror");
    sqlx::query(
        "INSERT INTO mini_rezka_active_partial_rolls (
             session_id, order_id, apparatus, slot_index, generation,
             contained_kadr_count, status, source_input_batch_ids,
             started_at, updated_at
         ) VALUES (
             'run-empty-kept', $1, $2, 1, 1,
             1, 'active', ARRAY['wip-stale-mirror'],
             $3::timestamptz, $3::timestamptz
         )",
    )
    .bind(ORDER)
    .bind(APPARATUS_7)
    .bind(T_START)
    .execute(pool)
    .await
    .expect("seed empty-rolls stale mirror");
    sqlx::query(
        "INSERT INTO mini_progress_batches (
             batch_id, session_id, apparatus, canonical_apparatus_id, order_id,
             action, status,
             produced_qty, uom, qr_payload, label_item_code, label_item_name,
             worker_role, worker_ref, worker_display_name, wip_status, payload_json
         ) VALUES (
             'batch-empty-kept', 'run-empty-kept', $1, $1, $2,
             'complete', 'completed',
             10, 'kg', 'qr:batch-empty-kept', 'CUTOVER', 'Empty output',
             'aparatchi', 'worker-1', 'Worker', 'waiting',
             '{\"source_input_links\": []}'::jsonb
         )",
    )
    .bind(APPARATUS_7)
    .bind(ORDER)
    .execute(pool)
    .await
    .expect("seed empty output batch");
    sqlx::query(
        "INSERT INTO mini_progress_batch_input_links (
             output_batch_id, session_id, order_id,
             input_batch_id, input_qr_payload, source_apparatus, source_kind, sequence_no
         ) VALUES (
             'batch-empty-kept', 'run-empty-kept', $1,
             'wip-stale-mirror', 'qr:stale', $2, 'progress_batch', 1)",
    )
    .bind(ORDER)
    .bind(APPARATUS_7)
    .execute(pool)
    .await
    .expect("seed empty-output stale mirror");

    // A JSON number is not a valid input_batch_id (Rust needs String) and
    // must lose to the valid mirror.
    sqlx::query(
        "INSERT INTO mini_order_run_sessions (
             session_id, apparatus, canonical_apparatus_id, order_id, status, stage_node_id,
             worker_role, worker_ref, worker_display_name,
             started_at, updated_at, payload_json
         ) VALUES (
             'run-numeric-id', $1, $1, $2, 'active', 'rezka-stage',
             'aparatchi', 'worker-1', 'Worker',
             $3::timestamptz, $3::timestamptz,
             '{\"input_lineage\": [{\"input_batch_id\": 123,
                \"input_qr_payload\": \"qr:x\",
                \"source_apparatus\": \"apparatus:default:asset-010\",
                \"source_kind\": \"progress_batch\",
                \"stage_node_id\": \"rezka-stage\",
                \"sequence_no\": 1, \"status\": \"in_use\",
                \"linked_at_unix\": 1772352000}]}'::jsonb
         )",
    )
    .bind(APPARATUS_8)
    .bind(ORDER)
    .bind(T_START)
    .execute(pool)
    .await
    .expect("seed numeric-id session");
    sqlx::query(
        "INSERT INTO mini_order_run_input_links (
             session_id, order_id, target_apparatus,
             input_batch_id, input_qr_payload, source_apparatus, source_kind,
             stage_node_id, sequence_no, status, linked_at, processed_at
         ) VALUES (
             'run-numeric-id', $1, $2,
             'wip-parent-1', 'qr:wip-parent-1', 'apparatus:default:asset-010', 'progress_batch',
             'rezka-stage', 1, 'in_use', $3::timestamptz, NULL)",
    )
    .bind(ORDER)
    .bind(APPARATUS_8)
    .bind(T_START)
    .execute(pool)
    .await
    .expect("seed numeric-id recovery mirror");
}

#[tokio::test]
async fn rezka_lineage_cutover_aborts_on_unrecoverable_payload() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://superuser@127.0.0.1:5432/postgres".to_string());
    let db_name = format!(
        "mini_rs_erp_test_rezka_lineage_cutover_abort_{}",
        std::process::id()
    );
    let admin_pool = sqlx::PgPool::connect(&admin_url).await.expect("admin db");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop test db");
    sqlx::query(&format!(r#"CREATE DATABASE "{db_name}""#))
        .execute(&admin_pool)
        .await
        .expect("create test db");
    admin_pool.close().await;

    let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&admin_url, &db_name))
        .await
        .expect("test db");
    apply_postgres_migrations_through(&pool, 91)
        .await
        .expect("apply migrations up to 0091");
    seed_standard_canonical_apparatus(&pool).await;

    // D. Malformed canonical payload with NO mirror rows and NO fallback
    // source: nothing safe to recover from, so the cutover must abort
    // instead of dropping history it cannot represent.
    sqlx::query(
        "INSERT INTO mini_production_maps (id, product_code, title, map_json)
         VALUES ('order-hopeless', 'CUTOVER', 'Hopeless map', '{}'::jsonb)",
    )
    .execute(&pool)
    .await
    .expect("seed hopeless order map");
    sqlx::query(
        "INSERT INTO mini_order_run_sessions (
             session_id, apparatus, canonical_apparatus_id, order_id, status, stage_node_id,
             worker_role, worker_ref, worker_display_name,
             started_at, updated_at, payload_json
         ) VALUES (
             'run-hopeless', $1, $1, 'order-hopeless', 'active', 'rezka-stage',
             'aparatchi', 'worker-1', 'Worker',
             $2::timestamptz, $2::timestamptz,
             '{\"input_lineage\": [{}]}'::jsonb
         )",
    )
    .bind(APPARATUS)
    .bind(T_START)
    .execute(&pool)
    .await
    .expect("seed hopeless session");

    apply_foundation_migration(&pool)
        .await
        .expect_err("cutover must fail closed on unrecoverable lineage");

    // The abort is atomic: mirrors were NOT dropped and payload is untouched.
    for table in [
        "mini_order_run_input_links",
        "mini_rezka_active_partial_rolls",
        "mini_progress_batch_input_links",
    ] {
        let exists: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(table)
            .fetch_one(&pool)
            .await
            .expect("mirror table existence probe after abort");
        assert!(exists, "{table} must survive the aborted cutover");
    }
    let hopeless: serde_json::Value = sqlx::query_scalar(
        "SELECT payload_json->'input_lineage'
         FROM mini_order_run_sessions WHERE session_id = 'run-hopeless'",
    )
    .fetch_one(&pool)
    .await
    .expect("hopeless payload after abort");
    assert_eq!(hopeless, serde_json::json!([{}]));

    // Cleanup test database.
    pool.close().await;
    let admin_pool = sqlx::PgPool::connect(&admin_url)
        .await
        .expect("admin cleanup");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop test db");
}
