//! Real PostgreSQL A/B experiment. No injected delay or production DB writes.
use crate::core::apparatus_standard::test_support::TestApparatusSpec;
use crate::core::apparatus_standard::{CanonicalApparatusService, ProcessTechnology};
use crate::core::production_map::*;
use crate::db::postgres::{apply_foundation_migration, postgres_test_database_options};
use crate::db::postgres_canonical_apparatus::PostgresCanonicalApparatusRepository;
use crate::db::postgres_production_map::PostgresProductionMapStore;
use std::sync::Arc;
use std::time::Instant;

const STATION: &str = "apparatus:test:worker-hot-path";
const OTHER: &str = "apparatus:test:worker-unrelated";

fn map(id: &str, station: &str, number: usize) -> ProductionMapDefinition {
    serde_json::from_value(serde_json::json!({
        "id": id, "title": id, "product_code": "BENCH", "order_number": format!("{number:04}"),
        "nodes": [{"id":"start","kind":"start","title":"Start"},
            {"id":"print","kind":"apparatus","title":"Print","apparatus_id":station},
            {"id":"end","kind":"end","title":"End"}],
        "edges": [{"from":"start","to":"print"},{"from":"print","to":"end"}]
    }))
    .unwrap()
}

fn actor() -> QueueActionActor {
    QueueActionActor {
        role: "aparatchi".into(),
        ref_: "bench-worker".into(),
        display_name: "Bench Worker".into(),
    }
}

fn report() -> QueueProgressInput {
    QueueProgressInput {
        produced_qty: Some(10.0),
        finished_goods_meter: Some(10.0),
        finished_goods_kg: Some(2.0),
        gross_qty: Some(2.0),
        bobina_kg: Some(0.1),
        total_waste: Some(1.0),
        return_ink_kg: Some(0.5),
        ..Default::default()
    }
}

#[tokio::test]
#[ignore = "requires local PostgreSQL; creates and drops its own randomly named benchmark database"]
async fn postgres_worker_hot_path_benchmark_and_conflict_parity() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
    let admin = sqlx::PgPool::connect(&admin_url).await.unwrap();
    let db_name = format!("mini_rs_worker_bench_{:016x}", rand::random::<u64>());
    sqlx::query(&format!("CREATE DATABASE {db_name}"))
        .execute(&admin)
        .await
        .unwrap();
    println!("WORKER_BENCH_DATABASE {db_name}");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(8)
        .connect_with(postgres_test_database_options(&admin_url, &db_name))
        .await
        .unwrap();
    let task_pool = pool.clone();
    let task_name = db_name.clone();
    let outcome =
        tokio::spawn(async move { exercise_hot_path(&task_pool, &task_name).await }).await;
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {db_name} WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    outcome.expect("benchmark/parity assertions passed; isolated database cleaned up");
}

async fn exercise_hot_path(pool: &sqlx::PgPool, db_name: &str) {
    let concentrated = std::env::var("WORKER_BENCH_CONCENTRATED").as_deref() == Ok("1");
    let background_station = if concentrated { STATION } else { OTHER };
    println!(
        "WORKER_BENCH {}",
        serde_json::json!({"metric":"dataset", "orders":1025,
        "historical_sessions":5000,"orders_on_target":if concentrated {1025} else {25},
        "profile":"dev_unoptimized"})
    );
    apply_foundation_migration(&pool).await.unwrap();
    for station in [STATION, OTHER] {
        super::seed_canonical_apparatus(
            &pool,
            TestApparatusSpec::print(station, station, ProcessTechnology::Rotogravure, Some(7)),
        )
        .await;
    }
    let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
    let legacy = Arc::new(PostgresProductionMapStore::new(pool.clone()).with_legacy_queue_reads());
    let resolver = Arc::new(CanonicalServiceApparatusResolver::new(
        CanonicalApparatusService::new(Arc::new(PostgresCanonicalApparatusRepository::new(
            pool.clone(),
        ))),
    ));
    let current_service = ProductionMapService::new(store.clone(), resolver.clone());
    let legacy_service = ProductionMapService::new(legacy.clone(), resolver);
    let mut maps: Vec<_> = (1..=1000)
        .map(|n| map(&format!("zakaz-background-{n}"), background_station, n))
        .collect();
    let ids: Vec<_> = (0..12)
        .flat_map(|sample| {
            [
                format!("zakaz-bench-old-{sample}"),
                format!("zakaz-bench-new-{sample}"),
            ]
        })
        .collect();
    maps.extend(
        ids.iter()
            .enumerate()
            .map(|(n, id)| map(id, STATION, 2000 + n)),
    );
    maps.push(map("zakaz-conflict", STATION, 2999));
    store.put_maps_batch(&maps).await.unwrap();
    let mut sequence = ids.clone();
    sequence.push("zakaz-conflict".into());
    store
        .put_apparatus_sequence(STATION, sequence)
        .await
        .unwrap();
    // Completed history must not be loaded just to check today's capacity.
    sqlx::query(
        "INSERT INTO mini_order_run_sessions
        (session_id, apparatus, canonical_apparatus_id, order_id, stage_node_id, status,
         worker_role, worker_ref, worker_display_name, started_at, updated_at, payload_json)
        SELECT 'bench-history-' || n, $1, $1, 'zakaz-background-1', 'print', 'completed',
               'aparatchi', 'history-worker', 'History Worker', now(), now(), '{}'::jsonb
        FROM generate_series(1,5000) n",
    )
    .bind(background_station)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("ANALYZE").execute(pool).await.unwrap();

    // Point reads retain existing missing/normalization behavior and are fresh.
    for id in [&ids[0][..], "missing", ""] {
        assert_eq!(
            serde_json::to_value(store.map_by_id(id).await.unwrap()).unwrap(),
            serde_json::to_value(legacy.map_by_id(id).await.unwrap()).unwrap()
        );
    }
    assert_eq!(
        store.maps_for_apparatus(STATION).await.unwrap().len(),
        if concentrated { 1025 } else { 25 }
    );
    assert_eq!(
        store.maps_for_apparatus(OTHER).await.unwrap().len(),
        if concentrated { 0 } else { 1000 }
    );
    let candidate_ids = store
        .maps_for_apparatus(STATION)
        .await
        .unwrap()
        .into_iter()
        .map(|map| map.id)
        .collect::<Vec<_>>();
    let full_read_ids = legacy
        .maps()
        .await
        .unwrap()
        .into_iter()
        .filter(|map| map.nodes.iter().any(|node| node.apparatus_id == STATION))
        .map(|map| map.id)
        .collect::<Vec<_>>();
    assert_eq!(
        candidate_ids, full_read_ids,
        "same-timestamp maps must keep identical implicit queue order"
    );
    assert_eq!(
        store
            .active_order_run_sessions_for_apparatus(STATION)
            .await
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        legacy
            .active_order_run_sessions_for_apparatus(STATION)
            .await
            .unwrap()
            .len(),
        0
    );
    let mut changed = store.map_by_id(&ids[0]).await.unwrap().unwrap();
    changed.customer_name = "Fresh after write".into();
    store.put_map(changed).await.unwrap();
    assert_eq!(
        current_service
            .raw_map(&ids[0])
            .await
            .unwrap()
            .unwrap()
            .customer_name,
        "Fresh after write"
    );
    for use_legacy in [true, false] {
        let reader: &dyn ProductionMapStorePort = if use_legacy {
            legacy.as_ref()
        } else {
            store.as_ref()
        };
        let begin = Instant::now();
        for _ in 0..30 {
            assert!(reader.map_by_id(&ids[0]).await.unwrap().is_some());
        }
        println!(
            "WORKER_BENCH {}",
            serde_json::json!({"metric":"map_point_read", "legacy":use_legacy,
            "orders":1025,"historical_sessions":5000,"iterations":30,"mean_ms":begin.elapsed().as_secs_f64()*1000.0/30.0})
        );
    }
    use queue_state::ApparatusQueueAction as A;
    for (index, id) in ids.iter().enumerate() {
        let use_legacy = index % 2 == 0;
        let service = if use_legacy {
            &legacy_service
        } else {
            &current_service
        };
        for action in [A::Start, A::Pause, A::Resume, A::Complete] {
            let progress = if matches!(action, A::Pause | A::Complete) {
                report()
            } else {
                Default::default()
            };
            let begin = Instant::now();
            let result = service
                .apply_apparatus_queue_action_with_progress(
                    STATION,
                    id,
                    action,
                    &[STATION.into()],
                    actor(),
                    progress,
                )
                .await
                .unwrap_or_else(|e| panic!("{id} {action:?}: {e:?}"));
            let elapsed_ms = begin.elapsed().as_secs_f64() * 1000.0;
            let expected = match action {
                A::Start | A::Resume => "in_progress",
                A::Pause => "paused",
                _ => "completed",
            };
            assert_eq!(result.states.get(id).map(String::as_str), Some(expected));
            if index >= 4 {
                println!(
                    "WORKER_BENCH {}",
                    serde_json::json!({"metric":"queue_action_committed", "legacy":use_legacy,
                    "action":format!("{action:?}"),"sample":index/2-2,"elapsed_ms":elapsed_ms})
                );
            }
        }
    }
    // Two independently prepared writes see pending. Only one may commit;
    // this deliberately bypasses the in-process guard to test DB protection.
    let one = current_service
        .prepare_apparatus_queue_action_with_progress(
            STATION,
            "zakaz-conflict",
            A::Start,
            &[STATION.into()],
            actor(),
            Default::default(),
        )
        .await
        .unwrap();
    let two = current_service
        .prepare_apparatus_queue_action_with_progress(
            STATION,
            "zakaz-conflict",
            A::Start,
            &[STATION.into()],
            actor(),
            Default::default(),
        )
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        current_service.commit_prepared_queue_action(one),
        current_service.commit_prepared_queue_action(two)
    );
    assert_eq!(
        usize::from(a.is_ok()) + usize::from(b.is_ok()),
        1,
        "one stale start must fail: {a:?} {b:?}"
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_queue_action_events WHERE order_id = 'zakaz-conflict' AND action = 'start'")
        .fetch_one(pool).await.unwrap();
    assert_eq!(count, 1);
    assert_eq!(
        store
            .active_order_run_sessions_for_apparatus(STATION)
            .await
            .unwrap()
            .len(),
        1
    );
    println!(
        "WORKER_BENCH {}",
        serde_json::json!({"metric":"conflict_parity", "same_pending_start_writers":2, "commits":1,
        "events":count, "fresh_read_after_write":true, "database":db_name})
    );
    // Fresh control reads must include the joined canonical freeze target and
    // must observe cancellation without a cache or an omitted control row.
    let mut admin_actor = actor();
    admin_actor.role = "admin".into();
    current_service
        .request_order_freeze("zakaz-conflict", admin_actor.clone())
        .await
        .unwrap();
    for expected in [
        OrderControlState::FreezeRequested,
        OrderControlState::Active,
    ] {
        let fast = store.order_control_by_id("zakaz-conflict").await.unwrap();
        let old = legacy.order_control_by_id("zakaz-conflict").await.unwrap();
        assert_eq!(
            serde_json::to_value(&fast).unwrap(),
            serde_json::to_value(old).unwrap()
        );
        assert_eq!(fast.unwrap().state, expected);
        if expected == OrderControlState::FreezeRequested {
            current_service
                .cancel_order_freeze_request("zakaz-conflict", admin_actor.clone())
                .await
                .unwrap();
        }
    }
    assert!(
        store
            .order_control_by_id("missing")
            .await
            .unwrap()
            .is_none()
    );
    // A double tap / independently prepared completion must not mint two WIP
    // outputs or account for the same produced quantity twice.
    let one = current_service
        .prepare_apparatus_queue_action_with_progress(
            STATION,
            "zakaz-conflict",
            A::Complete,
            &[STATION.into()],
            actor(),
            report(),
        )
        .await
        .unwrap();
    let two = current_service
        .prepare_apparatus_queue_action_with_progress(
            STATION,
            "zakaz-conflict",
            A::Complete,
            &[STATION.into()],
            actor(),
            report(),
        )
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        current_service.commit_prepared_queue_action(one),
        current_service.commit_prepared_queue_action(two),
    );
    assert_eq!(
        usize::from(a.is_ok()) + usize::from(b.is_ok()),
        1,
        "one stale completion must fail: {a:?} {b:?}"
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_queue_action_events WHERE order_id = 'zakaz-conflict' AND action = 'complete'")
        .fetch_one(pool).await.unwrap();
    let outputs = store
        .progress_batches_for_order("zakaz-conflict")
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0].produced_qty, 10.0);
    assert!(
        store
            .active_order_run_sessions_for_apparatus(STATION)
            .await
            .unwrap()
            .is_empty()
    );
    println!(
        "WORKER_BENCH {}",
        serde_json::json!({"metric":"completion_conflict_parity",
        "same_active_complete_writers":2,"commits":1,"events":count,"output_batches":outputs.len(),
        "produced_qty":outputs[0].produced_qty})
    );
    // Seed read-only adapter fixtures with a legacy display snapshot: canonical
    // identity comes from the typed column, not the display text in JSON.
    for (index, order_id) in [ids[0].as_str(), "zakaz-background-1"]
        .into_iter()
        .enumerate()
    {
        sqlx::query("INSERT INTO mini_raw_material_stock
            (id, warehouse, item_code, item_name, barcode, qty, uom, status, source_receipt_id, payload_json)
            VALUES ($1,'Warehouse A','BOPP','Film',$1,10,'kg','available','bench-receipt','{}'::jsonb)")
            .bind(format!("BENCH-RAW-{index}")).execute(pool).await.unwrap();
        let payload = serde_json::json!({"order_id":order_id, "apparatus_id":"Legacy label",
            "apparatus":"Legacy label", "barcode":format!("BENCH-RAW-{index}"),
            "item_code":"BOPP", "item_name":"Film", "item_group":"rulon",
            "assigned_by_role":"material_taminotchi", "assigned_by_ref":"bench-supply",
            "assigned_by_display_name":"Supply", "assigned_at":"2026-09-07T00:00:00Z"});
        sqlx::query("INSERT INTO mini_raw_material_assignments
            (barcode, order_id, apparatus, canonical_apparatus_id, item_code, item_group, payload_json)
            VALUES ($1,$2,'Legacy label',$3,'BOPP','rulon',$4)")
            .bind(format!("BENCH-RAW-{index}")).bind(order_id).bind(STATION).bind(payload)
            .execute(pool).await.unwrap();
    }
    for order_id in [ids[0].as_str(), "zakaz-background-1", "missing"] {
        let fast = store
            .raw_material_assignments_for_order(order_id)
            .await
            .unwrap();
        let old = legacy
            .raw_material_assignments_for_order(order_id)
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(&fast).unwrap(),
            serde_json::to_value(old).unwrap()
        );
        assert_eq!(fast.len(), usize::from(order_id != "missing"));
        for assignment in fast {
            assert_eq!(assignment.apparatus_id.as_str(), STATION);
            assert_eq!(assignment.apparatus, "Legacy label");
        }
    }
    let mut moved = store
        .map_by_id("zakaz-background-2")
        .await
        .unwrap()
        .unwrap();
    moved.nodes[1].apparatus_id = if concentrated { OTHER } else { STATION }.into();
    store.put_map(moved).await.unwrap();
    assert_eq!(
        store.maps_for_apparatus(STATION).await.unwrap().len(),
        if concentrated { 1024 } else { 26 }
    );
    println!(
        "WORKER_BENCH {}",
        serde_json::json!({"metric":"scoped_read_parity",
        "freeze_cancel_fresh":true,"material_scope_and_canonical_identity":true,"map_projection_fresh":true})
    );
}
