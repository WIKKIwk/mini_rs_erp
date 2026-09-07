use std::sync::Arc;

use crate::core::production_map::{
    ProductionMapError, ProductionMapService, ProductionMapStorePort, QueueActionActor,
    QueueProgressInput, queue_state::ApparatusQueueAction as Action,
};
use crate::db::postgres::{apply_foundation_migration, postgres_test_database_options};
use crate::db::postgres_production_map::PostgresProductionMapStore;

#[tokio::test]
async fn rezka_outputs_and_paddon_membership_commit_atomically() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
    let admin = sqlx::PgPool::connect(&admin_url)
        .await
        .expect("local test PostgreSQL");
    let db_name = format!(
        "mini_rs_erp_test_rezka_paddon_{:016x}",
        rand::random::<u64>()
    );
    sqlx::query(&format!("CREATE DATABASE {db_name}"))
        .execute(&admin)
        .await
        .unwrap();
    let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&admin_url, &db_name))
        .await
        .unwrap();
    apply_foundation_migration(&pool).await.unwrap();
    // Exercise the receipt guard without requiring the separate receipt
    // feature's migration to be part of this change.
    sqlx::query("ALTER TABLE mini_paddons ADD COLUMN IF NOT EXISTS receipt_json JSONB")
        .execute(&pool)
        .await
        .unwrap();
    super::seed_standard_canonical_apparatus(&pool).await;
    let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
    let service = ProductionMapService::new_for_test(store.clone());
    let apparatus = "apparatus:default:asset-010";
    let actor = QueueActionActor {
        role: "aparatchi".into(),
        ref_: "rezka-paddon-worker".into(),
        display_name: "Rezka".into(),
    };
    let paddon_a = service.create_paddon("", "A", &actor).await.unwrap();
    let paddon_b = service.create_paddon("", "B", &actor).await.unwrap();

    for intermediate in [false, true] {
        let order = if intermediate {
            "zakaz-paddon-intermediate"
        } else {
            "zakaz-paddon-final"
        };
        let mut map = serde_json::json!({
            "id":order,"product_code":order,"title":order,"order_number":if intermediate {"9522"}else{"9521"},
            "nodes":[{"id":"start","kind":"start","title":"Start"},
                {"id":"rezka","kind":"apparatus","title":"Rezka","apparatus_id":apparatus,"rezka_kadr_count":3,"rezka_frame_groups":[1,2]},
                {"id":"end","kind":"end","title":"End"}],
            "edges":[{"from":"start","to":"rezka"},{"from":"rezka","to":"end"}]
        });
        if intermediate {
            map["nodes"].as_array_mut().unwrap().insert(2,serde_json::json!({"id":"lam","kind":"apparatus","title":"Laminatsiya","apparatus_id":"apparatus:default:asset-007"}));
            map["edges"][1]["to"] = serde_json::json!("lam");
            map["edges"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({"from":"lam","to":"end"}));
        }
        service
            .upsert_map(serde_json::from_value(map).unwrap())
            .await
            .unwrap();
        let start = service
            .apply_apparatus_queue_action_with_progress(
                apparatus,
                order,
                Action::Start,
                &[apparatus.into()],
                actor.clone(),
                QueueProgressInput::default(),
            )
            .await
            .unwrap();
        let cycle = start.session.unwrap().session_id;
        let frame = serde_json::from_value(serde_json::json!({"produced_qty":120.0,"gross_qty":12.0,"bobina_kg":0.5,"diameter":45.0})).unwrap();
        let single = QueueProgressInput {
            rezka_record_frame_index: Some(1),
            rezka_output_cycle: cycle.clone(),
            rezka_frames: vec![frame],
            ..Default::default()
        };
        // A nonexistent pallet fails the entire write, including the QR and session.
        let mut invalid = service
            .prepare_apparatus_queue_action_with_progress(
                apparatus,
                order,
                Action::RollComplete,
                &[apparatus.into()],
                actor.clone(),
                single.clone(),
            )
            .await
            .unwrap();
        invalid.attach_output_paddon("missing-paddon");
        let rejected = service.commit_prepared_queue_action(invalid).await;
        assert!(
            matches!(rejected, Err(ProductionMapError::PaddonNotFound)),
            "{rejected:?}"
        );
        assert!(
            store
                .progress_batches_for_order(order)
                .await
                .unwrap()
                .is_empty()
        );
        let mut prepared = service
            .prepare_apparatus_queue_action_with_progress(
                apparatus,
                order,
                Action::RollComplete,
                &[apparatus.into()],
                actor.clone(),
                single.clone(),
            )
            .await
            .unwrap();
        prepared.attach_output_paddon(&paddon_a.code);
        let saved = service
            .commit_prepared_queue_action(prepared)
            .await
            .unwrap();
        let batch_id = saved.progress_batch.as_ref().unwrap().batch_id.clone();
        let mut retry = service
            .prepare_apparatus_queue_action_with_progress(
                apparatus,
                order,
                Action::RollComplete,
                &[apparatus.into()],
                actor.clone(),
                single.clone(),
            )
            .await
            .unwrap();
        retry.attach_output_paddon(&paddon_b.code);
        service.commit_prepared_queue_action(retry).await.unwrap();
        let membership: String = sqlx::query_scalar("SELECT paddon_id FROM mini_paddon_items WHERE progress_batch_id=$1 AND removed_at IS NULL").bind(&batch_id).fetch_one(&pool).await.unwrap();
        assert_eq!(
            membership, paddon_a.id,
            "replay must not move a previously printed roll"
        );

        let issue =
            serde_json::from_value(serde_json::json!({"issue_note":"Kadr yirtilgan"})).unwrap();
        let issue_input = QueueProgressInput {
            rezka_record_frame_index: Some(2),
            rezka_output_cycle: cycle.clone(),
            rezka_frames: vec![issue],
            ..Default::default()
        };
        let mut issue_write = service
            .prepare_apparatus_queue_action_with_progress(
                apparatus,
                order,
                Action::RollComplete,
                &[apparatus.into()],
                actor.clone(),
                issue_input.clone(),
            )
            .await
            .unwrap();
        issue_write.attach_output_paddon("missing-paddon");
        assert!(issue_write.progress_output_batches().is_empty());
        service
            .commit_prepared_queue_action(issue_write)
            .await
            .unwrap();
        let mut frames = vec![
            single.rezka_frames[0].clone(),
            issue_input.rezka_frames[0].clone(),
        ];
        if !intermediate {
            frames.push(single.rezka_frames[0].clone());
        }
        let completion = QueueProgressInput {
            rezka_output_cycle: cycle.clone(),
            rezka_frames: frames,
            total_waste: Some(1.5),
            ..Default::default()
        };
        let action = if intermediate {
            Action::DetachRoll
        } else {
            Action::Complete
        };
        if !intermediate {
            sqlx::query("UPDATE mini_paddons SET receipt_json='{}'::jsonb WHERE id=$1")
                .bind(&paddon_b.id)
                .execute(&pool)
                .await
                .unwrap();
            let mut closed = service
                .prepare_apparatus_queue_action_with_progress(
                    apparatus,
                    order,
                    action,
                    &[apparatus.into()],
                    actor.clone(),
                    completion.clone(),
                )
                .await
                .unwrap();
            closed.attach_output_paddon(&paddon_b.code);
            assert!(matches!(
                service.commit_prepared_queue_action(closed).await,
                Err(ProductionMapError::PaddonInvalidInput)
            ));
            assert_eq!(
                store.progress_batches_for_order(order).await.unwrap().len(),
                1
            );
            sqlx::query("UPDATE mini_paddons SET receipt_json=NULL WHERE id=$1")
                .bind(&paddon_b.id)
                .execute(&pool)
                .await
                .unwrap();
        }
        let mut finish = service
            .prepare_apparatus_queue_action_with_progress(
                apparatus,
                order,
                action,
                &[apparatus.into()],
                actor.clone(),
                completion,
            )
            .await
            .unwrap();
        finish.attach_output_paddon(&paddon_b.code);
        service.commit_prepared_queue_action(finish).await.unwrap();
        let batches = store.progress_batches_for_order(order).await.unwrap();
        assert_eq!(batches.len(), if intermediate { 1 } else { 2 });
        for batch in batches {
            let id: String = sqlx::query_scalar("SELECT paddon_id FROM mini_paddon_items WHERE progress_batch_id=$1 AND removed_at IS NULL").bind(&batch.batch_id).fetch_one(&pool).await.unwrap();
            assert_eq!(
                id,
                if batch.batch_id == batch_id {
                    paddon_a.id.clone()
                } else {
                    paddon_b.id.clone()
                }
            );
        }
        if intermediate {
            let resumed = service
                .apply_apparatus_queue_action_with_progress(
                    apparatus,
                    order,
                    Action::Resume,
                    &[apparatus.into()],
                    actor.clone(),
                    QueueProgressInput::default(),
                )
                .await
                .unwrap();
            let next_cycle = resumed.session.unwrap().payload_json["rezka_output_cycle"]
                .as_str()
                .unwrap()
                .to_string();
            let mut bulk = service
                .prepare_apparatus_queue_action_with_progress(
                    apparatus,
                    order,
                    Action::DetachRoll,
                    &[apparatus.into()],
                    actor.clone(),
                    QueueProgressInput {
                        rezka_output_cycle: next_cycle,
                        rezka_frames: vec![
                            single.rezka_frames[0].clone(),
                            single.rezka_frames[0].clone(),
                        ],
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            bulk.attach_output_paddon(&paddon_b.code);
            let detached = service.commit_prepared_queue_action(bulk).await.unwrap();
            assert_eq!(detached.progress_batches.len(), 2);
            assert_eq!(
                detached.progress_batches[1].payload_json["contained_kadr_count"],
                2
            );
            for batch in detached.progress_batches {
                let id: String = sqlx::query_scalar("SELECT paddon_id FROM mini_paddon_items WHERE progress_batch_id=$1 AND removed_at IS NULL")
                    .bind(&batch.batch_id).fetch_one(&pool).await.unwrap();
                assert_eq!(id, paddon_b.id);
            }
        }
    }
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {db_name}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
