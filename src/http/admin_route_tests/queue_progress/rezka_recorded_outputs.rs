use super::*;
use crate::core::production_map::{
    ProductionMapError, QueueActionActor, QueueProgressInput, queue_state,
};

async fn verify_recorded_rezka_outputs(intermediate: bool, all_individual: bool) {
    let requests = Arc::new(Mutex::new(Vec::<ScaleDriverPrintRequest>::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new().with_driver(Arc::new(FakeProgressDriver {
        requests: requests.clone(),
        fail: false,
    }));
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "recorded-rezka-worker".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["apparatus:default:asset-010".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .unwrap();
    let admin = session(&state, PrincipalRole::Admin).await;
    let worker = session_for(&state, PrincipalRole::Aparatchi, "recorded-rezka-worker").await;
    let service = state.production_maps.clone();
    let router = build_router(state);
    let apparatus = "apparatus:default:asset-010";
    let order = "zakaz-rezka-recorded";
    let mut map = serde_json::json!({
        "id": order, "product_code": "REZKA-RECORDED", "title": "Recorded rolls", "order_number": "9501",
        "nodes": [
            {"id":"start", "kind":"start", "title":"Start"},
            {"id":"rezka", "kind":"apparatus", "title":"Rezka", "apparatus_id":apparatus,
             "rezka_kadr_count":3, "rezka_frame_groups":[1,2]},
            {"id":"end", "kind":"end", "title":"End"}
        ],
        "edges":[{"from":"start","to":"rezka"},{"from":"rezka","to":"end"}]
    });
    if intermediate {
        map["nodes"].as_array_mut().unwrap().insert(2, serde_json::json!({
            "id":"lam", "kind":"apparatus", "title":"Laminatsiya", "apparatus_id":"apparatus:default:asset-007"
        }));
        map["edges"][1]["to"] = serde_json::json!("lam");
        map["edges"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"from":"lam","to":"end"}));
    }
    let response = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin,
            &map.to_string(),
        ))
        .await
        .unwrap();
    let status = response.status();
    assert_eq!(status, StatusCode::OK, "{:?}", json_body(response).await);
    let (status, start) = super::rezka::queue_action_json(
        &router,
        &worker,
        serde_json::json!({
            "apparatus":apparatus,"order_id":order,"action":"start"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{start:?}");
    let cycle = start["session"]["session_id"].as_str().unwrap();
    let count = if intermediate { 2 } else { 3 };
    let frames: Vec<_> = (0..count)
        .map(|i| {
            serde_json::json!({
                "produced_qty":100.0 + i as f64, "finished_goods_meter":100.0 + i as f64,
                "gross_qty":10.0 + i as f64, "finished_goods_kg":10.0 + i as f64,
                "bobina_kg":0.5, "diameter":45.0 + i as f64,
            })
        })
        .collect();
    let stale_write = service
        .prepare_apparatus_queue_action_with_progress(
            apparatus,
            order,
            queue_state::ApparatusQueueAction::RollComplete,
            &[apparatus.to_string()],
            QueueActionActor {
                role: "aparatchi".to_string(),
                ref_: "recorded-rezka-worker".to_string(),
                display_name: "Rezka worker".to_string(),
            },
            QueueProgressInput {
                rezka_record_frame_index: Some(1),
                rezka_output_cycle: cycle.to_string(),
                rezka_frames: vec![serde_json::from_value(frames[0].clone()).unwrap()],
                ..QueueProgressInput::default()
            },
        )
        .await
        .unwrap();
    let mut recorded_ids = Vec::new();
    // Start from the second physical roll: indices must not depend on print order.
    let indices = if all_individual {
        if intermediate {
            vec![2, 1]
        } else {
            vec![2, 1, 3]
        }
    } else {
        vec![2]
    };
    for index in indices {
        let input = serde_json::json!({
            "apparatus":apparatus,"order_id":order,"action":"roll_complete",
            "rezka_output_cycle":cycle,"rezka_record_frame_index":index,
            "rezka_frames":[frames[index - 1]], "uom":"m"
        });
        let (status, saved) =
            super::rezka::queue_action_json(&router, &worker, input.clone()).await;
        assert_eq!(status, StatusCode::OK, "{saved:?}");
        assert_eq!(saved["states"][order], "in_progress");
        assert_eq!(saved["session"]["status"], "active");
        assert_eq!(saved["progress_batches"].as_array().unwrap().len(), 1);
        assert_eq!(
            saved["prints"].as_array().unwrap().len(),
            0,
            "save must not dispatch print"
        );
        let batch = &saved["progress_batch"];
        assert_eq!(batch["payload_json"]["rezka_frame_index"], index);
        assert_eq!(
            batch["payload_json"]["contained_kadr_count"],
            if intermediate && index == 2 { 2 } else { 1 }
        );
        assert_eq!(
            batch["payload_json"]["rezka_output_kind"],
            if intermediate {
                "grouped_roll"
            } else {
                "frame"
            }
        );
        assert_eq!(batch["produced_qty"], frames[index - 1]["produced_qty"]);
        recorded_ids.push(batch["batch_id"].clone());
        let (status, replay) =
            super::rezka::queue_action_json(&router, &worker, input.clone()).await;
        assert_eq!(status, StatusCode::OK, "{replay:?}");
        assert_eq!(replay["progress_batches"].as_array().unwrap().len(), 0);
        assert_eq!(
            replay["session"]["payload_json"]["rezka_output_report"]
                .as_array()
                .unwrap()
                .len(),
            recorded_ids.len()
        );
        let mut edited = input;
        edited["rezka_frames"][0]["gross_qty"] = serde_json::json!(999);
        let (status, _) = super::rezka::queue_action_json(&router, &worker, edited).await;
        assert_eq!(status, StatusCode::CONFLICT);
        let response = router.clone().oneshot(request_with_body("POST",
            "/v1/mobile/admin/production-maps/progress-qr/reprint", &worker,
            &serde_json::json!({"progress_batch_id":batch["batch_id"], "qr_payload":batch["qr_payload"],
                "printer":"zebra", "print_mode":"rfid", "print_transport":"offline"}).to_string(),
        )).await.unwrap();
        let status = response.status();
        let print = json_body(response).await;
        assert_eq!(status, StatusCode::OK, "{print:?}");
        assert_eq!(print["batch"]["qr_payload"], batch["qr_payload"]);
    }
    let response = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &worker,
        ))
        .await
        .unwrap();
    assert!(
        matches!(
            service.commit_prepared_queue_action(stale_write).await,
            Err(ProductionMapError::RezkaOutputCycleConflict)
        ),
        "stale writer must not overwrite another saved slot"
    );
    let snapshot = json_body(response).await;
    let snapshot_text = snapshot.to_string();
    assert!(snapshot_text.contains("rezka_output_report") && snapshot_text.contains(cycle));
    let (status, _) = super::rezka::queue_action_json(
        &router,
        &worker,
        serde_json::json!({
            "apparatus":apparatus,"order_id":order,"action":"complete", "rezka_output_cycle":cycle,
            "rezka_frames":[frames[0]], "total_waste":1.5
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "incomplete group cannot finish"
    );
    let (status, _) = super::rezka::queue_action_json(
        &router,
        &worker,
        serde_json::json!({
            "apparatus":apparatus,"order_id":order,"action":"merge", "qr_payload":"another-input"
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "finish the printed output group before changing lineage"
    );
    let action = if intermediate { "pause" } else { "complete" };
    let (status, finished) = super::rezka::queue_action_json(&router, &worker, serde_json::json!({
        "apparatus":apparatus,"order_id":order,"action":action, "rezka_output_cycle":cycle,
        "rezka_frames":frames, "total_waste":1.5, "printer":"zebra", "print_mode":"rfid", "print_transport":"offline"
    })).await;
    assert_eq!(status, StatusCode::OK, "{finished:?}");
    assert_eq!(
        finished["progress_batches"].as_array().unwrap().len(),
        count - recorded_ids.len()
    );
    assert_eq!(
        finished["prints"].as_array().unwrap().len(),
        count - recorded_ids.len()
    );
    if all_individual {
        assert_eq!(finished["progress_event"]["produced_qty"], 0.0);
        assert_eq!(
            finished["progress_event"]["payload_json"]["rezka_previously_recorded_batches"]
                .as_array()
                .unwrap()
                .len(),
            count
        );
    }
    assert_eq!(
        finished["session"]["payload_json"]["rezka_output_report"],
        serde_json::json!([])
    );
    if intermediate {
        let (status, _) = super::rezka::queue_action_json(
            &router,
            &worker,
            serde_json::json!({
                "apparatus":apparatus,"order_id":order,"action":"resume"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = super::rezka::queue_action_json(&router, &worker, serde_json::json!({
            "apparatus":apparatus,"order_id":order,"action":"roll_complete", "rezka_output_cycle":cycle,
            "rezka_record_frame_index":1, "rezka_frames":[frames[0]]
        })).await;
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "old dialog cannot create a roll in a new cycle"
        );
    }
    let response = router.clone().oneshot(request("GET",
        "/v1/mobile/admin/production-maps/wip-batches?apparatus=apparatus%3Adefault%3Aasset-010&status=all&order_id=zakaz-rezka-recorded", &admin)).await.unwrap();
    let stored = json_body(response).await;
    let batches = stored["batches"].as_array().unwrap();
    assert_eq!(
        batches.len(),
        count,
        "no duplicate production or WIP on finalization"
    );
    for id in recorded_ids {
        assert!(batches.iter().any(|batch| batch["batch_id"] == id));
    }
    if !intermediate {
        assert_eq!(
            batches
                .iter()
                .filter_map(|batch| batch["total_waste"].as_f64())
                .sum::<f64>(),
            1.5
        );
    }
    assert!(
        requests.lock().await.is_empty(),
        "offline preparation never prints on the server"
    );
}

#[tokio::test]
async fn final_rezka_individual_rolls_complete_without_duplicate_batches_or_prints() {
    verify_recorded_rezka_outputs(false, true).await;
}

#[tokio::test]
async fn intermediate_rezka_recorded_group_and_bulk_pause_keep_correct_rolls() {
    verify_recorded_rezka_outputs(true, false).await;
}

#[tokio::test]
async fn intermediate_rezka_all_individual_rolls_pause_and_resume_without_reusing_output() {
    verify_recorded_rezka_outputs(true, true).await;
}

async fn verify_recorded_rezka_issues(intermediate: bool, all_issues: bool) {
    let requests = Arc::new(Mutex::new(Vec::<ScaleDriverPrintRequest>::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new().with_driver(Arc::new(FakeProgressDriver {
        requests: requests.clone(),
        fail: false,
    }));
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "recorded-rezka-worker".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["apparatus:default:asset-010".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .unwrap();
    let admin = session(&state, PrincipalRole::Admin).await;
    let worker = session_for(&state, PrincipalRole::Aparatchi, "recorded-rezka-worker").await;
    let router = build_router(state);
    let apparatus = "apparatus:default:asset-010";
    let order = "zakaz-rezka-recorded";
    let mut map = serde_json::json!({
        "id": order, "product_code": "REZKA-RECORDED", "title": "Recorded rolls", "order_number": "9501",
        "nodes": [
            {"id":"start", "kind":"start", "title":"Start"},
            {"id":"rezka", "kind":"apparatus", "title":"Rezka", "apparatus_id":apparatus,
             "rezka_kadr_count":3, "rezka_frame_groups":[1,2]},
            {"id":"end", "kind":"end", "title":"End"}
        ],
        "edges":[{"from":"start","to":"rezka"},{"from":"rezka","to":"end"}]
    });
    if intermediate {
        map["nodes"].as_array_mut().unwrap().insert(2, serde_json::json!({
            "id":"lam", "kind":"apparatus", "title":"Laminatsiya", "apparatus_id":"apparatus:default:asset-007"
        }));
        map["edges"][1]["to"] = serde_json::json!("lam");
        map["edges"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"from":"lam","to":"end"}));
    }
    let response = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin,
            &map.to_string(),
        ))
        .await
        .unwrap();
    let status = response.status();
    assert_eq!(status, StatusCode::OK, "{:?}", json_body(response).await);
    let (status, start) = super::rezka::queue_action_json(
        &router,
        &worker,
        serde_json::json!({
            "apparatus":apparatus,"order_id":order,"action":"start"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{start:?}");
    let cycle = start["session"]["session_id"].as_str().unwrap();
    let count = if intermediate { 2 } else { 3 };

    let healthy = serde_json::json!({
        "produced_qty":120.0,"gross_qty":12.0,"bobina_kg":0.5,"diameter":45.0
    });
    let issue = serde_json::json!({"issue_note":"Kadr yirtilgan"});
    let frames: Vec<_> = (0..count).map(|index|
        if all_issues || index == 0 { issue.clone() } else { healthy.clone() }
    ).collect();
    // Save the issue last, after a healthy roll, including a grouped two-kadr roll.
    let indices: Vec<_> = if all_issues { (1..=count).rev().collect() } else { vec![2, 1] };
    for (saved_count, index) in indices.into_iter().enumerate() {
        let input = serde_json::json!({
            "apparatus":apparatus,"order_id":order,"action":"roll_complete",
            "rezka_output_cycle":cycle,"rezka_record_frame_index":index,
            "rezka_frames":[frames[index - 1]],"uom":"m"
        });
        let (status, saved) = super::rezka::queue_action_json(&router,&worker,input.clone()).await;
        assert_eq!(status, StatusCode::OK, "{saved:?}");
        assert_eq!(saved["states"][order], "in_progress");
        assert_eq!(saved["session"]["status"], "active");
        assert!(saved["prints"].as_array().unwrap().is_empty());
        let report = saved["session"]["payload_json"]["rezka_output_report"].as_array().unwrap();
        assert_eq!(report.len(), saved_count + 1);
        let slot = report.iter().find(|slot| slot["frame_index"] == index).unwrap();
        if all_issues || index == 1 {
            assert!(saved["progress_batches"].as_array().unwrap().is_empty());
            assert_eq!(slot["batch_id"], "");
            assert_eq!(slot["qr_payload"], "");
            assert_eq!(slot["input"]["issue_note"], "Kadr yirtilgan");
            assert_eq!(saved["progress_event"]["produced_qty"], 0.0);
            assert_eq!(saved["progress_event"]["payload_json"]["rezka_frame_issues"][0]["frame_index"], index);
        }
        let (status,replay) = super::rezka::queue_action_json(&router,&worker,input.clone()).await;
        assert_eq!(status,StatusCode::OK,"{replay:?}");
        assert!(replay["progress_batches"].as_array().unwrap().is_empty());
        assert_eq!(replay["session"]["payload_json"]["rezka_output_report"].as_array().unwrap().len(),saved_count+1);
        let mut converted = input;
        converted["rezka_frames"][0] = if all_issues || index == 1 {healthy.clone()} else {issue.clone()};
        let (status, _) = super::rezka::queue_action_json(&router,&worker,converted).await;
        assert_eq!(status,StatusCode::CONFLICT,"saved cards are immutable");
    }
    let response = router.clone().oneshot(request("GET",
        "/v1/mobile/admin/production-maps/sequence", &worker)).await.unwrap();
    let snapshot = json_body(response).await.to_string();
    assert!(snapshot.contains("Kadr yirtilgan") && snapshot.contains(cycle));
    let action = if intermediate {"pause"} else {"complete"};
    let (status, finished) = super::rezka::queue_action_json(&router,&worker,serde_json::json!({
        "apparatus":apparatus,"order_id":order,"action":action,
        "rezka_output_cycle":cycle,"rezka_frames":frames,"total_waste":1.5,
        "printer":"zebra","print_mode":"rfid","print_transport":"offline"
    })).await;
    assert_eq!(status,StatusCode::OK,"{finished:?}");
    let remaining_healthy = if all_issues || intermediate {0} else {1};
    assert_eq!(finished["progress_batches"].as_array().unwrap().len(),remaining_healthy);
    assert_eq!(finished["prints"].as_array().unwrap().len(),remaining_healthy);
    assert_eq!(finished["progress_event"]["payload_json"]["rezka_frame_issues"].as_array().unwrap().len(),
        if all_issues {count} else {1});
    assert_eq!(finished["session"]["payload_json"]["rezka_output_report"],serde_json::json!([]));
    assert!(finished["progress_event"]["payload_json"]["rezka_previously_recorded_batches"]
        .as_array().unwrap().iter().all(|batch| batch.as_str().unwrap() != ""));
    if intermediate {
        let (status,resumed) = super::rezka::queue_action_json(&router,&worker,serde_json::json!({
            "apparatus":apparatus,"order_id":order,"action":"resume"
        })).await;
        assert_eq!(status,StatusCode::OK,"{resumed:?}");
        assert_eq!(resumed["states"][order],"in_progress");
        let (status,_) = super::rezka::queue_action_json(&router,&worker,serde_json::json!({
            "apparatus":apparatus,"order_id":order,"action":"roll_complete",
            "rezka_output_cycle":cycle,"rezka_record_frame_index":1,"rezka_frames":[issue]
        })).await;
        assert_eq!(status,StatusCode::CONFLICT);
    }
    let response = router.clone().oneshot(request("GET",
        "/v1/mobile/admin/production-maps/wip-batches?apparatus=apparatus%3Adefault%3Aasset-010&status=all&order_id=zakaz-rezka-recorded", &admin)).await.unwrap();
    let stored = json_body(response).await;
    let batches = stored["batches"].as_array().unwrap();
    assert_eq!(batches.len(), if all_issues {0} else {count-1},"issues must never produce WIP");
    if !intermediate {
        if all_issues {
            assert_eq!(finished["progress_event"]["total_waste"],1.5);
        } else {
            assert_eq!(finished["progress_event"]["total_waste"],1.5);
            assert_eq!(finished["progress_event"]["produced_qty"],120.0);
            assert_eq!(batches.iter().filter_map(|batch|batch["total_waste"].as_f64()).sum::<f64>(),1.5,
                "the first healthy roll retains waste even when card 1 is an issue");
        }
    }
    assert!(requests.lock().await.is_empty());
}

#[tokio::test]
async fn intermediate_rezka_last_card_issue_detaches_and_resumes() {
    verify_recorded_rezka_issues(true,false).await;
}
#[tokio::test]
async fn intermediate_rezka_all_cards_issue_detaches_and_resumes_without_qr() {
    verify_recorded_rezka_issues(true,true).await;
}
#[tokio::test]
async fn final_rezka_saved_issue_and_printed_and_pending_rolls_complete() {
    verify_recorded_rezka_issues(false,false).await;
}
#[tokio::test]
async fn final_rezka_all_cards_issue_completes_without_qr_and_keeps_waste() {
    verify_recorded_rezka_issues(false,true).await;
}
