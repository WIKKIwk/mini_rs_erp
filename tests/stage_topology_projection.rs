#![cfg(feature = "verification")]
//! Exact old/new production closure parity, including lifecycle-write decisions.
#[path = "support/stage_work_reference.rs"]
mod reference;

use mini_rs_erp::core::production_map::{
    queue_state::{ApparatusQueueAction as Action, ApparatusQueueOrderState as State},
    verification_stage_work::{self as actual, Event, Input},
    *,
};
use serde_json::json;
use std::{collections::BTreeMap, hint::black_box, time::Instant};

fn map(operations: usize, repeated: bool, alternative: bool) -> ProductionMapDefinition {
    let mut nodes = vec![json!({"id":"start","kind":"start","title":"Start"})];
    let mut edges = Vec::new();
    let mut previous = "start".to_string();
    for n in 0..operations {
        let id = format!("node-{n}");
        let apparatus = format!(
            "apparatus:test:machine-{}",
            if repeated { n % 2 } else { n }
        );
        let mut node = json!({"id":id,"kind":"apparatus","title":"Work","apparatus_id":apparatus});
        if alternative && n == 1 {
            node["alternative_group_id"] = json!(" shared ");
            node["alternative_assigned_apparatus_id"] = json!(apparatus);
            nodes.push(
                json!({"id":"alternative","kind":"apparatus","title":"Alternative",
                "apparatus_id":"apparatus:test:alternative","alternative_group_id":"shared",
                "alternative_assigned_apparatus_id":apparatus}),
            );
        }
        nodes.push(node);
        edges.push(json!({"from":previous,"to":id}));
        previous = id;
    }
    nodes.push(json!({"id":"end","kind":"end","title":"End"}));
    edges.push(json!({"from":previous,"to":"end"}));
    serde_json::from_value(
        json!({"id":"order","product_code":"P","title":"Order","nodes":nodes,"edges":edges}),
    )
    .unwrap()
}

fn session(map: &ProductionMapDefinition, index: usize) -> OrderRunSession {
    let stages = chain::linear_work_stages(map);
    let stage = stages.get(index % stages.len().max(1));
    OrderRunSession {
        session_id: format!("session-{index:04}"),
        apparatus: stage
            .and_then(|s| s.apparatus_id.clone())
            .unwrap_or_else(|| "apparatus:test:machine-0".into()),
        order_id: map.id.clone(),
        stage_node_id: stage.map(|s| s.node_id.clone()).unwrap_or_default(),
        status: OrderRunStatus::Completed,
        worker_role: "aparatchi".into(),
        worker_ref: format!("worker-{index}"),
        worker_display_name: "Worker".into(),
        started_at_unix: index as i64,
        updated_at_unix: index as i64 + 1,
        payload_json: json!({"stage_work_protocol":1,"stage_work_report":{
            "report_id":format!("report-{index}"),"sequence":index,"submitted_at_unix":index,
            "worker_ref":format!("worker-{index}"),"worker_role":"aparatchi","worker_display_name":"Worker"}}),
    }
}

fn parity(
    map: &ProductionMapDefinition,
    sessions: &[OrderRunSession],
    inputs: &[Input],
    states: &BTreeMap<String, BTreeMap<String, String>>,
    events: &[Event],
) {
    let before = json!({"map":map,"sessions":sessions,"states":states});
    let expected = reference::stage_work_statuses(map, sessions, inputs, states, events);
    let got = actual::statuses(map, sessions, inputs, states, events);
    assert_eq!(
        serde_json::to_value(&got).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    for fallback in [
        ProductionOrderLifecycleStatus::Released,
        ProductionOrderLifecycleStatus::InProgress,
        ProductionOrderLifecycleStatus::ProductionCompleted,
        ProductionOrderLifecycleStatus::Closed,
        ProductionOrderLifecycleStatus::Cancelled,
    ] {
        assert_eq!(
            actual::lifecycle(fallback, &got, sessions),
            reference::work_lifecycle(fallback, &expected, sessions)
        );
    }
    assert_eq!(
        before,
        json!({"map":map,"sessions":sessions,"states":states}),
        "pure evaluator must not mutate ERP inputs"
    );
}

#[test]
fn exact_parity_covers_topology_history_inputs_events_and_legacy_identity() {
    let mut cases = 0;
    for operations in [0, 1, 3, 6] {
        for repeated in [false, true] {
            for alternative in [false, true] {
                for history in [0, 1, 20, 100] {
                    for raw_order_id in ["order", " order ", "", " "] {
                        let mut map = map(operations, repeated, alternative);
                        map.id = raw_order_id.into();
                        let mut sessions =
                            (0..history).map(|i| session(&map, i)).collect::<Vec<_>>();
                        for (i, session) in sessions.iter_mut().enumerate() {
                            session.status = [
                                OrderRunStatus::Completed,
                                OrderRunStatus::Active,
                                OrderRunStatus::Paused,
                                OrderRunStatus::Frozen,
                                OrderRunStatus::RollDetached,
                            ][i % 5];
                            match i % 11 {
                                0 => session.stage_node_id.clear(),
                                1 => session.stage_node_id = "  ".into(),
                                2 => session.stage_node_id = "deleted-node".into(),
                                3 => session.apparatus = format!(" {} ", session.apparatus),
                                4 => session.order_id = "different-order".into(),
                                5 => session.order_id = format!(" {} ", map.id),
                                6 => session.payload_json = json!({}),
                                7 => session.payload_json = json!({"stage_work_protocol":1}),
                                8 => {
                                    session.payload_json["stage_work_report"] =
                                        json!({"invalid":true})
                                }
                                9 => session.stage_node_id = "alternative".into(),
                                _ => {}
                            }
                        }
                        let states = chain::linear_work_stages(&map)
                            .into_iter()
                            .filter_map(|s| s.apparatus_id)
                            .map(|a| (a, BTreeMap::from([(map.id.clone(), "completed".into())])))
                            .collect();
                        let events = vec![
                            Event {
                                stage_node_id: "node-0".into(),
                                action: Action::Complete,
                                to_state: State::Completed,
                            },
                            Event {
                                stage_node_id: "node-0".into(),
                                action: Action::Start,
                                to_state: State::InProgress,
                            },
                        ];
                        let inputs = vec![
                            Input {
                                target_node: "node-1".into(),
                                outstanding: true,
                                available: true,
                                ..Default::default()
                            },
                            Input {
                                source_node: "node-0".into(),
                                outstanding: true,
                                ..Default::default()
                            },
                            Input {
                                target_apparatus: "apparatus:test:machine-0".into(),
                                outstanding: true,
                                ..Default::default()
                            },
                            Input {
                                source_apparatus: "apparatus:test:machine-0".into(),
                                outstanding: true,
                                ..Default::default()
                            },
                            Input {
                                target_node: "node-0".into(),
                                outstanding: false,
                                available: true,
                                ..Default::default()
                            },
                        ];
                        parity(&map, &sessions, &[], &states, &[]);
                        parity(&map, &sessions, &inputs, &states, &events);
                        sessions.reverse();
                        parity(&map, &sessions, &inputs, &states, &events);
                        cases += 3;
                    }
                }
            }
        }
    }
    assert_eq!(cases, 768);
}

#[test]
fn exact_ties_keep_first_source_and_repeated_legacy_sessions_remain_ambiguous() {
    let simple = map(1, false, false);
    let first = session(&simple, 0);
    let mut second = first.clone();
    second.status = OrderRunStatus::Active;
    second.payload_json = json!({"stage_work_protocol":1});
    for sessions in [
        vec![first.clone(), second.clone()],
        vec![second.clone(), first.clone()],
    ] {
        parity(&simple, &sessions, &[], &BTreeMap::new(), &[]);
        assert_eq!(
            actual::statuses(&simple, &sessions, &[], &BTreeMap::new(), &[])[0].completed,
            sessions[0].status == OrderRunStatus::Completed
        );
    }
    let repeated = map(3, true, false);
    let mut legacy = session(&repeated, 0);
    legacy.stage_node_id = " \t ".into();
    let actual = actual::statuses(&repeated, &[legacy.clone()], &[], &BTreeMap::new(), &[]);
    assert!(
        actual
            .iter()
            .all(|s| !s.completed && s.last_worker_ref.is_empty())
    );
    parity(&repeated, &[legacy], &[], &BTreeMap::new(), &[]);

    // Equal report sequence on alternative participants uses BTreeMap apparatus order.
    let alternatives = map(3, false, true);
    let mut sessions = (0..4)
        .map(|i| session(&alternatives, i))
        .collect::<Vec<_>>();
    for s in &mut sessions {
        s.payload_json["stage_work_report"]["sequence"] = json!(10);
    }
    parity(&alternatives, &sessions, &[], &BTreeMap::new(), &[]);
    sessions.reverse();
    parity(&alternatives, &sessions, &[], &BTreeMap::new(), &[]);
}

#[test]
fn actual_resolver_counts_drop_from_operations_times_history_to_history() {
    for operations in [0, 1, 3, 6] {
        for history in [0, 1, 20, 100] {
            let map = map(operations, false, false);
            let sessions = (0..history).map(|i| session(&map, i)).collect::<Vec<_>>();
            let (expected, old_calls) = actual::measure_resolutions(|| {
                reference::stage_work_statuses(&map, &sessions, &[], &BTreeMap::new(), &[])
            });
            let (got, new_calls) = actual::measure_resolutions(|| {
                actual::statuses(&map, &sessions, &[], &BTreeMap::new(), &[])
            });
            assert_eq!(
                serde_json::to_value(got).unwrap(),
                serde_json::to_value(expected).unwrap()
            );
            assert_eq!(old_calls, operations * history);
            assert_eq!(new_calls, if operations == 0 { 0 } else { history });
            println!(
                "topology-count operations={operations} sessions={history} old={old_calls} new={new_calls}"
            );
        }
    }
}

#[tokio::test]
async fn actual_lifecycle_writes_match_reference_and_preserve_session_history() {
    for operations in [1, 3, 6] {
        for history in [0, 1, 20, 100] {
            let map = map(operations, false, false);
            let store = MemoryProductionMapStore::new();
            store.put_map(map.clone()).await.unwrap();
            let sessions = (0..history).map(|i| session(&map, i)).collect::<Vec<_>>();
            for session in &sessions {
                store.put_order_run_session(session.clone()).await.unwrap();
            }
            let mut states = BTreeMap::new();
            for stage in chain::linear_work_stages(&map) {
                let apparatus = stage.apparatus_id.unwrap();
                let values = BTreeMap::from([(map.id.clone(), "completed".into())]);
                states.insert(apparatus.clone(), values.clone());
                store
                    .put_apparatus_queue_states(&apparatus, values)
                    .await
                    .unwrap();
            }
            let expected = reference::stage_work_statuses(&map, &sessions, &[], &states, &[]);
            let lifecycle = reference::work_lifecycle(
                ProductionOrderLifecycleStatus::ProductionCompleted,
                &expected,
                &sessions,
            );
            let saved = store
                .production_order_lifecycles(std::slice::from_ref(&map.id))
                .await
                .unwrap();
            assert_eq!(saved[&map.id].status, lifecycle);
            let committed = json!({"map":store.maps().await.unwrap(),"sessions":store.order_run_sessions_for_order(&map.id).await.unwrap(),"states":store.apparatus_queue_states().await.unwrap(),"lifecycle":saved});
            // Repeating the same write must not create a lifecycle version change.
            let apparatus = chain::linear_work_stages(&map)[0]
                .apparatus_id
                .clone()
                .unwrap();
            store
                .put_apparatus_queue_states(&apparatus, states[&apparatus].clone())
                .await
                .unwrap();
            assert_eq!(
                committed,
                json!({"map":store.maps().await.unwrap(),"sessions":store.order_run_sessions_for_order(&map.id).await.unwrap(),"states":store.apparatus_queue_states().await.unwrap(),"lifecycle":store.production_order_lifecycles(std::slice::from_ref(&map.id)).await.unwrap()})
            );
            assert_eq!(
                serde_json::to_value(store.order_run_sessions_for_order(&map.id).await.unwrap())
                    .unwrap(),
                serde_json::to_value(sessions).unwrap()
            );
        }
    }
}

#[tokio::test]
async fn lifecycle_writes_do_not_close_active_or_unreported_executions() {
    for status in [
        OrderRunStatus::Active,
        OrderRunStatus::Paused,
        OrderRunStatus::Frozen,
        OrderRunStatus::RollDetached,
        OrderRunStatus::Completed,
    ] {
        let map = map(3, false, false);
        let store = MemoryProductionMapStore::new();
        store.put_map(map.clone()).await.unwrap();
        let mut run = session(&map, 0);
        run.status = status;
        run.payload_json = json!({"stage_work_protocol":1});
        store.put_order_run_session(run.clone()).await.unwrap();
        let mut states = BTreeMap::new();
        for stage in chain::linear_work_stages(&map) {
            let apparatus = stage.apparatus_id.unwrap();
            let values = BTreeMap::from([(map.id.clone(), "completed".into())]);
            states.insert(apparatus.clone(), values.clone());
            store
                .put_apparatus_queue_states(&apparatus, values)
                .await
                .unwrap();
        }
        let expected = reference::stage_work_statuses(&map, &[run.clone()], &[], &states, &[]);
        let expected = reference::work_lifecycle(
            ProductionOrderLifecycleStatus::ProductionCompleted,
            &expected,
            &[run.clone()],
        );
        assert_eq!(expected, ProductionOrderLifecycleStatus::InProgress);
        let saved = store
            .production_order_lifecycles(std::slice::from_ref(&map.id))
            .await
            .unwrap();
        assert_eq!(saved[&map.id].status, expected);
        assert_eq!(
            store.order_run_sessions_for_order(&map.id).await.unwrap(),
            vec![run]
        );
    }
}

#[test]
#[ignore = "repeatable elapsed-time measurement; no timing threshold in correctness tests"]
fn repeated_compiled_projection_workload() {
    for operations in [0, 1, 3, 6] {
        for history in [0, 1, 20, 100] {
            let map = map(operations, false, false);
            let sessions = (0..history).map(|i| session(&map, i)).collect::<Vec<_>>();
            let states = BTreeMap::new();
            let repetitions = 285;
            let mut old = Vec::new();
            let mut new = Vec::new();
            for round in 0..7 {
                for baseline in if round % 2 == 0 {
                    [true, false]
                } else {
                    [false, true]
                } {
                    let start = Instant::now();
                    for _ in 0..repetitions {
                        black_box(if baseline {
                            reference::stage_work_statuses(
                                black_box(&map),
                                black_box(&sessions),
                                &[],
                                &states,
                                &[],
                            )
                        } else {
                            actual::statuses(
                                black_box(&map),
                                black_box(&sessions),
                                &[],
                                &states,
                                &[],
                            )
                        });
                    }
                    if round > 0 {
                        if baseline {
                            old.push(start.elapsed().as_micros());
                        } else {
                            new.push(start.elapsed().as_micros());
                        }
                    }
                }
            }
            old.sort_unstable();
            new.sort_unstable();
            println!(
                "topology-elapsed operations={operations} sessions={history} maps={repetitions} baseline_median_us={} candidate_median_us={} ratio={:.3}",
                old[old.len() / 2],
                new[new.len() / 2],
                new[new.len() / 2] as f64 / old[old.len() / 2] as f64
            );
        }
    }
}
