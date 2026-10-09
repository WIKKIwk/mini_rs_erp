use super::*;
use crate::core::production_map::ProductionMapStorePort;

const APPARATUS: &str = "apparatus:default:asset-010";
const ORDER: &str = "zakaz-rezka-prepared-label";

struct PreparedLabelFixture {
    router: axum::Router,
    admin: String,
    worker: String,
    cycle: String,
    store: Arc<MemoryProductionMapStore>,
    driver_requests: Arc<Mutex<Vec<ScaleDriverPrintRequest>>>,
}

impl PreparedLabelFixture {
    async fn new(driver_fails: bool) -> Self {
        let store = Arc::new(MemoryProductionMapStore::new());
        let driver_requests = Arc::new(Mutex::new(Vec::new()));
        let mut state = test_state();
        state.production_maps = production_map_service_with_store(&state, store.clone());
        state.gscale = GscaleService::new().with_driver(Arc::new(FakeProgressDriver {
            requests: driver_requests.clone(),
            fail: driver_fails,
        }));
        state
            .admin
            .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
                principal_role: PrincipalRole::Aparatchi,
                principal_ref: "prepared-label-worker".into(),
                role_id: "aparatchi".into(),
                assigned_apparatus: vec![APPARATUS.into()],
                assigned_item_groups: vec![],
            })
            .await
            .unwrap();
        let admin = session(&state, PrincipalRole::Admin).await;
        let worker = session_for(&state, PrincipalRole::Aparatchi, "prepared-label-worker").await;
        let router = build_router(state);
        let saved = router
            .clone()
            .oneshot(request_with_body(
                "PUT",
                "/v1/mobile/admin/production-maps",
                &admin,
                &pechat_order_map_json(ORDER, "Prepared label rolls", "9531", APPARATUS),
            ))
            .await
            .unwrap();
        let status = saved.status();
        assert_eq!(status, StatusCode::OK, "{:?}", json_body(saved).await);
        let (status, started) = super::rezka::queue_action_json(
            &router,
            &worker,
            serde_json::json!({"apparatus":APPARATUS,"order_id":ORDER,"action":"start"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{started:?}");
        Self {
            router,
            admin,
            worker,
            cycle: started["session"]["session_id"].as_str().unwrap().into(),
            store,
            driver_requests,
        }
    }

    fn frame_request(&self, transport: &str) -> serde_json::Value {
        serde_json::json!({
            "apparatus":APPARATUS,"order_id":ORDER,"action":"roll_complete",
            "rezka_output_cycle":self.cycle,"rezka_record_frame_index":2,
            "rezka_frames":[{
                "produced_qty":125.5,"finished_goods_meter":125.5,
                "gross_qty":12.75,"finished_goods_kg":12.75,
                "bobina_kg":0.75,"diameter":45.5,
            }],
            "uom":"m","driver_url":"http://printer.test",
            "printer":"xp-p323b","print_mode":"label","print_count":9,
            "print_transport":transport,
        })
    }

    async fn stored_batches(&self) -> Vec<serde_json::Value> {
        let response = self
            .router
            .clone()
            .oneshot(request(
                "GET",
                &format!("/v1/mobile/admin/production-maps/wip-batches?apparatus={APPARATUS}&status=all&order_id={ORDER}"),
                &self.admin,
            ))
            .await
            .unwrap();
        let status = response.status();
        let body = json_body(response).await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        body["batches"].as_array().unwrap().clone()
    }

    async fn reprint(
        &self,
        batch: &serde_json::Value,
        transport: &str,
    ) -> (StatusCode, serde_json::Value) {
        let response = self
            .router
            .clone()
            .oneshot(request_with_body(
                "POST",
                "/v1/mobile/admin/production-maps/progress-qr/reprint",
                &self.worker,
                &serde_json::json!({
                    "progress_batch_id":batch["batch_id"],"qr_payload":batch["qr_payload"],
                    "driver_url":"http://printer.test","printer":"xp-p323b",
                    "print_mode":"label","print_count":1,"print_transport":transport,
                })
                .to_string(),
            ))
            .await
            .unwrap();
        (response.status(), json_body(response).await)
    }
}

fn assert_prepared_label_matches_batch(saved: &serde_json::Value) {
    let batch = &saved["progress_batch"];
    let print = &saved["print"];
    assert_eq!(saved["states"][ORDER], "in_progress");
    assert_eq!(saved["session"]["status"], "active");
    assert_eq!(saved["progress_batches"].as_array().unwrap().len(), 1);
    assert_eq!(saved["prints"], serde_json::json!([print]));
    assert_eq!(print["ok"], true);
    assert_eq!(print["status"], "prepared");
    assert_eq!(print["qr_payload"], batch["qr_payload"]);
    assert!(!print["qr_payload"].as_str().unwrap().is_empty());
    assert_eq!(print["apparatus"], APPARATUS);
    assert_eq!(print["label_kind"], "progress");
    assert_eq!(print["printer"], "xp-p323b");
    assert_eq!(print["print_mode"], "label");
    assert_eq!(print["print_count"], 1, "one label for one saved roll");
    assert_eq!(print["qty"], 125.5);
    assert_eq!(print["gross_qty"], 12.75);
    assert_eq!(print["tare_enabled"], true);
    assert_eq!(print["tare_kg"], 0.75);
    assert_eq!(print["unit"], "kg");
    assert_eq!(print["progress_unit"], "m");
    assert_eq!(batch["payload_json"]["rezka_frame_index"], 2);
    let slot = &saved["session"]["payload_json"]["rezka_output_report"][0];
    assert_eq!(slot["frame_index"], 2);
    assert_eq!(slot["batch_id"], batch["batch_id"]);
    assert_eq!(slot["qr_payload"], print["qr_payload"]);
    assert_eq!(slot["input"]["gross_qty"], 12.75);
    assert_eq!(slot["input"]["diameter"], 45.5);
}

#[tokio::test]
async fn single_rezka_frame_prints_without_paddon_and_replay_creates_nothing() {
    // Even an unavailable server printer cannot affect a local prepared label.
    let fixture = PreparedLabelFixture::new(true).await;
    let actor = crate::core::production_map::QueueActionActor {
        role: "aparatchi".into(),
        ref_: "prepared-label-worker".into(),
        display_name: String::new(),
    };
    assert_eq!(
        fixture.store.active_rezka_paddon(APPARATUS, &actor).await.unwrap(),
        None,
    );
    let mut input = fixture.frame_request("offline");
    input["output_paddon_code"] = serde_json::json!("stale-device-paddon");
    let (status, saved) =
        super::rezka::queue_action_json(&fixture.router, &fixture.worker, input.clone()).await;
    assert_eq!(status, StatusCode::OK, "{saved:?}");
    assert_prepared_label_matches_batch(&saved);
    let stored = fixture.stored_batches().await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0]["batch_id"], saved["progress_batch"]["batch_id"]);
    assert_eq!(stored[0]["qr_payload"], saved["print"]["qr_payload"]);
    assert!(fixture.driver_requests.lock().await.is_empty());

    let (status, replay) =
        super::rezka::queue_action_json(&fixture.router, &fixture.worker, input.clone()).await;
    assert_eq!(status, StatusCode::OK, "{replay:?}");
    assert_eq!(replay["progress_batches"], serde_json::json!([]));
    assert_eq!(replay["prints"], serde_json::json!([]));
    assert!(replay["print"].is_null());
    assert_eq!(fixture.stored_batches().await.len(), 1);

    let (status, reprinted) = fixture.reprint(&saved["progress_batch"], "offline").await;
    assert_eq!(status, StatusCode::OK, "{reprinted:?}");
    assert_eq!(reprinted["print"], saved["print"]);
    assert_eq!(fixture.stored_batches().await.len(), 1);
    assert!(fixture.driver_requests.lock().await.is_empty());

    let mut conflicting = input;
    conflicting["rezka_frames"][0]["gross_qty"] = serde_json::json!(99.0);
    let (status, rejected) =
        super::rezka::queue_action_json(&fixture.router, &fixture.worker, conflicting).await;
    assert_eq!(status, StatusCode::CONFLICT, "{rejected:?}");
    assert!(rejected["print"].is_null());
    assert_eq!(fixture.stored_batches().await.len(), 1);
}

#[tokio::test]
async fn single_rezka_frame_wifi_waits_for_reprint_and_dispatches_exactly_once() {
    let fixture = PreparedLabelFixture::new(false).await;
    let (status, saved) = super::rezka::queue_action_json(
        &fixture.router,
        &fixture.worker,
        fixture.frame_request("wifi"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved:?}");
    assert_eq!(saved["prints"], serde_json::json!([]));
    assert!(saved["print"].is_null());
    tokio::task::yield_now().await;
    assert!(fixture.driver_requests.lock().await.is_empty());

    let (status, printed) = fixture.reprint(&saved["progress_batch"], "wifi").await;
    assert_eq!(status, StatusCode::OK, "{printed:?}");
    wait_for_progress_print_request_count(&fixture.driver_requests, 1).await;
    let requests = fixture.driver_requests.lock().await;
    assert_eq!(
        requests.len(),
        1,
        "save must not also dispatch a WiFi label"
    );
    assert_eq!(requests[0].epc, saved["progress_batch"]["qr_payload"]);
    assert_eq!(requests[0].gross_qty, 12.75);
    assert_eq!(requests[0].tare_kg, 0.75);
    assert_eq!(requests[0].qty, Some(125.5));
    assert_eq!(requests[0].print_count, 1);
    drop(requests);
    assert_eq!(fixture.stored_batches().await.len(), 1);
}

#[tokio::test]
async fn single_rezka_frame_local_issue_produces_neither_label_nor_wip() {
    let fixture = PreparedLabelFixture::new(false).await;
    let mut input = fixture.frame_request("offline");
    input["rezka_frames"] = serde_json::json!([{"issue_note":"Kadr yirtilgan"}]);
    let (status, saved) =
        super::rezka::queue_action_json(&fixture.router, &fixture.worker, input).await;
    assert_eq!(status, StatusCode::OK, "{saved:?}");
    assert_eq!(saved["prints"], serde_json::json!([]));
    assert!(saved["print"].is_null());
    assert_eq!(saved["progress_batches"], serde_json::json!([]));
    let slot = &saved["session"]["payload_json"]["rezka_output_report"][0];
    assert_eq!(slot["input"]["issue_note"], "Kadr yirtilgan");
    assert_eq!(slot["qr_payload"], "");
    assert!(fixture.stored_batches().await.is_empty());
    assert!(fixture.driver_requests.lock().await.is_empty());
}

#[tokio::test]
async fn single_rezka_frame_failed_commit_returns_no_prepared_label_or_saved_roll() {
    let fixture = PreparedLabelFixture::new(false).await;
    let input = fixture.frame_request("offline");
    fixture.store.fail_next_queue_progress_commit();
    let (status, failed) =
        super::rezka::queue_action_json(&fixture.router, &fixture.worker, input.clone()).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{failed:?}");
    assert!(failed["print"].is_null());
    assert!(fixture.stored_batches().await.is_empty());
    assert!(fixture.driver_requests.lock().await.is_empty());

    let (status, saved) =
        super::rezka::queue_action_json(&fixture.router, &fixture.worker, input).await;
    assert_eq!(status, StatusCode::OK, "{saved:?}");
    assert_prepared_label_matches_batch(&saved);
    assert_eq!(fixture.stored_batches().await.len(), 1);
    assert!(fixture.driver_requests.lock().await.is_empty());
}
