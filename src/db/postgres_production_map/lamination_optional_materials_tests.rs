use super::*;
use crate::core::apparatus_standard::*;
use crate::core::production_map::*;
use crate::db::postgres::{
    apply_foundation_migration, apply_postgres_migrations_through_version,
    canonical_apparatus_service,
};
use sqlx::{PgPool, postgres::PgConnectOptions};
use std::sync::Arc;

const LAM: &str = "apparatus:default:asset-007";

fn actor() -> QueueActionActor {
    QueueActionActor {
        role: "aparatchi".into(),
        ref_: "optional-material-worker".into(),
        display_name: "Laminator".into(),
    }
}

#[tokio::test]
#[ignore = "requires disposable MINI_ERP_LAYER_REMAINDER_TEST_DATABASE_URL"]
async fn lamination_completion_respects_optional_material_policy() {
    let url = std::env::var("MINI_ERP_LAYER_REMAINDER_TEST_DATABASE_URL").unwrap();
    let admin = PgPool::connect(&url).await.unwrap();
    let name = format!("optional_lamination_{:032x}", rand::random::<u128>());
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&admin)
        .await
        .unwrap();
    let pool = PgPool::connect_with(url.parse::<PgConnectOptions>().unwrap().database(&name))
        .await
        .unwrap();
    let checks_pool = pool.clone();
    let result = tokio::task::LocalSet::new()
        .run_until(async move {
            tokio::task::spawn_local(async move { checks(checks_pool).await }).await
        })
        .await;
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {name} WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    result.unwrap();
}

async fn checks(pool: PgPool) {
    sqlx::raw_sql("DO $$ BEGIN IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='mini_rs_erp') THEN CREATE ROLE mini_rs_erp NOLOGIN; END IF; END $$;")
        .execute(&pool).await.unwrap();
    apply_postgres_migrations_through_version(&pool, "0121")
        .await
        .unwrap();
    let canonical = canonical_apparatus_service(pool.clone());
    canonical.bootstrap_factory_defaults().await.unwrap();
    apply_foundation_migration(&pool).await.unwrap();
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
        Arc::new(CanonicalServiceApparatusResolver::new(canonical.clone())),
    );

    // Both the bare factory default and an explicitly optional material rule.
    for (index, groups) in [vec![], vec!["All Item Groups".into()]]
        .into_iter()
        .enumerate()
    {
        let id = ApparatusId::new(LAM).unwrap();
        let current = canonical.current_configuration(&id).await.unwrap().unwrap();
        canonical
            .patch(
                id,
                current.runtime.source_revision,
                CanonicalApparatusPatch {
                    policies: Some(ApparatusOperationalPolicies {
                        queue: current.queue.discipline,
                        material: MaterialExecutionPolicy::NotRequired {
                            item_group_ids: groups,
                        },
                        tooling: current.material.tooling.clone(),
                    }),
                    ..Default::default()
                },
                CanonicalCommandMetadata::new("regression", format!("optional-materials-{index}")),
            )
            .await
            .unwrap();
        let order_id = format!("zakaz-optional-lamination-{index}");
        service
            .upsert_map(
                serde_json::from_value(json!({
                    "id":order_id,"product_code":"FILM","title":"Optional lamination",
                    "nodes":[{"id":"s","kind":"start","title":"Start"},
                        {"id":"lam","kind":"apparatus","title":"Lamination","apparatus_id":LAM},
                        {"id":"e","kind":"end","title":"End"}],
                    "edges":[{"from":"s","to":"lam"},{"from":"lam","to":"e"}]
                }))
                .unwrap(),
            )
            .await
            .unwrap();
        service
            .set_apparatus_sequence(LAM, vec![order_id.clone()])
            .await
            .unwrap();
        let controls = service.queue_action_controls().await.unwrap();
        assert!(
            controls[LAM][&order_id]
                .allowed_actions
                .contains(&queue_state::ApparatusQueueAction::Start)
        );
        assert!(!controls[LAM][&order_id].interaction.material_scan_required);
        service
            .apply_apparatus_queue_action_with_material_scan_and_progress(
                MaterialScanProgressAction {
                    apparatus: LAM,
                    order_id: &order_id,
                    action: queue_state::ApparatusQueueAction::Start,
                    assigned_apparatus: &[LAM.into()],
                    actor: actor(),
                    material_barcodes: &[],
                    state_material_barcodes: &[],
                    progress: QueueProgressInput::default(),
                    qolip_validation: None,
                },
            )
            .await
            .unwrap();
        // Reproduce the report's four existing output rolls without inventing inputs.
        for meters in [7020.0, 7060.0, 7070.0, 7080.0] {
            for (action, progress) in [
                (
                    queue_state::ApparatusQueueAction::DetachRoll,
                    QueueProgressInput {
                        finished_goods_meter: Some(meters),
                        finished_goods_kg: Some(100.0),
                        bobina_kg: Some(1.0),
                        ..Default::default()
                    },
                ),
                (
                    queue_state::ApparatusQueueAction::Resume,
                    QueueProgressInput::default(),
                ),
            ] {
                service
                    .apply_apparatus_queue_action_with_material_scan_and_progress(
                        MaterialScanProgressAction {
                            apparatus: LAM,
                            order_id: &order_id,
                            action,
                            assigned_apparatus: &[LAM.into()],
                            actor: actor(),
                            material_barcodes: &[],
                            state_material_barcodes: &[],
                            progress,
                            qolip_validation: None,
                        },
                    )
                    .await
                    .unwrap();
            }
        }
        let snapshot = service
            .laminatsiya_astatka_snapshot(LAM, &order_id, &actor().ref_, "")
            .await
            .unwrap();
        assert!(snapshot.sources.is_empty());
        assert!(snapshot.allow_empty_sources);
        let session = snapshot.session.unwrap();
        let previous = store.progress_batches_for_order(&order_id).await.unwrap();
        assert_eq!(previous.len(), 4);
        assert_eq!(
            previous.iter().map(|b| b.produced_qty).sum::<f64>(),
            28230.0
        );
        let input = LaminatsiyaAstatkaShiftInput {
            action: queue_state::ApparatusQueueAction::Complete,
            full_completion_report_required: true,
            apparatus: LAM.into(),
            order_id: order_id.clone(),
            session_id: session.session_id.clone(),
            request_id: format!("optional-complete-{index}"),
            lamination_print_leftover_rolls: index as f64,
            lamination_film_leftover_rolls: index as f64,
            total_waste: index as f64,
            finished_goods_meter: Some(100.0),
            finished_goods_kg: Some(10.0),
            bobina_kg: Some(1.0),
            description: "Optional material completion".into(),
            layer_remainders: vec![],
        };
        // Required rules still reject an empty report, including a direct store call.
        let id = ApparatusId::new(LAM).unwrap();
        let current = canonical.current_configuration(&id).await.unwrap().unwrap();
        let optional_policies = ApparatusOperationalPolicies {
            queue: current.queue.discipline,
            material: current.material.policy.clone(),
            tooling: current.material.tooling.clone(),
        };
        canonical
            .patch(
                id.clone(),
                current.runtime.source_revision,
                CanonicalApparatusPatch {
                    policies: Some(ApparatusOperationalPolicies {
                        material: MaterialExecutionPolicy::AllRequired {
                            item_group_ids: vec!["All Item Groups".into()],
                        },
                        ..optional_policies.clone()
                    }),
                    ..Default::default()
                },
                CanonicalCommandMetadata::new("regression", format!("required-materials-{index}")),
            )
            .await
            .unwrap();
        assert!(
            !service
                .laminatsiya_astatka_snapshot(LAM, &order_id, &actor().ref_, "")
                .await
                .unwrap()
                .allow_empty_sources
        );
        assert_eq!(
            service
                .record_laminatsiya_astatka_shift(input.clone(), actor(), &[LAM.into()])
                .await
                .unwrap_err(),
            ProductionMapError::ProgressInputInvalid
        );
        assert_eq!(
            store
                .commit_laminatsiya_astatka(
                    input.request_id.clone(),
                    json!(input),
                    None,
                    session.clone(),
                    actor(),
                    None,
                    vec![]
                )
                .await
                .unwrap_err(),
            ProductionMapError::ProgressInputInvalid
        );
        assert_eq!(
            store.progress_batches_for_order(&order_id).await.unwrap(),
            previous
        );
        assert_eq!(
            store
                .active_order_run_session(LAM, &order_id)
                .await
                .unwrap()
                .unwrap(),
            session
        );
        let current = canonical.current_configuration(&id).await.unwrap().unwrap();
        canonical
            .patch(
                id,
                current.runtime.source_revision,
                CanonicalApparatusPatch {
                    policies: Some(optional_policies),
                    ..Default::default()
                },
                CanonicalCommandMetadata::new("regression", format!("restore-optional-{index}")),
            )
            .await
            .unwrap();
        let result = service
            .record_laminatsiya_astatka_shift(input.clone(), actor(), &[LAM.into()])
            .await
            .unwrap();
        assert_eq!(result.session.status, OrderRunStatus::Completed);
        assert_eq!(result.session.session_id, session.session_id);
        assert!(result.remainders.is_empty());
        assert_eq!(result.progress_batches.len(), 1);
        assert_eq!(
            service
                .record_laminatsiya_astatka_shift(input.clone(), actor(), &[LAM.into()])
                .await
                .unwrap(),
            result
        );
        let after = store.progress_batches_for_order(&order_id).await.unwrap();
        assert_eq!(after.len(), 5);
        for batch in previous {
            assert!(after.contains(&batch));
        }
        assert!(
            service
                .layer_remainder_history(LAM, &order_id)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            store
                .laminatsiya_astatka_reports_for_order(&order_id)
                .await
                .unwrap()
                .is_empty()
        );
        let receipt = service
            .laminatsiya_astatka_snapshot(LAM, &order_id, &actor().ref_, &input.request_id)
            .await
            .unwrap();
        assert_eq!(receipt.request_result, Some(result));
    }
    runtime.close().await;
}
