use crate::core::production_map::*;
use crate::db::{
    postgres::{apply_foundation_migration, postgres_test_database_options},
    postgres_production_map::PostgresProductionMapStore,
};
use queue_state::ApparatusQueueAction as A;
use std::sync::Arc;
const P: &str = "apparatus:default:bosma_9";
const X: &str = "apparatus:default:asset-007";
const Y: &str = "apparatus:default:asset-008";
const ORDER: &str = "zakaz-cooperative-db";
fn actor(id: &str) -> QueueActionActor {
    QueueActionActor {
        role: "aparatchi".into(),
        ref_: id.into(),
        display_name: id.into(),
    }
}
async fn act(
    s: &ProductionMapService,
    machine: &str,
    action: A,
    progress: QueueProgressInput,
) -> ApparatusQueueActionResult {
    // Exercise the persistence boundary directly; physical Qolip/warehouse
    // scanning is covered by the route authorization tests, not this fixture.
    let prepared = s
        .prepare_apparatus_queue_action_with_progress(
            machine,
            ORDER,
            action,
            &[machine.into()],
            actor(machine),
            progress,
        )
        .await
        .unwrap_or_else(|e| panic!("{machine} {action:?}: {e:?}"));
    s.commit_prepared_queue_action(prepared)
        .await
        .unwrap_or_else(|e| panic!("commit {machine} {action:?}: {e:?}"))
}

#[tokio::test]
async fn postgres_cooperative_claim_race_late_closure_reports_and_restart() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .expect("isolated test PostgreSQL URL required");
    let admin = sqlx::PgPool::connect(&url).await.unwrap();
    let name = format!(
        "mini_rs_erp_test_cooperative_{:016x}",
        rand::random::<u64>()
    );
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&admin)
        .await
        .unwrap();
    let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&url, &name))
        .await
        .unwrap();
    apply_foundation_migration(&pool).await.unwrap();
    super::seed_standard_canonical_apparatus(&pool).await;
    let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
    let s = ProductionMapService::new_for_test(store.clone());
    let map:ProductionMapDefinition=serde_json::from_value(serde_json::json!({"id":ORDER,"product_code":"SHARED","title":"Shared",
        "nodes":[{"id":"start","kind":"start","title":"Start"},
        {"id":"source","kind":"apparatus","title":"Bosma 9","apparatus_id":P},
        {"id":"a","kind":"apparatus","title":"Laminatsiya 1","apparatus_id":X,"alternative_group_id":"shared","alternative_assigned_apparatus_id":X},
        {"id":"b","kind":"apparatus","title":"Laminatsiya 2","apparatus_id":Y,"alternative_group_id":"shared","alternative_assigned_apparatus_id":X},
        {"id":"end","kind":"end","title":"End"}],
        "edges":[{"from":"start","to":"source"},{"from":"source","to":"a"},{"from":"source","to":"b"},{"from":"a","to":"end"},{"from":"b","to":"end"}]})).unwrap();
    s.upsert_map(map).await.unwrap();
    act(&s, P, A::Start, QueueProgressInput::default()).await;
    let first = act(
        &s,
        P,
        A::DetachRoll,
        QueueProgressInput {
            produced_qty: Some(50.0),
            uom: "m".into(),
            ..Default::default()
        },
    )
    .await
    .progress_batch
    .unwrap();
    act(&s, P, A::Resume, QueueProgressInput::default()).await;
    // Two independent preflights see the same unclaimed roll. Only the first
    // committing transaction may own it; the second must leave no session/event.
    let left = s
        .prepare_apparatus_queue_action_with_progress(
            X,
            ORDER,
            A::Start,
            &[X.into()],
            actor(X),
            QueueProgressInput {
                qr_payload: first.qr_payload.clone(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let right = s
        .prepare_apparatus_queue_action_with_progress(
            Y,
            ORDER,
            A::Start,
            &[Y.into()],
            actor(Y),
            QueueProgressInput {
                qr_payload: first.qr_payload,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let (left, right) = tokio::join!(
        s.commit_prepared_queue_action(left),
        s.commit_prepared_queue_action(right)
    );
    assert_ne!(
        left.is_ok(),
        right.is_ok(),
        "exactly one concurrent claim must commit"
    );
    let second_machine = if left.is_ok() { Y } else { X };
    assert!(
        store
            .active_order_run_session(second_machine, ORDER)
            .await
            .unwrap()
            .is_none()
    );
    let last = act(
        &s,
        P,
        A::DetachRoll,
        QueueProgressInput {
            produced_qty: Some(50.0),
            uom: "m".into(),
            ..Default::default()
        },
    )
    .await
    .progress_batch
    .unwrap();
    act(
        &s,
        second_machine,
        A::Start,
        QueueProgressInput {
            qr_payload: last.qr_payload.clone(),
            ..Default::default()
        },
    )
    .await;
    for machine in [X, Y] {
        act(
            &s,
            machine,
            A::Complete,
            QueueProgressInput {
                produced_qty: Some(45.0),
                uom: "m".into(),
                finished_goods_meter: Some(45.0),
                finished_goods_kg: Some(10.0),
                ..Default::default()
            },
        )
        .await;
    }
    // One report precedes upstream closure. The other will be requested later.
    s.record_laminatsiya_astatka(
        Y,
        ORDER,
        actor(Y),
        Some(0.0),
        Some(0.0),
        Some(0.0),
        None,
        None,
        None,
        "",
    )
    .await
    .unwrap();
    let before = store.progress_batches_for_order(ORDER).await.unwrap();
    let reported = s.live_snapshot_shared().await.unwrap();
    assert!(!reported.visible_order_ids[Y].contains(&ORDER.into()), "local report need not wait for the open producer");
    assert!(reported.visible_order_ids[X].contains(&ORDER.into()));
    assert!(reported.queue_action_controls[Y][ORDER].stage_work.as_ref().unwrap().local_completed);
    assert!(!reported.queue_action_controls[Y][ORDER].stage_work.as_ref().unwrap().completed);
    act(
        &s,
        P,
        A::Complete,
        QueueProgressInput {
            complete_without_output: true,
            progress_batch_id: last.batch_id,
            total_waste: Some(0.0),
            return_ink_kg: Some(0.0),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(
        before,
        store.progress_batches_for_order(ORDER).await.unwrap()
    );
    let controls = s.queue_action_controls().await.unwrap();
    assert!(
        controls[X][ORDER]
            .stage_work
            .as_ref()
            .unwrap()
            .astatka_required
    );
    assert!(
        !controls[Y][ORDER]
            .stage_work
            .as_ref()
            .unwrap()
            .astatka_required
    );
    // Read through a new service so visibility must come from persisted local
    // report/session facts, not an in-process success flag or cached UI state.
    let reloaded = ProductionMapService::new_for_test(Arc::new(PostgresProductionMapStore::new(pool.clone())));
    let snapshot = reloaded.live_snapshot_shared().await.unwrap();
    assert!(!snapshot.visible_order_ids[Y].contains(&ORDER.into()));
    assert!(snapshot.visible_order_ids[X].contains(&ORDER.into()));
    assert_eq!(snapshot.order_statuses[ORDER].lifecycle_status, ProductionOrderLifecycleStatus::InProgress);
    // A form prepared against an older execution snapshot must roll back its
    // report when the execution changes before the transaction acquires locks.
    let anchor = s.astatka_execution_anchor(ORDER, X).await.unwrap().unwrap();
    let mut changed = anchor.clone();
    changed.status = OrderRunStatus::Active;
    changed.updated_at_unix += 1;
    store.put_order_run_session(changed).await.unwrap();
    let mut stale = store
        .laminatsiya_astatka_reports_for_order(ORDER)
        .await
        .unwrap()
        .pop()
        .unwrap();
    stale.apparatus = X.into();
    stale.report_id = "stale-execution-report".into();
    assert!(
        store
            .commit_stage_astatka_report(
                StageAstatkaReport::Laminate(stale),
                Some(anchor.clone()),
                actor(X)
            )
            .await
            .is_err()
    );
    assert_eq!(
        store
            .laminatsiya_astatka_reports_for_order(ORDER)
            .await
            .unwrap()
            .len(),
        1
    );
    store.put_order_run_session(anchor).await.unwrap();
    s.record_laminatsiya_astatka(
        X,
        ORDER,
        actor(X),
        Some(0.0),
        Some(0.0),
        Some(0.0),
        None,
        None,
        None,
        "",
    )
    .await
    .unwrap();
    assert_eq!(
        s.order_status_detail(ORDER).await.unwrap().lifecycle_status,
        ProductionOrderLifecycleStatus::ProductionCompleted
    );
    // Repeating Y's astatka must not move Y behind X in final attribution.
    s.record_laminatsiya_astatka(
        Y,
        ORDER,
        actor(Y),
        Some(0.0),
        Some(0.0),
        Some(0.0),
        None,
        None,
        None,
        "",
    )
    .await
    .unwrap();
    let restarted =
        ProductionMapService::new_for_test(Arc::new(PostgresProductionMapStore::new(pool.clone())));
    let controls = restarted.queue_action_controls().await.unwrap();
    let snapshot = restarted.live_snapshot_shared().await.unwrap();
    for machine in [X, Y] {
        assert!(!snapshot.visible_order_ids[machine].contains(&ORDER.into()));
    }
    assert_eq!(
        restarted.fully_completed_orders(10).await.unwrap()[0].closed_by_ref,
        X
    );
    for machine in [X, Y] {
        let work = controls[machine][ORDER].stage_work.as_ref().unwrap();
        assert!(work.completed);
        assert!(!work.astatka_required);
        assert_eq!(work.last_apparatus, X);
    }
    pool.close().await;
    // A rejected concurrent transaction may still be closing its connection.
    // This name is generated above and belongs exclusively to this test.
    sqlx::query(&format!("DROP DATABASE {name} WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
