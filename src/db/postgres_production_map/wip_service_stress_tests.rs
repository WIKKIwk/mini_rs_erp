use super::*;
use std::time::Instant;
use tokio::sync::{Barrier, Mutex};

const DEVICES: usize = 8;
const CARDS: usize = 4;

#[derive(Clone)]
struct Device {
    apparatus: String,
    order: String,
    paddon: String,
    actor: QueueActionActor,
}

#[derive(Default)]
struct DeviceMeasurements {
    start_ms: Vec<f64>,
    card_ms: Vec<f64>,
    replay_ms: Vec<f64>,
    batches: Vec<String>,
    empty_replay_responses: usize,
    errors: Vec<String>,
}

fn distribution(values: &[f64]) -> serde_json::Value {
    let mut values = values.to_vec();
    values.sort_by(f64::total_cmp);
    if values.is_empty() {
        return serde_json::json!({ "samples": 0 });
    }
    serde_json::json!({
        "samples": values.len(),
        "median_ms": if values.len() % 2 == 0 {
            (values[values.len() / 2 - 1] + values[values.len() / 2]) / 2.0
        } else { values[values.len() / 2] },
        "p95_ms": values[(values.len() * 95).div_ceil(100).saturating_sub(1)],
    })
}

async fn prepare_devices(pool: &PgPool, service: &ProductionMapService) -> Vec<Device> {
    let canonical = CanonicalApparatusService::new(Arc::new(
        PostgresCanonicalApparatusRepository::new(pool.clone()),
    ));
    let mut devices = Vec::with_capacity(DEVICES);
    for index in 0..DEVICES {
        let apparatus = format!("apparatus:test:stress-cut-{index}");
        canonical
            .seed_for_test(
                ApparatusId::new(&apparatus).unwrap(),
                canonical_draft(&TestApparatusSpec::cut(
                    &apparatus,
                    &format!("Stress Cut {index}"),
                )),
            )
            .await
            .unwrap();
        let order = format!("zakaz-stress-cut-{index}");
        let map = serde_json::from_value(serde_json::json!({
            "id":order,"product_code":order,"title":order,"order_number":format!("98{index:02}"),
            "nodes":[{"id":"start","kind":"start","title":"Start"},
                {"id":"cut","kind":"apparatus","title":apparatus,"apparatus_id":apparatus,
                 "rezka_kadr_count":CARDS,"rezka_frame_groups":vec![1;CARDS]},
                {"id":"end","kind":"end","title":"End"}],
            "edges":[{"from":"start","to":"cut"},{"from":"cut","to":"end"}]
        }))
        .unwrap();
        service.upsert_map(map).await.unwrap();
        let actor = QueueActionActor {
            role: "aparatchi".into(),
            ref_: format!("stress-worker-{index}"),
            display_name: format!("Stress worker {index}"),
        };
        let paddon = service
            .create_paddon("", "Isolated service stress fixture", &actor)
            .await
            .unwrap();
        service
            .set_active_rezka_paddon(&apparatus, &actor, &paddon.code)
            .await
            .unwrap();
        devices.push(Device {
            apparatus,
            order,
            paddon: paddon.id,
            actor,
        });
    }
    // Warm resolver/connection paths equally before each timed workload.
    service.active_canonical_apparatuses().await.unwrap();
    devices
}

async fn card_action(
    service: &ProductionMapService,
    device: &Device,
    index: usize,
    cycle: &str,
) -> Result<Option<String>, ProductionMapError> {
    let progress = QueueProgressInput {
        rezka_record_frame_index: Some(index),
        rezka_output_cycle: cycle.into(),
        rezka_frames: vec![
            serde_json::from_value(serde_json::json!({
                "produced_qty":120.0,"gross_qty":12.0,"bobina_kg":0.5,"diameter":45.0
            }))
            .unwrap(),
        ],
        ..Default::default()
    };
    // This is the HTTP handler's real guarded prepare/commit path, including
    // transaction-time active-paddon selection. No printer is called.
    let _guard = service
        .queue_progress_action_guard(
            &device.apparatus,
            queue_state::ApparatusQueueAction::RollComplete,
            &progress,
            &[],
            &[],
        )
        .await?;
    let mut prepared = service
        .prepare_apparatus_queue_action_with_progress(
            &device.apparatus,
            &device.order,
            queue_state::ApparatusQueueAction::RollComplete,
            std::slice::from_ref(&device.apparatus),
            device.actor.clone(),
            progress,
        )
        .await?;
    prepared.attach_active_paddon();
    let result = service.commit_prepared_queue_action(prepared).await?;
    Ok(result.progress_batch.map(|batch| batch.batch_id))
}

async fn run_device(
    service: ProductionMapService,
    device: Device,
    start: Arc<Barrier>,
    serialized_gate: Option<Arc<Mutex<()>>>,
) -> DeviceMeasurements {
    let mut measurements = DeviceMeasurements::default();
    start.wait().await;
    let began = Instant::now();
    let gate = if let Some(gate) = &serialized_gate {
        Some(gate.lock().await)
    } else {
        None
    };
    let started = service
        .apply_apparatus_queue_action_with_progress(
            &device.apparatus,
            &device.order,
            queue_state::ApparatusQueueAction::Start,
            std::slice::from_ref(&device.apparatus),
            device.actor.clone(),
            QueueProgressInput::default(),
        )
        .await;
    measurements
        .start_ms
        .push(began.elapsed().as_secs_f64() * 1000.0);
    drop(gate);
    let cycle = match started {
        Ok(result) => result.session.expect("Start has a session").session_id,
        Err(error) => {
            measurements
                .errors
                .push(format!("{} Start: {error:?}", device.apparatus));
            return measurements;
        }
    };
    for index in 1..=CARDS {
        let began = Instant::now();
        let gate = if let Some(gate) = &serialized_gate {
            Some(gate.lock().await)
        } else {
            None
        };
        let card = card_action(&service, &device, index, &cycle).await;
        measurements
            .card_ms
            .push(began.elapsed().as_secs_f64() * 1000.0);
        drop(gate);
        let batch_id = match card {
            Ok(Some(batch_id)) => batch_id,
            Ok(None) => {
                measurements.errors.push(format!(
                    "{} Card {index}: missing first output",
                    device.apparatus
                ));
                return measurements;
            }
            Err(error) => {
                measurements
                    .errors
                    .push(format!("{} Card {index}: {error:?}", device.apparatus));
                return measurements;
            }
        };
        measurements.batches.push(batch_id.clone());
        let began = Instant::now();
        let gate = if let Some(gate) = &serialized_gate {
            Some(gate.lock().await)
        } else {
            None
        };
        let replay = card_action(&service, &device, index, &cycle).await;
        measurements
            .replay_ms
            .push(began.elapsed().as_secs_f64() * 1000.0);
        drop(gate);
        match replay {
            Ok(Some(replayed_id)) if replayed_id == batch_id => {}
            Ok(None) => measurements.empty_replay_responses += 1,
            result => {
                measurements
                    .errors
                    .push(format!("{} Replay {index}: {result:?}", device.apparatus));
                return measurements;
            }
        }
    }
    measurements
}

async fn run_mode(serialized: bool) -> serde_json::Value {
    let (pool, store, admin, name) = fixture().await;
    let canonical = CanonicalApparatusService::new(Arc::new(
        PostgresCanonicalApparatusRepository::new(pool.clone()),
    ));
    let service = ProductionMapService::new(
        store.clone(),
        Arc::new(CanonicalServiceApparatusResolver::new(canonical)),
    );
    let devices = prepare_devices(&pool, &service).await;
    // Equivalent warm pools isolate serialization from initial TCP/auth cost.
    let mut warm_connections = Vec::with_capacity(16);
    for _ in 0..16 {
        warm_connections.push(pool.acquire().await.unwrap());
    }
    drop(warm_connections);
    let start = Arc::new(Barrier::new(DEVICES));
    let gate = serialized.then(|| Arc::new(Mutex::new(())));
    let began = Instant::now();
    let mut workers = Vec::with_capacity(DEVICES);
    for device in devices.clone() {
        workers.push(tokio::spawn(run_device(
            service.clone(),
            device,
            start.clone(),
            gate.clone(),
        )));
    }
    let mut all = DeviceMeasurements::default();
    for worker in workers {
        let mut result = worker.await.unwrap();
        all.start_ms.append(&mut result.start_ms);
        all.card_ms.append(&mut result.card_ms);
        all.replay_ms.append(&mut result.replay_ms);
        all.batches.append(&mut result.batches);
        all.errors.append(&mut result.errors);
        all.empty_replay_responses += result.empty_replay_responses;
    }
    let wall_ms = began.elapsed().as_secs_f64() * 1000.0;
    let outputs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mini_progress_batches WHERE order_id LIKE 'zakaz-stress-cut-%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let unique_qrs: i64 = sqlx::query_scalar("SELECT count(DISTINCT qr_payload) FROM mini_progress_batches WHERE order_id LIKE 'zakaz-stress-cut-%'")
        .fetch_one(&pool).await.unwrap();
    let memberships: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_paddon_items i JOIN mini_progress_batches b ON b.batch_id=i.progress_batch_id WHERE i.removed_at IS NULL AND b.order_id LIKE 'zakaz-stress-cut-%'")
        .fetch_one(&pool).await.unwrap();
    let events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mini_queue_action_events WHERE order_id LIKE 'zakaz-stress-cut-%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let batch_qty: f64 = sqlx::query_scalar("SELECT COALESCE(sum(produced_qty),0)::double precision FROM mini_progress_batches WHERE order_id LIKE 'zakaz-stress-cut-%'")
        .fetch_one(&pool).await.unwrap();
    let event_qty: f64 = sqlx::query_scalar("SELECT COALESCE(sum(produced_qty),0)::double precision FROM mini_order_progress_events WHERE order_id LIKE 'zakaz-stress-cut-%'")
        .fetch_one(&pool).await.unwrap();
    let mut valid_memberships = true;
    for device in &devices {
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_paddon_items i JOIN mini_progress_batches b ON b.batch_id=i.progress_batch_id WHERE i.removed_at IS NULL AND i.paddon_id=$1 AND b.order_id=$2")
            .bind(&device.paddon).bind(&device.order).fetch_one(&pool).await.unwrap();
        valid_memberships &= count == CARDS as i64;
    }
    let distinct_batch_ids = all
        .batches
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let report = serde_json::json!({
        "mode": if serialized { "serialized_service_gate" } else { "scoped_parallel_service" },
        "devices": DEVICES, "cards_per_device": CARDS, "pool_max_connections":16,
        "pool_acquire_timeout_ms":500, "total_wall_ms":wall_ms,
        "start": distribution(&all.start_ms), "recorded_card":distribution(&all.card_ms),
        "replay":distribution(&all.replay_ms), "errors":all.errors,
        "outputs":outputs,"unique_qrs":unique_qrs,"active_memberships":memberships,
        "events":events,"distinct_response_batch_ids":distinct_batch_ids,
        "correct_paddon_memberships":valid_memberships,
        "batch_produced_qty":batch_qty,"progress_event_produced_qty":event_qty,
        "empty_replay_responses":all.empty_replay_responses,
        "retry_audit_events":events - (DEVICES * (CARDS + 1)) as i64,
    });
    drop(service);
    cleanup(pool, store, admin, name).await;
    report
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "manual real PostgreSQL service contention benchmark; uses two disposable databases"]
async fn benchmark_postgres_service_eight_apparatuses_and_selected_paddons() {
    let baseline = run_mode(true).await;
    let parallel = run_mode(false).await;
    println!(
        "WIP_SERVICE_STRESS {}",
        serde_json::json!({ "baseline":baseline,"parallel":parallel })
    );
    for report in [&baseline, &parallel] {
        assert_eq!(report["errors"], serde_json::json!([]), "{report}");
        for field in [
            "outputs",
            "unique_qrs",
            "active_memberships",
            "distinct_response_batch_ids",
        ] {
            assert_eq!(
                report[field],
                serde_json::json!(DEVICES * CARDS),
                "{field}: {report}"
            );
        }
        assert_eq!(
            report["batch_produced_qty"],
            serde_json::json!((DEVICES * CARDS) as f64 * 120.0)
        );
        assert_eq!(
            report["progress_event_produced_qty"],
            serde_json::json!((DEVICES * CARDS) as f64 * 120.0)
        );
        // Existing fresh-request retries record zero-quantity audit events;
        // replay of the same prepared transaction stays idempotent.
        assert!(report["events"].as_u64().unwrap() >= (DEVICES * (CARDS + 1)) as u64);
        assert!(report["events"].as_u64().unwrap() <= (DEVICES * (2 * CARDS + 1)) as u64);
        assert_eq!(
            report["correct_paddon_memberships"],
            serde_json::json!(true)
        );
    }
}
