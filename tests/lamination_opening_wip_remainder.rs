#![cfg(feature = "verification")]

use mini_rs_erp::{
    core::production_map::{queue_state::ApparatusQueueAction as Action, *},
    db::{postgres, postgres_production_map::PostgresProductionMapStore},
};
use serde_json::json;
use sqlx::{PgPool, postgres::PgConnectOptions};
use std::sync::Arc;

const PRESS: &str = "apparatus:default:bosma_8";
const LAM: &str = "apparatus:default:asset-007";
const CUT: &str = "apparatus:default:asset-010";

fn actor() -> QueueActionActor {
    QueueActionActor {
        role: "aparatchi".into(),
        ref_: "opening-laminator".into(),
        display_name: "Laminator".into(),
    }
}

#[tokio::test]
#[ignore = "requires disposable MINI_ERP_LAYER_REMAINDER_TEST_DATABASE_URL"]
async fn opening_remainders_keep_their_passport_and_scan_as_opening_wip() {
    let url = std::env::var("MINI_ERP_LAYER_REMAINDER_TEST_DATABASE_URL").unwrap();
    let admin = PgPool::connect(&url).await.unwrap();
    let database = format!("opening_remainders_{:032x}", rand::random::<u128>());
    sqlx::query(&format!("CREATE DATABASE {database}"))
        .execute(&admin)
        .await
        .unwrap();
    let pool = PgPool::connect_with(url.parse::<PgConnectOptions>().unwrap().database(&database))
        .await
        .unwrap();
    let checks_pool = pool.clone();
    let result = tokio::task::LocalSet::new()
        .run_until(async move {
            tokio::task::spawn_local(async move { checks(checks_pool).await }).await
        })
        .await;
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {database} WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    result.unwrap();
}

async fn report(
    service: &ProductionMapService,
    store: &PostgresProductionMapStore,
    order: &str,
    action: Action,
    kg: f64,
) -> (LaminatsiyaAstatkaShiftInput, LaminatsiyaAstatkaCommitResult) {
    let snapshot = service
        .laminatsiya_astatka_snapshot(LAM, order, &actor().ref_, "")
        .await
        .unwrap();
    assert_eq!(snapshot.sources.len(), 1);
    let source = &snapshot.sources[0];
    let input = LaminatsiyaAstatkaShiftInput {
        action,
        full_completion_report_required: action == Action::Complete,
        apparatus: LAM.into(),
        order_id: order.into(),
        session_id: snapshot.session.unwrap().session_id,
        request_id: format!("opening-return-{:032x}", rand::random::<u128>()),
        lamination_print_leftover_rolls: kg,
        lamination_film_leftover_rolls: 0.0,
        total_waste: 0.0,
        finished_goods_meter: Some(100.0),
        finished_goods_kg: Some(10.0),
        bobina_kg: Some(1.0),
        description: "Opening roll residual".into(),
        layer_remainders: vec![],
        layer_remainder_weights: Some(LaminationRemainderWeights {
            wip_net_kg: kg,
            film_net_kg: 0.0,
            wip_source_id: None,
            film_source_id: None,
            expected_sources: vec![LaminationRemainderSourceRevision {
                source_kind: source.source_kind.clone(),
                source_id: source.source_id.clone(),
                expected_revision: source.revision,
            }],
        }),
    };
    let before = store.progress_batches_for_order(order).await.unwrap();
    let mut stale = input.clone();
    stale
        .layer_remainder_weights
        .as_mut()
        .unwrap()
        .expected_sources[0]
        .expected_revision += 1;
    assert_eq!(
        service
            .record_laminatsiya_astatka_shift(stale, actor(), &[LAM.into()])
            .await
            .unwrap_err(),
        ProductionMapError::ProgressBatchCorrectionConflict
    );
    let mut excessive = input.clone();
    excessive
        .layer_remainder_weights
        .as_mut()
        .unwrap()
        .wip_net_kg = source.source_net_kg + 1.0;
    assert!(
        service
            .record_laminatsiya_astatka_shift(excessive, actor(), &[LAM.into()])
            .await
            .is_err()
    );
    assert_eq!(
        store.progress_batches_for_order(order).await.unwrap(),
        before
    );
    let result = service
        .record_laminatsiya_astatka_shift(input.clone(), actor(), &[LAM.into()])
        .await
        .unwrap();
    assert_eq!(result.progress_batches.len(), 1);
    assert_eq!(result.progress_batches[0].produced_qty, 100.0);
    assert_eq!(result.remainders.len(), 1);
    assert_eq!(result.remainders[0].source_id, source.source_id);
    assert_eq!(result.remainders[0].remaining_net_kg, kg);
    let after = store.progress_batches_for_order(order).await.unwrap();
    assert_eq!(
        service
            .record_laminatsiya_astatka_shift(input.clone(), actor(), &[LAM.into()])
            .await
            .unwrap(),
        result
    );
    assert_eq!(
        store.progress_batches_for_order(order).await.unwrap(),
        after
    );
    (input, result)
}

async fn scan(
    service: &ProductionMapService,
    order: &str,
    qr: &str,
) -> Result<(), ProductionMapError> {
    let controls = service.queue_action_controls().await?;
    let control = &controls[LAM][order];
    let action = if control.allowed_actions.contains(&Action::Resume) {
        Action::Resume
    } else {
        Action::Start
    };
    service
        .apply_apparatus_queue_action_with_progress(
            LAM,
            order,
            action,
            &[LAM.into()],
            actor(),
            QueueProgressInput {
                qr_payload: qr.into(),
                ..Default::default()
            },
        )
        .await?;
    Ok(())
}

async fn checks(pool: PgPool) {
    postgres::apply_postgres_migrations_through_version(&pool, "0121")
        .await
        .unwrap();
    let canonical = postgres::canonical_apparatus_service(pool.clone());
    canonical.bootstrap_factory_defaults().await.unwrap();
    postgres::apply_foundation_migration(&pool).await.unwrap();
    let owner = ProductionMapService::new(
        Arc::new(PostgresProductionMapStore::new(pool.clone())),
        Arc::new(CanonicalServiceApparatusResolver::new(canonical.clone())),
    );
    let runtime = sqlx::postgres::PgPoolOptions::new()
        .after_connect(|conn, _| {
            Box::pin(async move {
                sqlx::query("SET ROLE mini_rs_erp").execute(conn).await?;
                Ok(())
            })
        })
        .connect_with((*pool.connect_options()).clone())
        .await
        .unwrap();
    let store = Arc::new(PostgresProductionMapStore::new(runtime.clone()));
    let service = ProductionMapService::new(
        store.clone(),
        Arc::new(CanonicalServiceApparatusResolver::new(canonical)),
    );
    for legacy in [false, true] {
        for action in [Action::Pause, Action::Complete] {
            for kg in [0.0, 20.0] {
                let order = format!("zakaz-opening-return-{:032x}", rand::random::<u128>());
                owner
                    .upsert_map(
                        serde_json::from_value(json!({
                            "id":order,"title":"Printed PET","product_code":"PET","order_number":order,
                            "customer_name":"Original Customer",
                            "nodes":[{"id":"s","kind":"start","title":"Start"},
                                {"id":"press","kind":"apparatus","title":"Print","apparatus_id":PRESS},
                                {"id":"lam","kind":"apparatus","title":"Lamination","apparatus_id":LAM},
                                {"id":"cut","kind":"apparatus","title":"Cut","apparatus_id":CUT},
                                {"id":"e","kind":"end","title":"End"}],
                            "edges":[{"from":"s","to":"press"},{"from":"press","to":"lam"},
                                {"from":"lam","to":"cut"},{"from":"cut","to":"e"}]
                        }))
                        .unwrap(),
                    )
                    .await
                    .unwrap();
                let original = owner
                    .create_opening_wip(
                        OpeningWipCreateInput {
                            idempotency_key: format!("original-{order}"),
                            order_id: order.clone(),
                            entry_apparatus: if legacy { PRESS.into() } else { String::new() },
                            source_operation: "print".into(),
                            source_apparatus: if legacy { String::new() } else { PRESS.into() },
                            source_stage_node_id: if legacy {
                                String::new()
                            } else {
                                "press".into()
                            },
                            current_location: if legacy { LAM.into() } else { String::new() },
                            note: "Original passport".into(),
                            batches: vec![OpeningWipBatchInput {
                                quantity_basis: OpeningWipQuantityBasis::Measured,
                                finished_goods_meter: Some(1500.0),
                                finished_goods_kg: Some(52.0),
                                bobina_kg: Some(2.0),
                                diameter: None,
                            }],
                        },
                        QueueActionActor {
                            role: "admin".into(),
                            ref_: "original-owner".into(),
                            display_name: "Original Owner".into(),
                        },
                    )
                    .await
                    .unwrap();
                owner
                    .set_apparatus_sequence(LAM, vec![order.clone()])
                    .await
                    .unwrap();
                owner
                    .update_lamination_label_settings(
                        LaminationLabelSettingsPatch {
                            print_wip_remainder_labels: Some(action == Action::Complete),
                            print_film_remainder_labels: Some(false),
                        },
                        &QueueActionActor {
                            role: "admin".into(),
                            ref_: "admin".into(),
                            display_name: "Admin".into(),
                        },
                    )
                    .await
                    .unwrap();
                let original_qr = &original.batches[0].qr_payload;
                scan(&service, &order, original_qr).await.unwrap();
                let (request, result) = report(&service, &store, &order, action, kg).await;
                assert_eq!(
                    result.session.status,
                    if action == Action::Pause {
                        OrderRunStatus::Paused
                    } else {
                        OrderRunStatus::Completed
                    }
                );
                let receipt = &result.remainders[0];
                assert_eq!(receipt.source_kind, "opening_wip");
                assert!(receipt.progress_batch.is_none());
                if action == Action::Complete {
                    assert_eq!(
                        store.apparatus_queue_states().await.unwrap()[LAM][&order],
                        if kg > 0.0 { "pending" } else { "completed" }
                    );
                    assert_eq!(
                        result
                            .session
                            .payload_json
                            .get("stage_work_report")
                            .is_some(),
                        kg == 0.0
                    );
                }
                let parent = service.opening_wip_batch("", original_qr).await.unwrap();
                assert_eq!(parent.batch.wip_status, OpeningWipBatchStatus::Processed);
                assert_eq!(parent.batch.finished_goods_kg, Some(52.0));
                assert_eq!(parent.batch.finished_goods_meter, Some(1500.0));
                assert_eq!(parent.intake, original.intake);
                if kg == 0.0 {
                    assert!(receipt.opening_wip.is_none());
                    assert!(receipt.qr_payload.is_empty());
                } else {
                    let child = receipt.opening_wip.as_ref().unwrap();
                    assert_eq!(json!(child.intake), json!(original.intake));
                    assert_eq!(child.batch.parent_batch_id, parent.batch.batch_id);
                    assert_eq!(child.batch.intake_id, parent.batch.intake_id);
                    assert_eq!(child.batch.finished_goods_kg, Some(22.0));
                    assert_eq!(child.batch.bobina_kg, Some(2.0));
                    assert_eq!(child.batch.finished_goods_meter, Some(600.0));
                    assert_eq!(child.batch.label_item_code, "PET");
                    assert_eq!(child.batch.wip_status, OpeningWipBatchStatus::Waiting);
                    assert_eq!(receipt.producer_name, "Original Owner");
                    assert!(scan(&service, &order, original_qr).await.is_err());
                    assert!(
                        sqlx::query(
                            "UPDATE mini_opening_wip_batches SET wip_status='waiting' WHERE batch_id=$1"
                        )
                        .bind(&parent.batch.batch_id)
                        .execute(&runtime)
                        .await
                        .is_err()
                    );
                    scan(&service, &order, &child.batch.qr_payload)
                        .await
                        .unwrap();
                    let reloaded = service
                        .opening_wip_batch("", &child.batch.qr_payload)
                        .await
                        .unwrap();
                    assert_eq!(reloaded.batch.wip_status, OpeningWipBatchStatus::InUse);
                    assert_eq!(reloaded.batch.used_by_apparatus, LAM);
                    let (_, again) = report(&service, &store, &order, Action::Pause, 10.0).await;
                    let grandchild = again.remainders[0].opening_wip.as_ref().unwrap();
                    assert_eq!(grandchild.batch.parent_batch_id, child.batch.batch_id);
                    assert_eq!(grandchild.intake, child.intake);
                    assert_eq!(grandchild.batch.finished_goods_meter, Some(300.0));
                    scan(&service, &order, &grandchild.batch.qr_payload)
                        .await
                        .unwrap();
                    let (_, exhausted) =
                        report(&service, &store, &order, Action::Complete, 0.0).await;
                    assert!(exhausted.remainders[0].opening_wip.is_none());
                    assert_eq!(
                        store.apparatus_queue_states().await.unwrap()[LAM][&order],
                        "completed"
                    );
                    assert_eq!(
                        service
                            .record_laminatsiya_astatka_shift(request, actor(), &[LAM.into()])
                            .await
                            .unwrap(),
                        result
                    );
                    assert_eq!(
                        service
                            .layer_remainder_history(LAM, &order)
                            .await
                            .unwrap()
                            .len(),
                        3
                    );
                }
                // Release a zero-remainder paused fixture before the next independent case.
                sqlx::query("UPDATE mini_order_run_sessions SET status='completed' WHERE order_id=$1 AND status='paused'")
                    .bind(&order).execute(&pool).await.unwrap();
                sqlx::query("UPDATE mini_queue_states SET state='completed' WHERE order_id=$1 AND canonical_apparatus_id=$2")
                    .bind(&order).bind(LAM).execute(&pool).await.unwrap();
            }
        }
    }
    printed_remainder_completion(&owner, &service, &store).await;
    runtime.close().await;
}

async fn printed_remainder_completion(
    owner: &ProductionMapService,
    service: &ProductionMapService,
    store: &PostgresProductionMapStore,
) {
    const ORDER: &str = "zakaz-printed-return-completion";
    owner.upsert_map(serde_json::from_value(json!({
        "id": ORDER, "product_code": "PET", "title": "Printed roll completion",
        "nodes": [
            {"id":"s","kind":"start","title":"Start"},
            {"id":"press","kind":"apparatus","title":"Print","apparatus_id":PRESS},
            {"id":"lam","kind":"apparatus","title":"Lamination","apparatus_id":LAM},
            {"id":"e","kind":"end","title":"End"}
        ],
        "edges":[{"from":"s","to":"press"},{"from":"press","to":"lam"},{"from":"lam","to":"e"}]
    })).unwrap()).await.unwrap();
    owner
        .set_apparatus_sequence(LAM, vec![ORDER.into()])
        .await
        .unwrap();
    store
        .put_apparatus_queue_states(
            PRESS,
            std::collections::BTreeMap::from([(ORDER.into(), "completed".into())]),
        )
        .await
        .unwrap();
    let printed: OrderProgressBatch = serde_json::from_value(json!({
        "batch_id": "printed-original", "session_id": "print-session", "order_id": ORDER,
        "started_at_unix": 1, "completed_at_unix": 2, "apparatus": PRESS,
        "action": "complete", "status": "completed", "produced_qty": 1500.0, "uom": "m",
        "qr_payload": "40010000000000000000AA01", "label_item_code": "PET",
        "label_item_name": "Printed PET", "executor_name": "Original Printer",
        "worker_role": "aparatchi", "worker_ref": "printer", "worker_display_name": "Printer",
        "wip_status": "waiting", "next_apparatus": LAM, "finished_goods_kg": 52.0,
        "bobina_kg": 2.0, "finished_goods_meter": 1500.0,
        "payload_json": {"stage_node_id":"press","next_stage_node_id":"lam","gross_qty":52.0}
    }))
    .unwrap();
    store
        .put_order_progress_batch(printed.clone())
        .await
        .unwrap();
    scan(service, ORDER, &printed.qr_payload).await.unwrap();

    let (_, partial) = report(service, store, ORDER, Action::Complete, 12.0).await;
    assert_eq!(partial.session.status, OrderRunStatus::Completed);
    assert!(
        partial
            .session
            .payload_json
            .get("stage_work_report")
            .is_none()
    );
    assert_eq!(
        store.apparatus_queue_states().await.unwrap()[LAM][ORDER],
        "pending"
    );
    assert_eq!(
        service
            .order_status_detail(ORDER)
            .await
            .unwrap()
            .lifecycle_status,
        ProductionOrderLifecycleStatus::InProgress
    );
    let history = service
        .completed_queue_orders_for_apparatuses(&[LAM.into()], 20)
        .await
        .unwrap();
    assert!(
        history.iter().any(|entry| entry.order_id == ORDER
            && entry.status == CompletedQueueOrderStatus::InProgress)
    );
    let returned = partial.remainders[0].progress_batch.as_ref().unwrap();
    assert_eq!(returned.wip_status, OrderProgressBatchWipStatus::Waiting);
    assert_eq!(returned.parent_batch_id, printed.batch_id);
    scan(service, ORDER, &returned.qr_payload).await.unwrap();

    let (_, final_report) = report(service, store, ORDER, Action::Complete, 0.0).await;
    assert!(
        final_report
            .session
            .payload_json
            .get("stage_work_report")
            .is_some()
    );
    assert_eq!(
        store.apparatus_queue_states().await.unwrap()[LAM][ORDER],
        "completed"
    );
    assert_eq!(
        service
            .order_status_detail(ORDER)
            .await
            .unwrap()
            .lifecycle_status,
        ProductionOrderLifecycleStatus::ProductionCompleted
    );
}
