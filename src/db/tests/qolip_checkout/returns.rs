use super::*;
use crate::core::production_map::{
    ApparatusQueueActionEvent, ApparatusQueuePolicy, ProductionMapError, QueueActionActor,
    QueueActionProgressWrite,
    queue_state::{ApparatusQueueAction, ApparatusQueueOrderState},
};
use crate::core::production_map::{ProductionMapDefinition, ProductionQrSessionResources};
use crate::core::qolip::{QolipError, QolipStorePort};
use crate::db::postgres_qolip::PostgresQolipStore;
use serde_json::json;
use std::collections::BTreeMap;

const ORDER: &str = "order-qolip-completion";
const HEAD: &str = "order-before-qolip";
const PRINT: &str = "apparatus:default:bosma_7";
const LAM: &str = "apparatus:default:asset-007";

fn map(id: &str) -> ProductionMapDefinition {
    serde_json::from_value(json!({
        "id":id,"product_code":"ITEM-1","title":"Return regression",
        "nodes":[
            {"id":"start","kind":"start","title":"Start"},
            {"id":"bosma_7","kind":"apparatus","title":"Print","apparatus_id":PRINT},
            {"id":"lam","kind":"apparatus","title":"Lamination","apparatus_id":LAM},
            {"id":"end","kind":"end","title":"End"}
        ],
        "edges":[{"from":"start","to":"bosma_7"},{"from":"bosma_7","to":"lam"},{"from":"lam","to":"end"}]
    })).unwrap()
}

async fn seed(f: &Fixture, first: bool, queue: &str, status: &str) {
    for id in [ORDER, HEAD] {
        sqlx::query("INSERT INTO mini_production_maps(id,product_code,title,map_json) VALUES($1,'ITEM-1','Return regression',$2) ON CONFLICT(id) DO UPDATE SET map_json=excluded.map_json")
            .bind(id).bind(serde_json::to_value(map(id)).unwrap()).execute(&f.pool).await.unwrap();
    }
    sqlx::query("DELETE FROM mini_order_control_states")
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM mini_stage_order_controls")
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM mini_qolip_checkouts")
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM mini_qolip_locations")
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE mini_production_maps SET lifecycle_status='in_progress'")
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE mini_order_run_sessions SET canonical_apparatus_id=$1,status=$2,payload_json=$3 WHERE session_id='session-qolip-completion'")
        .bind(PRINT).bind(status).bind(session(OrderRunStatus::Paused).payload_json)
        .execute(&f.pool).await.unwrap();
    f.store
        .put_apparatus_queue_states(
            PRINT,
            BTreeMap::from([
                (HEAD.into(), "pending".into()),
                (ORDER.into(), queue.into()),
            ]),
        )
        .await
        .unwrap();
    f.store
        .put_apparatus_sequence(
            PRINT,
            if first {
                vec![ORDER.into(), HEAD.into()]
            } else {
                vec![HEAD.into(), ORDER.into()]
            },
        )
        .await
        .unwrap();
    insert_open_checkout(&f.pool, "checkout-return", "original-worker", "Q-SESSION").await;
}

async fn global_control(f: &Fixture, order: &str, state: &str) {
    sqlx::query("INSERT INTO mini_order_control_states(order_id,state,frozen_at_unix) VALUES($1,$2,CASE WHEN $2='frozen' THEN 1 ELSE NULL END)")
        .bind(order).bind(state).execute(&f.pool).await.unwrap();
}

#[tokio::test]
async fn physical_qolip_return_obeys_print_activity_and_effective_queue_head() {
    let f = Fixture::new().await;
    let qolip = PostgresQolipStore::new(f.pool.clone());
    // Includes stale session rows left by older freeze/close paths. Worker
    // identity changes must not prevent the Qolipchi receiving the physical loan.
    let cases = [
        ("active first", true, "in_progress", "active", "", false),
        ("active later", false, "in_progress", "active", "", false),
        ("paused first", true, "paused", "paused", "", false),
        ("paused later", false, "paused", "paused", "", true),
        ("detached first", true, "paused", "roll_detached", "", false),
        ("detached later", false, "paused", "roll_detached", "", true),
        ("frozen session first", true, "paused", "frozen", "", true),
        (
            "completed session",
            true,
            "completed",
            "completed",
            "",
            true,
        ),
        (
            "completed queue stale owner",
            true,
            "completed",
            "active",
            "",
            true,
        ),
        (
            "frozen queue stale owner",
            true,
            "frozen",
            "paused",
            "",
            true,
        ),
        (
            "pending queue stale owner",
            true,
            "pending",
            "paused",
            "",
            true,
        ),
        (
            "paused queue stale active",
            false,
            "paused",
            "active",
            "",
            true,
        ),
        (
            "global freeze stale active",
            true,
            "in_progress",
            "active",
            "frozen",
            true,
        ),
        (
            "global freeze stale pause",
            true,
            "paused",
            "paused",
            "frozen",
            true,
        ),
        (
            "freeze requested active",
            false,
            "in_progress",
            "active",
            "freeze_requested",
            false,
        ),
        (
            "freeze requested pause head",
            true,
            "paused",
            "paused",
            "freeze_requested",
            false,
        ),
        (
            "freeze requested pause later",
            false,
            "paused",
            "paused",
            "freeze_requested",
            true,
        ),
        (
            "closed stale active",
            true,
            "in_progress",
            "active",
            "closed",
            true,
        ),
        (
            "cancelled stale pause",
            true,
            "paused",
            "paused",
            "cancelled",
            true,
        ),
        (
            "production completed stale pause",
            true,
            "paused",
            "paused",
            "production_completed",
            true,
        ),
        (
            "frozen head skipped",
            false,
            "paused",
            "paused",
            "head_frozen",
            false,
        ),
        (
            "completed head skipped",
            false,
            "paused",
            "paused",
            "head_completed",
            false,
        ),
        (
            "closed head skipped",
            false,
            "paused",
            "paused",
            "head_closed",
            false,
        ),
        (
            "unselected print alternative skipped",
            false,
            "paused",
            "paused",
            "head_other_print",
            false,
        ),
        (
            "local print freeze",
            true,
            "paused",
            "paused",
            "local_frozen",
            true,
        ),
        (
            "downstream active lineage",
            true,
            "in_progress",
            "active",
            "lamination",
            true,
        ),
        (
            "active legacy without queue row",
            false,
            "paused",
            "active",
            "missing_queue",
            false,
        ),
        (
            "paused legacy first without queue row",
            true,
            "paused",
            "paused",
            "missing_queue",
            false,
        ),
        (
            "paused legacy later without queue row",
            false,
            "paused",
            "paused",
            "missing_queue",
            true,
        ),
        (
            "frozen historical apparatus without runtime projection",
            true,
            "paused",
            "frozen",
            "missing_projection",
            true,
        ),
    ];
    let mut failures = Vec::new();
    for (name, first, queue, status, control, allowed) in cases {
        seed(&f, first, queue, status).await;
        match control {
            "frozen" | "freeze_requested" => global_control(&f, ORDER, control).await,
            "head_frozen" => global_control(&f, HEAD, "frozen").await,
            "head_completed" => {
                f.store
                    .put_apparatus_queue_states(
                        PRINT,
                        BTreeMap::from([
                            (HEAD.into(), "completed".into()),
                            (ORDER.into(), "paused".into()),
                        ]),
                    )
                    .await
                    .unwrap();
            }
            "head_closed" => {
                sqlx::query("UPDATE mini_production_maps SET lifecycle_status='closed',production_completed_at=now(),closed_at=now() WHERE id=$1").bind(HEAD).execute(&f.pool).await.unwrap();
            }
            "head_other_print" => {
                let mut other = map(HEAD);
                other
                    .nodes
                    .iter_mut()
                    .find(|n| n.id == "bosma_7")
                    .unwrap()
                    .apparatus_id = "apparatus:default:bosma_8".into();
                f.store.put_map(other).await.unwrap();
            }
            "local_frozen" => {
                sqlx::query("INSERT INTO mini_stage_order_controls(order_id,canonical_apparatus_id,stage_node_id,record_json) VALUES($1,$2,'bosma_7',$3)")
                    .bind(ORDER).bind(PRINT).bind(json!({"order_id":ORDER,"stage_node_id":"bosma_7","state":"frozen","freeze_request":{"target_apparatus":PRINT}})).execute(&f.pool).await.unwrap();
            }
            "lamination" => {
                sqlx::query("UPDATE mini_order_run_sessions SET canonical_apparatus_id=$1,stage_node_id='lam'").bind(LAM).execute(&f.pool).await.unwrap();
            }
            "missing_queue" => {
                sqlx::query(
                    "DELETE FROM mini_queue_states WHERE order_id=$1 AND canonical_apparatus_id=$2",
                )
                .bind(ORDER)
                .bind(PRINT)
                .execute(&f.pool)
                .await
                .unwrap();
            }
            "missing_projection" => {
                // Reproduce a historical partial projection in this disposable
                // database; ordinary runtime writes cannot create this state.
                let mut tx = f.pool.begin().await.unwrap();
                sqlx::query("SELECT set_config('mini_rs_erp.canonical_writer','on',true)")
                    .execute(&mut *tx)
                    .await
                    .unwrap();
                sqlx::query(
                    "DELETE FROM mini_apparatus_queue_policies WHERE canonical_apparatus_id=$1",
                )
                .bind(PRINT)
                .execute(&mut *tx)
                .await
                .unwrap();
                tx.commit().await.unwrap();
            }
            "closed" | "cancelled" | "production_completed" => {
                sqlx::query("UPDATE mini_production_maps SET lifecycle_status=$2,production_completed_at=now(),closed_at=now() WHERE id=$1").bind(ORDER).bind(control).execute(&f.pool).await.unwrap();
            }
            _ => {}
        }
        let returned = qolip.return_checkout("checkout-return", "A", Some(1)).await;
        if if allowed {
            returned.is_err()
        } else {
            returned != Err(QolipError::QolipInUse)
        } {
            failures.push(format!("{name}: {returned:?}"));
        }
        let saved = f
            .store
            .order_run_session("session-qolip-completion")
            .await
            .unwrap()
            .unwrap();
        let debt = checkout_status(&f.pool, "checkout-return").await;
        if allowed && returned.is_ok() {
            if (status != "completed"
                && (saved.payload_json["qolip_lock_owner"] != false
                    || !saved.qolip_reacquisition_required()))
                || saved.payload_json["qolip_codes"]
                    != session(OrderRunStatus::Paused).payload_json["qolip_codes"]
                || debt != "returned"
            {
                failures.push(format!(
                    "{name}: debt/lineage/reacquisition changed incorrectly"
                ));
            }
            let resources = ProductionQrSessionResources::for_session(&saved);
            if status != "completed"
                && (resources.qolip_available || !resources.qolip_codes.is_empty())
            {
                failures.push(format!("{name}: returned tooling remains mounted"));
            }
        } else if debt != "open" || saved.payload_json["qolip_lock_owner"] != true {
            failures.push(format!("{name}: rejected return changed debt or owner"));
        }
    }
    f.cleanup().await;
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[tokio::test]
async fn physical_qolip_return_rolls_back_stock_failure_and_never_returns_twice() {
    let f = Fixture::new().await;
    seed(&f, false, "paused", "paused").await;
    let qolip = PostgresQolipStore::new(f.pool.clone());
    sqlx::query("INSERT INTO mini_qolip_locations(id,block,warehouse,item_code,item_name,qolip_code,size,quantity,row_letter,column_number,location_label) SELECT location_id,block,warehouse,item_code,item_name,'DIFFERENT-MOLD',size,1,row_letter,column_number,location_label FROM mini_qolip_checkouts")
        .execute(&f.pool).await.unwrap();
    let failed = qolip.return_checkout("checkout-return", "A", Some(1)).await;
    let unchanged = f
        .store
        .order_run_session("session-qolip-completion")
        .await
        .unwrap()
        .unwrap();
    let open = checkout_status(&f.pool, "checkout-return").await;
    sqlx::query("DELETE FROM mini_qolip_locations")
        .execute(&f.pool)
        .await
        .unwrap();
    let returned = qolip.return_checkout("checkout-return", "A", Some(1)).await;
    let replay = qolip.return_checkout("checkout-return", "A", Some(1)).await;
    let quantity: i32 = sqlx::query_scalar(
        "SELECT quantity FROM mini_qolip_locations WHERE qolip_code='Q-SESSION'",
    )
    .fetch_one(&f.pool)
    .await
    .unwrap();
    f.cleanup().await;
    assert_eq!(failed, Err(QolipError::LocationIdentityMismatch));
    assert_eq!(open, "open");
    assert_eq!(unchanged.payload_json["qolip_lock_owner"], true);
    assert!(!unchanged.qolip_reacquisition_required());
    assert!(returned.is_ok(), "{returned:?}");
    assert_eq!(replay, Err(QolipError::CheckoutNotReturnable));
    assert_eq!(quantity, 1);
}

#[tokio::test]
async fn physical_qolip_return_rechecks_resume_and_reorder_after_waiting_for_locks() {
    let f = Fixture::new().await;
    let mut failures = Vec::new();
    for resume in [true, false] {
        seed(&f, false, "paused", "paused").await;
        let mut tx = f.pool.begin().await.unwrap();
        crate::db::postgres_production_map::lock_order_for_edit_tx(&mut tx, ORDER)
            .await
            .unwrap();
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!("mini-rs-erp:production-map:apparatus:{PRINT}"))
            .execute(&mut *tx)
            .await
            .unwrap();
        let store = PostgresQolipStore::new(f.pool.clone());
        let mut returning =
            tokio::spawn(
                async move { store.return_checkout("checkout-return", "A", Some(1)).await },
            );
        if tokio::time::timeout(std::time::Duration::from_millis(150), &mut returning)
            .await
            .is_ok()
        {
            failures.push("return did not wait for production locks".into());
            tx.rollback().await.unwrap();
            continue;
        }
        if resume {
            sqlx::query("UPDATE mini_queue_states SET state='in_progress' WHERE order_id=$1 AND canonical_apparatus_id=$2").bind(ORDER).bind(PRINT).execute(&mut *tx).await.unwrap();
            sqlx::query("UPDATE mini_order_run_sessions SET status='active' WHERE order_id=$1")
                .bind(ORDER)
                .execute(&mut *tx)
                .await
                .unwrap();
        } else {
            sqlx::query(
                "UPDATE mini_queue_sequences SET order_ids=$2 WHERE canonical_apparatus_id=$1",
            )
            .bind(PRINT)
            .bind(json!([ORDER, HEAD]))
            .execute(&mut *tx)
            .await
            .unwrap();
        }
        tx.commit().await.unwrap();
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), returning)
            .await
            .unwrap()
            .unwrap();
        if result != Err(QolipError::QolipInUse) {
            failures.push(format!("resume={resume}: {result:?}"));
        }
        if checkout_status(&f.pool, "checkout-return").await != "open" {
            failures.push("a competing start/reorder allowed a physical return".into());
        }
    }
    f.cleanup().await;
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[tokio::test]
async fn physical_qolip_return_releases_the_session_without_returning_other_loans() {
    let f = Fixture::new().await;
    seed(&f, false, "paused", "paused").await;
    insert_open_checkout(
        &f.pool,
        "checkout-remaining",
        "original-worker",
        "Q-CATALOG-ONLY",
    )
    .await;
    let qolip = PostgresQolipStore::new(f.pool.clone());
    let result = qolip.return_checkout("checkout-return", "A", Some(1)).await;
    let remaining = checkout_status(&f.pool, "checkout-remaining").await;
    let lock = f
        .store
        .active_order_run_session_for_qolip("Q-SESSION")
        .await
        .unwrap();
    let remaining_return = qolip
        .return_checkout("checkout-remaining", "A", Some(1))
        .await;
    f.cleanup().await;
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(remaining, "open");
    assert!(lock.is_none());
    assert!(remaining_return.is_ok(), "{remaining_return:?}");
}

#[tokio::test]
async fn physical_qolip_return_rejects_a_resume_prepared_before_the_return() {
    let f = Fixture::new().await;
    seed(&f, false, "paused", "paused").await;
    // This command was prepared while the complete set was still mounted.
    let stale = session(OrderRunStatus::Active);
    let write = QueueActionProgressWrite {
        apparatus: PRINT.into(),
        map_update: None,
        states: BTreeMap::from([(ORDER.into(), "in_progress".into())]),
        sequence_updates: BTreeMap::new(),
        event: ApparatusQueueActionEvent {
            event_id: "stale-resume-after-physical-return".into(),
            apparatus: PRINT.into(),
            order_id: ORDER.into(),
            stage_node_id: "bosma_7".into(),
            action: ApparatusQueueAction::Resume,
            from_state: ApparatusQueueOrderState::Paused,
            to_state: ApparatusQueueOrderState::InProgress,
            policy: ApparatusQueuePolicy::StrictSequence,
            actor: QueueActionActor {
                role: stale.worker_role.clone(),
                ref_: stale.worker_ref.clone(),
                display_name: stale.worker_display_name.clone(),
            },
            assigned_apparatus: vec![PRINT.into()],
            payload_json: json!({}),
        },
        session: Some(stale),
        progress_event: None,
        progress_batch: None,
        progress_batches: vec![],
        progress_batch_updates: vec![],
        opening_wip_batch_updates: vec![],
        raw_material_stock_transitions: vec![],
        qolip_checkouts: vec![],
        returned_paint_report: None,
        order_control_update: None,
        schedule_reservation_status: None,
        print_preflight_hold_id: None,
        print_preflight_cancel_hold_id: None,
    };
    PostgresQolipStore::new(f.pool.clone())
        .return_checkout("checkout-return", "A", Some(1))
        .await
        .unwrap();
    let result = f
        .store
        .put_apparatus_queue_states_with_event_and_progress(&write)
        .await;
    let saved = f
        .store
        .order_run_session("session-qolip-completion")
        .await
        .unwrap()
        .unwrap();
    f.cleanup().await;
    assert_eq!(result, Err(ProductionMapError::QolipCodeMismatch));
    assert_eq!(saved.status, OrderRunStatus::Paused);
    assert_eq!(saved.payload_json["qolip_lock_owner"], false);
    assert!(saved.qolip_reacquisition_required());
}

#[tokio::test]
async fn physical_qolip_return_cannot_release_another_active_print_owner() {
    let f = Fixture::new().await;
    seed(&f, false, "paused", "paused").await;
    let mut competing = session(OrderRunStatus::Active);
    competing.session_id = "competing-printer".into();
    competing.order_id = HEAD.into();
    f.store.put_order_run_session(competing).await.unwrap();
    f.store
        .put_apparatus_queue_states(
            PRINT,
            BTreeMap::from([
                (HEAD.into(), "in_progress".into()),
                (ORDER.into(), "paused".into()),
            ]),
        )
        .await
        .unwrap();
    let result = PostgresQolipStore::new(f.pool.clone())
        .return_checkout("checkout-return", "A", Some(1))
        .await;
    let debt = checkout_status(&f.pool, "checkout-return").await;
    let target = f
        .store
        .order_run_session("session-qolip-completion")
        .await
        .unwrap()
        .unwrap();
    f.cleanup().await;
    assert_eq!(result, Err(QolipError::QolipInUse));
    assert_eq!(debt, "open");
    assert_eq!(target.payload_json["qolip_lock_owner"], true);
}
