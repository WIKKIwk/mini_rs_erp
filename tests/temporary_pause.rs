#![cfg(feature = "verification")]

use mini_rs_erp::core::{apparatus_standard::*, production_map::*};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};

use queue_state::ApparatusQueueAction as Action;

const PRESS: &str = "apparatus:default:bosma_7";
const CUT: &str = "apparatus:default:asset-010";
const A: &str = "zakaz-temporary-a";
const B: &str = "zakaz-temporary-b";
const C: &str = "zakaz-temporary-c";

struct Resolver(Vec<Arc<RuntimeApparatusConfiguration>>);

#[async_trait::async_trait]
impl CanonicalApparatusResolver for Resolver {
    async fn resolve(
        &self,
        id: &ApparatusId,
    ) -> Result<Option<Arc<RuntimeApparatusConfiguration>>, ProductionMapError> {
        Ok(self
            .0
            .iter()
            .find(|item| &item.runtime.apparatus_id == id)
            .cloned())
    }

    async fn list(&self) -> Result<Vec<Arc<RuntimeApparatusConfiguration>>, ProductionMapError> {
        Ok(self.0.clone())
    }
}

fn resolver(policy: QueueDiscipline) -> Arc<Resolver> {
    Arc::new(Resolver(
        [PRESS, CUT]
            .into_iter()
            .map(|id| {
                let mut revision = mini_rs_apparatus_contract::isa95::test_support::revision_with(
                    id,
                    &format!("physical-asset:{}", id.rsplit(':').next().unwrap()),
                    "Temporary pause fixture",
                );
                if id == CUT {
                    revision.execution_profile.operation = ExecutionOperation::Cut;
                    revision.execution_profile.technology = ProcessTechnology::Slitting;
                    revision.execution_profile.color_station_count = None;
                    revision.capabilities = vec![EquipmentCapability {
                        code: EquipmentCapabilityCode::Cut,
                        level: 1,
                    }];
                    revision.training.enabled = false;
                    revision.training.queue_enabled = false;
                    revision.training.material_tracking_enabled = false;
                }
                revision.policies.queue = policy;
                revision.policies.material = MaterialExecutionPolicy::NotRequired {
                    item_group_ids: vec![],
                };
                revision.policies.tooling = ToolingExecutionPolicy::NotRequired;
                revision.capacity.availability = CapacityAvailability::Always;
                revision.validate().unwrap();
                Arc::new(
                    project_apparatus_revision(&revision, AasxSha256::digest(id.as_bytes())).into(),
                )
            })
            .collect(),
    ))
}

fn actor() -> QueueActionActor {
    QueueActionActor {
        role: "aparatchi".into(),
        ref_: "temporary-pause-worker".into(),
        display_name: "Temporary pause worker".into(),
    }
}

fn map(order: &str, cutting: bool) -> ProductionMapDefinition {
    let mut nodes = vec![
        json!({"id":"start", "kind":"start", "title":"Start"}),
        json!({"id":"print", "kind":"apparatus", "title":"Print", "apparatus_id":PRESS}),
    ];
    let mut edges = vec![json!({"from":"start", "to":"print"})];
    if cutting {
        nodes.push(json!({"id":"cut", "kind":"apparatus", "title":"Cut", "apparatus_id":CUT, "rezka_kadr_count":2}));
        edges.push(json!({"from":"print", "to":"cut"}));
        edges.push(json!({"from":"cut", "to":"end"}));
    } else {
        edges.push(json!({"from":"print", "to":"end"}));
    }
    nodes.push(json!({"id":"end", "kind":"end", "title":"End"}));
    serde_json::from_value(
        json!({"id":order, "product_code":order, "title":order, "nodes":nodes, "edges":edges}),
    )
    .unwrap()
}

struct Fixture {
    service: ProductionMapService,
    store: Arc<MemoryProductionMapStore>,
}

impl Fixture {
    async fn new(policy: QueueDiscipline) -> Self {
        let store = Arc::new(MemoryProductionMapStore::new());
        let service = ProductionMapService::new(store.clone(), resolver(policy));
        for order in [A, B, C] {
            service.upsert_map(map(order, false)).await.unwrap();
        }
        service
            .set_apparatus_sequence(PRESS, vec![A.into(), B.into(), C.into()])
            .await
            .unwrap();
        Self { service, store }
    }

    async fn run(
        &self,
        apparatus: &str,
        order: &str,
        action: Action,
        input: QueueProgressInput,
    ) -> Result<ApparatusQueueActionResult, ProductionMapError> {
        self.service
            .apply_apparatus_queue_action_with_progress(
                apparatus,
                order,
                action,
                &[apparatus.into()],
                actor(),
                input,
            )
            .await
    }

    async fn session(&self, apparatus: &str, order: &str) -> OrderRunSession {
        self.store
            .active_order_run_session(apparatus, order)
            .await
            .unwrap()
            .unwrap()
    }

    async fn control(&self, apparatus: &str, order: &str) -> ApparatusQueueOrderActionControl {
        self.service
            .queue_action_controls()
            .await
            .unwrap()
            .remove(apparatus)
            .unwrap()
            .remove(order)
            .unwrap()
    }

    async fn pause(&self, apparatus: &str, order: &str) -> TemporaryPauseControl {
        let control = self
            .control(apparatus, order)
            .await
            .temporary_pause
            .expect("running worker has a server-owned temporary pause control");
        self.service
            .temporarily_pause_work(
                apparatus,
                order,
                &control.session_id,
                &control.token,
                &[apparatus.into()],
                actor(),
            )
            .await
            .unwrap();
        control
    }
}

fn output() -> QueueProgressInput {
    QueueProgressInput {
        produced_qty: Some(120.0),
        gross_qty: Some(12.0),
        finished_goods_meter: Some(120.0),
        finished_goods_kg: Some(12.0),
        bobina_kg: Some(1.0),
        uom: "m".into(),
        ..Default::default()
    }
}

#[tokio::test]
async fn urgent_order_output_does_not_close_or_restart_the_temporarily_removed_roll() {
    let f = Fixture::new(QueueDiscipline::StrictSequence).await;
    f.run(PRESS, A, Action::Start, QueueProgressInput::default())
        .await
        .unwrap();
    let mut original = f.session(PRESS, A).await;
    original.payload_json["wip_started_at_unix"] = json!(100);
    f.store
        .put_order_run_session(original.clone())
        .await
        .unwrap();

    let receipt = f.pause(PRESS, A).await;
    let stopped = f.session(PRESS, A).await;
    assert_eq!(stopped.session_id, original.session_id);
    assert_eq!(stopped.status, OrderRunStatus::Paused);
    assert_eq!(stopped.payload_json["wip_started_at_unix"], 100);
    assert_eq!(stopped.payload_json["wip_waiting_for_resume"], false);
    assert!(f.control(PRESS, A).await.temporarily_paused);
    assert!(
        f.store
            .progress_batches_for_order(A)
            .await
            .unwrap()
            .is_empty()
    );

    f.run(PRESS, B, Action::Start, QueueProgressInput::default())
        .await
        .unwrap();
    assert!(
        f.run(PRESS, A, Action::Resume, QueueProgressInput::default())
            .await
            .is_err(),
        "the machine cannot run both orders"
    );
    let urgent_output = f
        .run(PRESS, B, Action::DetachRoll, output())
        .await
        .unwrap()
        .progress_batch
        .unwrap();
    assert_eq!(urgent_output.order_id, B);
    assert!(
        f.store
            .progress_batches_for_order(A)
            .await
            .unwrap()
            .is_empty()
    );

    let resumed = f
        .run(PRESS, A, Action::Resume, QueueProgressInput::default())
        .await
        .unwrap();
    let continued = resumed.session.unwrap();
    assert_eq!(continued.session_id, original.session_id);
    assert_eq!(
        continued.payload_json["wip_started_at_unix"], 100,
        "continue the physical roll instead of starting a new output cycle"
    );
    assert!(resumed.progress_batch.is_none());
    assert!(resumed.progress_batches.is_empty());
    assert!(!f.control(PRESS, A).await.temporarily_paused);

    // An uncertain write may be replayed after the order has already continued.
    // Its durable receipt must acknowledge it without pausing the newer work.
    f.service
        .temporarily_pause_work(
            PRESS,
            A,
            &receipt.session_id,
            &receipt.token,
            &[PRESS.into()],
            actor(),
        )
        .await
        .unwrap();
    assert_eq!(f.session(PRESS, A).await, continued);

    let completed_roll = f
        .run(PRESS, A, Action::DetachRoll, output())
        .await
        .unwrap()
        .progress_batch
        .unwrap();
    assert_eq!(completed_roll.session_id, original.session_id);
    assert_eq!(completed_roll.started_at_unix, 100);
    assert_eq!(
        f.store.progress_batches_for_order(A).await.unwrap().len(),
        1
    );
    assert_ne!(completed_roll.batch_id, urgent_output.batch_id);
    assert_ne!(completed_roll.qr_payload, urgent_output.qr_payload);
}

#[tokio::test]
async fn strict_sequence_allows_only_the_adjacent_urgent_order_and_cannot_chain_suspensions() {
    let f = Fixture::new(QueueDiscipline::StrictSequence).await;
    f.run(PRESS, A, Action::Start, QueueProgressInput::default())
        .await
        .unwrap();
    f.pause(PRESS, A).await;
    assert!(
        f.control(PRESS, B)
            .await
            .allowed_actions
            .contains(&Action::Start)
    );
    assert!(
        !f.control(PRESS, C)
            .await
            .allowed_actions
            .contains(&Action::Start)
    );
    assert!(
        f.run(PRESS, C, Action::Start, QueueProgressInput::default())
            .await
            .is_err()
    );
    f.run(PRESS, B, Action::Start, QueueProgressInput::default())
        .await
        .unwrap();
    f.pause(PRESS, B).await;
    assert!(
        !f.control(PRESS, C)
            .await
            .allowed_actions
            .contains(&Action::Start)
    );
    assert!(
        f.run(PRESS, C, Action::Start, QueueProgressInput::default())
            .await
            .is_err(),
        "suspending B must not bypass the original A anchor"
    );
}

#[tokio::test]
async fn adjacent_urgent_order_can_start_after_its_color_preflight_passes() {
    let f = Fixture::new(QueueDiscipline::StrictSequence).await;
    f.run(PRESS, A, Action::Start, QueueProgressInput::default())
        .await
        .unwrap();
    f.pause(PRESS, A).await;
    assert!(f.control(PRESS, B).await.print_preflight_allowed);
    let hold = f
        .service
        .begin_print_preflight(PRESS, B, "urgent-color", "urgent-color", actor())
        .await
        .unwrap();
    assert_eq!(hold.status, PrintPreflightStatus::Running);
    assert!(
        !f.control(PRESS, B)
            .await
            .allowed_actions
            .contains(&Action::Start)
    );
    let passed = f
        .service
        .advance_print_preflight_at_revision(
            PRESS,
            B,
            &hold.hold_id,
            "passed",
            Some(hold.revision),
            actor(),
        )
        .await
        .unwrap();
    assert_eq!(passed.status, PrintPreflightStatus::Passed);
    f.service
        .validate_print_preflight_start(PRESS, B, &hold.hold_id)
        .await
        .unwrap();
    assert!(
        f.control(PRESS, B)
            .await
            .allowed_actions
            .contains(&Action::Start),
        "the adjacent order's own passed preflight must not be treated as another running production order"
    );
    assert!(
        !f.control(PRESS, C)
            .await
            .allowed_actions
            .contains(&Action::Start)
    );
}

#[tokio::test]
async fn configured_free_pick_bosma_can_run_a_nonadjacent_urgent_order() {
    let f = Fixture::new(QueueDiscipline::FreePick).await;
    let policy = f
        .service
        .apparatus_queue_policy_records()
        .await
        .unwrap()
        .into_iter()
        .find(|record| record.apparatus_id.as_str() == PRESS)
        .unwrap();
    assert_eq!(
        policy.policy,
        ApparatusQueuePolicy::FreePick,
        "Bosma must honor its configured queue discipline"
    );
    f.run(PRESS, A, Action::Start, QueueProgressInput::default())
        .await
        .unwrap();
    f.pause(PRESS, A).await;
    assert!(
        f.control(PRESS, C)
            .await
            .allowed_actions
            .contains(&Action::Start)
    );
    f.run(PRESS, C, Action::Start, QueueProgressInput::default())
        .await
        .unwrap();
    assert_eq!(f.session(PRESS, C).await.status, OrderRunStatus::Active);
    assert_eq!(f.session(PRESS, A).await.status, OrderRunStatus::Paused);
}

#[tokio::test]
async fn temporary_pause_revalidates_worker_assignment_token_and_frozen_state() {
    let f = Fixture::new(QueueDiscipline::FreePick).await;
    f.run(PRESS, A, Action::Start, QueueProgressInput::default())
        .await
        .unwrap();
    let control = f.control(PRESS, A).await.temporary_pause.unwrap();
    let mut wrong_role = actor();
    wrong_role.role = "admin".into();
    assert!(
        f.service
            .temporarily_pause_work(
                PRESS,
                A,
                &control.session_id,
                &control.token,
                &[PRESS.into()],
                wrong_role
            )
            .await
            .is_err()
    );
    assert!(
        f.service
            .temporarily_pause_work(PRESS, A, &control.session_id, &control.token, &[], actor())
            .await
            .is_err()
    );
    assert!(
        f.service
            .temporarily_pause_work(
                PRESS,
                A,
                &control.session_id,
                "invalid-token",
                &[PRESS.into()],
                actor()
            )
            .await
            .is_err()
    );
    assert_eq!(f.session(PRESS, A).await.status, OrderRunStatus::Active);

    let mut changed = f.session(PRESS, A).await;
    changed.payload_json["new_work_mutation"] = json!(true);
    f.store.put_order_run_session(changed).await.unwrap();
    assert!(
        f.service
            .temporarily_pause_work(
                PRESS,
                A,
                &control.session_id,
                &control.token,
                &[PRESS.into()],
                actor()
            )
            .await
            .is_err(),
        "a stale dialog cannot stop a changed session"
    );

    let refreshed = f.control(PRESS, A).await.temporary_pause.unwrap();
    let mut assigned_colleague = actor();
    assigned_colleague.ref_ = "another-assigned-worker".into();
    f.service
        .temporarily_pause_work(
            PRESS,
            A,
            &refreshed.session_id,
            &refreshed.token,
            &[PRESS.into()],
            assigned_colleague,
        )
        .await
        .unwrap();
    assert_eq!(
        f.session(PRESS, A).await.worker_ref,
        actor().ref_,
        "stopping work must not transfer its production ownership"
    );
    let audit = f.service.queue_action_logs_for_order(A).await.unwrap();
    assert!(
        audit
            .iter()
            .any(|event| event.action == Action::Pause
                && event.actor_ref == "another-assigned-worker"),
        "the stop retains its actual authenticated actor"
    );
    f.run(PRESS, A, Action::Resume, QueueProgressInput::default())
        .await
        .unwrap();
    let refreshed = f.control(PRESS, A).await.temporary_pause.unwrap();
    f.run(
        PRESS,
        A,
        Action::Freeze,
        QueueProgressInput {
            description: "Machine stopped".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(
        f.service
            .temporarily_pause_work(
                PRESS,
                A,
                &refreshed.session_id,
                &refreshed.token,
                &[PRESS.into()],
                actor()
            )
            .await
            .is_err()
    );
    assert_eq!(f.session(PRESS, A).await.status, OrderRunStatus::Frozen);
    assert!(f.control(PRESS, A).await.temporary_pause.is_none());
}

#[tokio::test]
async fn repeated_temporary_pauses_preserve_the_roll_but_ordinary_pause_still_requires_output() {
    let f = Fixture::new(QueueDiscipline::FreePick).await;
    f.run(PRESS, A, Action::Start, QueueProgressInput::default())
        .await
        .unwrap();
    assert!(
        f.run(PRESS, A, Action::Pause, QueueProgressInput::default())
            .await
            .is_err()
    );
    let original = f.session(PRESS, A).await;
    let first = f.pause(PRESS, A).await;
    f.run(PRESS, A, Action::Resume, QueueProgressInput::default())
        .await
        .unwrap();
    let second = f.pause(PRESS, A).await;
    assert_ne!(first.token, second.token);
    f.run(PRESS, A, Action::Resume, QueueProgressInput::default())
        .await
        .unwrap();
    let continued = f.session(PRESS, A).await;
    assert_eq!(continued.session_id, original.session_id);
    assert_eq!(
        continued.payload_json["wip_started_at_unix"],
        original.payload_json["wip_started_at_unix"]
    );
    assert!(
        f.store
            .progress_batches_for_order(A)
            .await
            .unwrap()
            .is_empty()
    );

    let mut before_ordinary_pause = continued;
    before_ordinary_pause.payload_json["wip_started_at_unix"] = json!(100);
    f.store
        .put_order_run_session(before_ordinary_pause)
        .await
        .unwrap();
    let ordinary = f.run(PRESS, A, Action::Pause, output()).await.unwrap();
    assert!(ordinary.progress_batch.is_some());
    assert!(!f.control(PRESS, A).await.temporarily_paused);
    f.run(PRESS, A, Action::Resume, QueueProgressInput::default())
        .await
        .unwrap();
    let ordinary_continuation = f.session(PRESS, A).await;
    assert_eq!(
        ordinary_continuation.payload_json["wip_started_at_unix"],
        ordinary_continuation.updated_at_unix
    );
    assert_ne!(
        ordinary_continuation.payload_json["wip_started_at_unix"], 100,
        "ordinary output pause starts the next physical roll"
    );
    assert_eq!(
        f.store.progress_batches_for_order(A).await.unwrap().len(),
        1
    );
}

#[tokio::test]
async fn rezka_temporary_pause_preserves_claimed_input_and_partially_recorded_output_cycle() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = ProductionMapService::new(store.clone(), resolver(QueueDiscipline::FreePick));
    service.upsert_map(map(A, true)).await.unwrap();
    store
        .put_apparatus_queue_states(PRESS, BTreeMap::from([(A.into(), "completed".into())]))
        .await
        .unwrap();
    store.put_order_run_session(serde_json::from_value(json!({
        "session_id":"producer-run", "apparatus":PRESS, "order_id":A, "stage_node_id":"print", "status":"completed",
        "worker_role":"aparatchi", "worker_ref":"producer", "worker_display_name":"Producer", "started_at_unix":1, "updated_at_unix":2, "payload_json":{}
    })).unwrap()).await.unwrap();
    store.put_order_progress_batch(serde_json::from_value(json!({
        "batch_id":"source-wip", "session_id":"producer-run", "apparatus":PRESS, "order_id":A,
        "started_at_unix":1, "completed_at_unix":2, "action":"complete", "status":"completed", "produced_qty":100.0, "uom":"m",
        "qr_payload":"source-wip-qr", "wip_status":"waiting", "next_apparatus":CUT,
        "label_item_code":A, "label_item_name":"Printed roll", "executor_name":"Producer", "worker_role":"aparatchi", "worker_ref":"producer", "worker_display_name":"Producer",
        "finished_goods_meter":100.0, "finished_goods_kg":10.0, "payload_json":{"stage_node_id":"print", "next_stage_node_id":"cut"}
    })).unwrap()).await.unwrap();
    let f = Fixture { service, store };
    f.run(
        CUT,
        A,
        Action::Start,
        QueueProgressInput {
            qr_payload: "source-wip-qr".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let initial = f.session(CUT, A).await;
    let frame: RezkaFrameProgressInput = serde_json::from_value(
        json!({"produced_qty":100.0, "gross_qty":10.0, "bobina_kg":1.0, "diameter":45.0}),
    )
    .unwrap();
    let recorded = f
        .run(
            CUT,
            A,
            Action::RollComplete,
            QueueProgressInput {
                rezka_record_frame_index: Some(1),
                rezka_output_cycle: initial.session_id.clone(),
                rezka_frames: vec![frame],
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .progress_batch
        .unwrap();
    let before = f.session(CUT, A).await;
    let input_before = f.store.progress_batch("source-wip").await.unwrap().unwrap();
    let batches_before = f.store.progress_batches_for_order(A).await.unwrap();
    f.pause(CUT, A).await;
    f.run(CUT, A, Action::Resume, QueueProgressInput::default())
        .await
        .unwrap();
    let after = f.session(CUT, A).await;
    assert_eq!(after.session_id, before.session_id);
    for field in [
        "input_progress_batch_id",
        "input_lineage",
        "rezka_active_partial_rolls",
        "rezka_output_cycle",
        "rezka_output_report",
        "wip_started_at_unix",
    ] {
        assert_eq!(
            after.payload_json[field], before.payload_json[field],
            "temporary suspension changed {field}"
        );
    }
    assert_eq!(
        f.store.progress_batch("source-wip").await.unwrap().unwrap(),
        input_before
    );
    assert_eq!(input_before.wip_status, OrderProgressBatchWipStatus::InUse);
    assert_eq!(
        f.store
            .progress_batch(&recorded.batch_id)
            .await
            .unwrap()
            .unwrap(),
        recorded
    );
    assert_eq!(
        f.store.progress_batches_for_order(A).await.unwrap(),
        batches_before
    );
}

#[tokio::test]
async fn temporary_stop_seeds_legacy_resumed_roll_clock_before_removing_its_checkpoint() {
    let f = Fixture::new(QueueDiscipline::FreePick).await;
    f.run(PRESS, A, Action::Start, QueueProgressInput::default())
        .await
        .unwrap();
    let mut legacy = f.session(PRESS, A).await;
    legacy.started_at_unix = 10;
    legacy.updated_at_unix = 200;
    let payload = legacy.payload_json.as_object_mut().unwrap();
    payload.remove("wip_started_at_unix");
    // Older reversible resumes retain their checkpoint instead of being
    // migrated. Its presence identifies updated_at as this roll's start.
    payload.insert("_resume_pause".into(), json!({"legacy":true}));
    f.store.put_order_run_session(legacy.clone()).await.unwrap();

    f.pause(PRESS, A).await;
    let stopped = f.session(PRESS, A).await;
    assert_eq!(stopped.payload_json["wip_started_at_unix"], 200);
    assert!(stopped.payload_json.get("_resume_pause").is_none());
    f.run(PRESS, A, Action::Resume, QueueProgressInput::default())
        .await
        .unwrap();
    let batch = f
        .run(PRESS, A, Action::DetachRoll, output())
        .await
        .unwrap()
        .progress_batch
        .unwrap();
    assert_eq!(batch.session_id, legacy.session_id);
    assert_eq!(
        batch.started_at_unix, 200,
        "preserve the old resumed roll's physical start rather than the work-session start or temporary-stop time"
    );
}
