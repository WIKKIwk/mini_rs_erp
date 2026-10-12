//! Differential tests against the pure write-validation algorithms.
use super::*;
use crate::core::production_map::{self as pm, chain, stage_execution, wip_route};
use serde_json::{Value, json};
use std::sync::Arc;

const LAM: &str = "apparatus:default:asset-007";
const CUT: &str = "apparatus:default:asset-010";
const CUT2: &str = "apparatus:default:asset-011";

fn fixture_map(index: usize, layout: usize) -> ProductionMapDefinition {
    let mut value = json!({
        "id": format!("review-order-{index}"), "product_code":"REVIEW", "title":"Review map",
        "nodes":[
            {"id":"start","kind":"start","title":"Start"},
            {"id":"lam1","kind":"apparatus","title":"Laminatsiya","apparatus_id":LAM},
            {"id":"cut1","kind":"apparatus","title":"Rezka","apparatus_id":CUT,"alternative_group_id":"cut-group","rezka_kadr_count":1},
            {"id":"cut2","kind":"apparatus","title":"Rezka 2","apparatus_id":CUT2,"alternative_group_id":"cut-group","rezka_kadr_count":1},
            {"id":"end","kind":"end","title":"End"}
        ],
        "edges":[{"from":"start","to":"lam1"},{"from":"lam1","to":"cut1"},
            {"from":"lam1","to":"cut2"},{"from":"cut1","to":"end"},{"from":"cut2","to":"end"}]
    });
    match layout {
        1 => value["nodes"][3]["alternative_group_id"] = json!(""),
        2 => value["nodes"][3]["apparatus_id"] = json!(CUT),
        3 => value["nodes"][1]["id"] = json!("replacement-lam"),
        4 => {
            value["nodes"].as_array_mut().unwrap().push(json!({"id":"later-cut","kind":"apparatus","title":"Repeated Rezka","apparatus_id":CUT,"rezka_kadr_count":1}));
            value["edges"]
                .as_array_mut()
                .unwrap()
                .push(json!({"from":"cut1","to":"later-cut"}));
        }
        5 => {
            value["nodes"].as_array_mut().unwrap().remove(3);
            value["edges"]
                .as_array_mut()
                .unwrap()
                .retain(|edge| edge["from"] != "cut2" && edge["to"] != "cut2");
        }
        _ => {}
    }
    serde_json::from_value(value).unwrap()
}

fn fixture_batch(order: &str, index: usize) -> OrderProgressBatch {
    serde_json::from_value(json!({
        "batch_id":format!("batch-{order}-{index}"),"session_id":"producer-session",
        "started_at_unix":1,"completed_at_unix":2,"apparatus":LAM,"order_id":order,
        "action":"detach_roll","status":"completed","produced_qty":100.,"uom":"m",
        "qr_payload":format!("qr-{order}-{index}"),"label_item_code":"","label_item_name":"",
        "executor_name":"","worker_role":"aparatchi","worker_ref":"worker","worker_display_name":"Worker",
        "wip_status":"waiting","current_apparatus":LAM,"next_apparatus":CUT,
        "payload_json":{"stage_node_id":"lam1","next_stage_node_id":"cut1"}
    })).unwrap()
}

#[test]
fn cached_routes_topology_and_stage_statuses_match_reference() {
    let maps = (0..6).map(|i| fixture_map(i, i)).collect::<Vec<_>>();
    let mut batches = BTreeMap::new();
    for map in &maps {
        let mut rows = Vec::new();
        for status in [
            pm::OrderProgressBatchWipStatus::Waiting,
            pm::OrderProgressBatchWipStatus::InUse,
            pm::OrderProgressBatchWipStatus::Processed,
        ] {
            for source in ["lam1", "", "missing", "cut1"] {
                for target in ["cut1", "cut2", "missing", ""] {
                    for action in ["detach_roll", "complete", "start"] {
                        let index = rows.len();
                        let mut row = fixture_batch(&map.id, index);
                        row.wip_status = status;
                        row.action = serde_json::from_value(json!(action)).unwrap();
                        row.payload_json["stage_node_id"] = json!(source);
                        row.payload_json["next_stage_node_id"] = json!(target);
                        match index % 5 {
                            1 => {
                                row.payload_json["wip_route_binding"] = json!({"source_stage_node_id":source,"stage_node_id":target,"consumer_apparatus_ids":[CUT,CUT2],"map_fingerprint":"old","remapped":true})
                            }
                            2 => row.payload_json["wip_route_binding"] = Value::Null,
                            3 => {
                                row.payload_json["wip_route_binding"] =
                                    json!({"stage_node_id":target})
                            }
                            _ => {}
                        }
                        if index % 7 == 0 {
                            row.used_by_apparatus = CUT2.into();
                        }
                        if index % 11 == 0 {
                            row.used_by_session_id = "session-x".into();
                        }
                        let mut duplicate = row.clone();
                        duplicate.batch_id.push_str("-different");
                        duplicate.produced_qty += 700.;
                        duplicate.completed_at_unix += 42;
                        duplicate.worker_ref = "another-worker".into();
                        duplicate.qr_payload.push_str("-different");
                        if !duplicate.used_by_session_id.is_empty() {
                            duplicate.used_by_session_id = "session-y".into();
                        }
                        rows.extend([row, duplicate]);
                    }
                }
            }
        }
        batches.insert(map.id.clone(), rows);
    }
    let expected_routes = maps
        .iter()
        .map(|map| {
            batches[&map.id]
                .iter()
                .map(|batch| wip_route::resolve_wip_input_route(map, batch))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let expected_stages = maps
        .iter()
        .map(|map| format!("{:?}", chain::linear_work_stages(map)))
        .collect::<Vec<_>>();
    let expected_statuses = maps
        .iter()
        .map(|map| {
            let inputs = stage_execution::work_inputs(map, &batches[&map.id], &[]);
            serde_json::to_value(stage_execution::stage_work_statuses(
                map,
                &[],
                &inputs,
                &BTreeMap::new(),
                &[],
            ))
            .unwrap()
        })
        .collect::<Vec<_>>();
    let mut compared = 0;
    for cached in [false, true] {
        for (i, map) in maps.iter().enumerate() {
            let view = if cached {
                ProjectionMap::new(map)
            } else {
                ProjectionMap::reference(map)
            };
            for (batch, expected) in batches[&map.id].iter().zip(&expected_routes[i]) {
                assert_eq!(
                    &view.route(batch),
                    expected,
                    "cached={cached} map={} batch={}",
                    map.id,
                    batch.batch_id
                );
                assert_eq!(&view.route(batch), expected);
                compared += 2;
            }
            assert_eq!(format!("{:?}", view.stages()), expected_stages[i]);
            for station in [LAM, CUT, CUT2, "missing"] {
                assert_eq!(
                    view.previous_stations(station),
                    chain::previous_work_stage_stations(map, station)
                );
                assert_eq!(
                    view.next_stations(station),
                    chain::next_work_stage_stations(map, station)
                );
                assert_eq!(
                    view.previous_unavailable(station),
                    chain::previous_stage_resolution_is_unavailable(map, station)
                );
                for node in ["lam1", "cut1", "cut2", ""] {
                    assert_eq!(
                        format!("{:?}", view.work_stage(station, node)),
                        format!("{:?}", chain::work_stage_for_station(map, station, node))
                    );
                }
            }
            let inputs = stage_execution::work_inputs_projected(&view, &batches[&map.id], &[]);
            let statuses = serde_json::to_value(stage_execution::stage_work_statuses_projected(
                &view,
                &[],
                &inputs,
                &BTreeMap::new(),
                &[],
            ))
            .unwrap();
            assert_eq!(
                statuses, expected_statuses[i],
                "stage status changed in cached={cached}"
            );
        }
    }
    println!(
        "route comparisons={compared}; maps=6; WIP variants={}",
        batches.values().map(Vec::len).sum::<usize>()
    );
}

#[test]
fn snapshot_equality_parallel_readers_and_invalidation() {
    tokio::runtime::Builder::new_multi_thread().worker_threads(4).enable_all().build().unwrap().block_on(async {
        let store = Arc::new(pm::MemoryProductionMapStore::new());
        for index in 0..12 {
            let map = fixture_map(index,if index % 3 == 0 { 4 } else { 0 });
            pm::ProductionMapStorePort::put_map(store.as_ref(),map.clone()).await.unwrap();
            for j in 0..4 {
                let mut b = fixture_batch(&map.id,j);
                if j == 2 {
                    b.wip_status = pm::OrderProgressBatchWipStatus::InUse;
                    b.used_by_apparatus = CUT2.into();
                    b.used_by_session_id = "consumer".into();
                }
                if j == 3 {
                    b.wip_status = pm::OrderProgressBatchWipStatus::Processed;
                    b.processed_by_apparatus = CUT.into();
                    b.processed_by_session_id = "closed-consumer".into();
                }
                pm::ProductionMapStorePort::put_order_progress_batch(store.as_ref(),b).await.unwrap();
            }
            let mut session = pm::OrderRunSession {
                session_id:format!("lam-session-{index}"),apparatus:LAM.into(),order_id:map.id.clone(),stage_node_id:"lam1".into(),
                status:pm::OrderRunStatus::Completed,worker_role:"aparatchi".into(),worker_ref:"worker".into(),worker_display_name:"Worker".into(),
                started_at_unix:1,updated_at_unix:2,payload_json:json!({"stage_work_protocol":1}),
            };
            stage_execution::stamp_work_report(&mut session,"report",1,&pm::QueueActionActor::default(),2);
            pm::ProductionMapStorePort::put_order_run_session(store.as_ref(),session).await.unwrap();
        }
        let resolver = Arc::new(pm::TestCanonicalApparatusResolver::standard());
        let mut baseline = None;
        for cached in [false, true] {
            let service = pm::ProductionMapService::new(store.clone(),resolver.clone());
            let service = Arc::new(if cached { service } else { service.with_reference_projection() });
            let (admin,_) = service.live_snapshot_shared_with_revision().await.unwrap();
            let admin_json = serde_json::to_value(admin.as_ref()).unwrap();
            let worker = service.worker_snapshot_shared_with_revision(&[CUT.into()],&[]).await.unwrap().0;
            let worker_json = serde_json::to_value(worker.as_ref()).unwrap();
            if let Some((old_admin,old_worker)) = &baseline {
                assert_eq!(&admin_json,old_admin,"admin JSON differs in cached={cached}");
                assert_eq!(&worker_json,old_worker,"worker JSON differs in cached={cached}");
            } else { baseline = Some((admin_json.clone(),worker_json)); }
            let warm = service.live_snapshot_shared_with_revision().await.unwrap().0;
            assert!(Arc::ptr_eq(&admin,&warm));
            service.notify_live();
            let mut readers = tokio::task::JoinSet::new();
            for _ in 0..8 {
                let service = service.clone();
                readers.spawn(async move { service.live_snapshot_shared_with_revision().await.unwrap() });
            }
            let mut results = Vec::new();
            while let Some(result) = readers.join_next().await { results.push(result.unwrap()); }
            for (snapshot,rev) in &results {
                assert_eq!(*rev,results[0].1);
                assert!(Arc::ptr_eq(snapshot,&results[0].0));
                assert_eq!(serde_json::to_value(snapshot.as_ref()).unwrap(),admin_json);
            }
            let mut changed = fixture_map(0,4);
            changed.title = format!("Edited in cached={cached}");
            pm::ProductionMapStorePort::put_map(store.as_ref(),changed).await.unwrap();
            service.notify_live();
            let updated = service.live_snapshot_shared_with_revision().await.unwrap().0;
            assert_ne!(serde_json::to_value(updated.as_ref()).unwrap(),admin_json);
            pm::ProductionMapStorePort::put_map(store.as_ref(),fixture_map(0,4)).await.unwrap();
        }
        println!("reference/production admin+worker JSON equality; concurrent readers; invalidation after edits");
    });
}

#[test]
fn cache_lifetime_thread_handoff_and_capacity() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ProjectionMap<'_>>();
    let mut map = fixture_map(0, 0);
    let old = {
        let view = ProjectionMap::new(&map);
        let old = view.stages();
        let map_ref = &map;
        std::thread::scope(|scope| {
            let handle = scope.spawn(move || {
                assert_eq!(view.stages(), old);
                // Many stale route identities must not retain unbounded state.
                for i in 0..400 {
                    let mut batch = fixture_batch(&map_ref.id, i);
                    batch.payload_json["stage_node_id"] = json!(format!("missing-{i}"));
                    assert_eq!(
                        view.route(&batch),
                        wip_route::resolve_wip_input_route(map_ref, &batch)
                    );
                }
                assert_eq!(view.routes.lock().unwrap().len(), MAX_ROUTE_CLASSES);
                for i in 0..400 {
                    view.next_stages(&format!("missing-{i}"));
                }
                assert!(view.adjacent.lock().unwrap().is_empty());
                view
            });
            let view = handle.join().unwrap();
            assert_eq!(view.stages(), chain::linear_work_stages(&map));
            view.stages()
        })
    };
    map.nodes[1].title = "Changed after old projection".into();
    let view = ProjectionMap::new(&map);
    assert_ne!(old, view.stages());
    assert!(view.routes.lock().unwrap().is_empty());
    let mut oversized = fixture_batch(&map.id, 0);
    oversized.payload_json["wip_route_binding"] =
        json!({"arbitrary": "x".repeat(MAX_ROUTE_KEY_BYTES * 2)});
    assert_eq!(
        view.route(&oversized),
        wip_route::resolve_wip_input_route(&map, &oversized)
    );
    assert!(view.routes.lock().unwrap().is_empty());
}

#[tokio::test]
#[ignore = "manual synthetic latency benchmark; no timing assertion"]
async fn projection_snapshot_latency_benchmark() {
    let store = Arc::new(pm::MemoryProductionMapStore::new());
    for i in 0..64 {
        let map = fixture_map(i, 0);
        pm::ProductionMapStorePort::put_map(store.as_ref(), map.clone())
            .await
            .unwrap();
        for j in 0..32 {
            let mut batch = fixture_batch(&map.id, j);
            if j % 2 == 0 {
                batch.wip_status = pm::OrderProgressBatchWipStatus::Processed;
            }
            pm::ProductionMapStorePort::put_order_progress_batch(store.as_ref(), batch)
                .await
                .unwrap();
        }
    }
    let resolver = Arc::new(pm::TestCanonicalApparatusResolver::standard());
    let mut expected = None;
    for pass in 0..3 {
        let order = if pass % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        };
        for cached in order {
            let service = pm::ProductionMapService::new(store.clone(), resolver.clone());
            let service = if cached {
                service
            } else {
                service.with_reference_projection()
            };
            let started = std::time::Instant::now();
            let snapshot = service
                .live_snapshot_shared_with_revision()
                .await
                .unwrap()
                .0;
            let elapsed = started.elapsed().as_secs_f64() * 1000.;
            let value = serde_json::to_value(snapshot.as_ref()).unwrap();
            if let Some(old) = &expected {
                assert_eq!(&value, old);
            } else {
                expected = Some(value);
            }
            println!(
                "{}",
                json!({"phase":"synthetic_admin_snapshot","pass":pass,
                "optimized":cached,"elapsed_ms":elapsed,"maps":64,"batches":2048,
                "controls":snapshot.queue_action_controls.values().map(BTreeMap::len).sum::<usize>(),
                "exact_json_equal":true})
            );
        }
    }
}
