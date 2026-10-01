use super::*;
use crate::core::apparatus_standard::test_support::runtime_configuration;
use crate::core::production_map::{
    OrderProgressBatch, ProductionMapDefinition, ProductionMapStorePort,
    TestCanonicalApparatusResolver,
};
use serde_json::json;

const LAM: &str = "apparatus:default:asset-007";
const CUT: &str = "apparatus:default:asset-010";
const CUT2: &str = "apparatus:test:http-route-cut2";
const ORDER: &str = "zakaz-http-old-wip-route";
const QR: &str = "400118DA2F17C3617F59DDC6";
const LOOKUP: &str = "/v1/mobile/admin/production-maps/progress-qr/lookup";

async fn fixture(case: &str) -> (AppState, Arc<MemoryProductionMapStore>, OrderProgressBatch) {
    let mut state = test_state();
    let store = Arc::new(MemoryProductionMapStore::new());
    state.production_maps = ProductionMapService::new(
        store.clone(),
        Arc::new(TestCanonicalApparatusResolver::new([
            runtime_configuration(TestApparatusSpec::laminate(LAM, "Laminatsiya1")),
            runtime_configuration(TestApparatusSpec::cut(CUT, "Rezka")),
            runtime_configuration(TestApparatusSpec::cut(CUT2, "Rezka2")),
        ])),
    );
    let mut map: ProductionMapDefinition = serde_json::from_value(json!({
        "id": ORDER, "product_code": "lazer", "title": "Lazer guruch uzun don 2 kg",
        "nodes": [
            {"id":"start", "kind":"start", "title":"Start"},
            {"id":"lam1", "kind":"apparatus", "title":"Laminatsiya1", "apparatus_id":LAM},
            {"id":"apparatus_6", "kind":"apparatus", "title":"Rezka", "apparatus_id":CUT, "alternative_group_id":"alt_cut_6", "rezka_kadr_count":1},
            {"id":"apparatus_7", "kind":"apparatus", "title":"Rezka2", "apparatus_id":CUT2, "alternative_group_id":"alt_cut_6", "rezka_kadr_count":1},
            {"id":"end", "kind":"end", "title":"End"}
        ],
        "edges": [{"from":"start", "to":"lam1"}, {"from":"lam1", "to":"apparatus_6"},
            {"from":"lam1", "to":"apparatus_7"}, {"from":"apparatus_6", "to":"end"}, {"from":"apparatus_7", "to":"end"}]
    })).unwrap();
    if case == "ambiguous" {
        map.nodes[3].alternative_group_id = "separate-work".into();
    }
    // This represents already edited legacy storage. The QR endpoint must not
    // need a save/migration or a new label to project its destination.
    store.put_map(map).await.unwrap();
    let mut batch: OrderProgressBatch = serde_json::from_value(json!({
        "batch_id":"http-physical-old-roll", "session_id":"lam-session", "started_at_unix":1, "completed_at_unix":2,
        "apparatus":LAM, "order_id":ORDER, "action":"detach_roll", "status":"completed",
        "produced_qty":6170.0, "uom":"m", "qr_payload":QR,
        "label_item_code":"lazer", "label_item_name":"Lazer", "executor_name":"Lam worker",
        "worker_role":"aparatchi", "worker_ref":"lam-worker", "worker_display_name":"Lam worker",
        "wip_status":"waiting", "current_apparatus":LAM, "next_apparatus":CUT,
        "payload_json":{"stage_node_id":"lam1", "next_stage_node_id":"rezka_5"}
    })).unwrap();
    if case == "missing_source" {
        batch.payload_json["stage_node_id"] = json!("deleted-lam1");
    }
    store.put_order_progress_batch(batch.clone()).await.unwrap();
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "route-cut2-worker".into(),
            role_id: "aparatchi".into(),
            assigned_apparatus: vec![CUT2.into()],
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
    (state, store, batch)
}

#[tokio::test]
async fn old_qr_lookup_returns_backend_effective_route_without_rewriting_the_batch() {
    let (state, store, original) = fixture("valid").await;
    let worker = session_for(&state, PrincipalRole::Aparatchi, "route-cut2-worker").await;
    let router = build_router(state);
    for scoped in [false, true, false, true] {
        let mut request = json!({"qr_payload":QR});
        if scoped {
            request["apparatus"] = json!(CUT2);
            request["order_id"] = json!(ORDER);
        }
        let response = router
            .clone()
            .oneshot(request_with_body(
                "POST",
                LOOKUP,
                &worker,
                &request.to_string(),
            ))
            .await
            .unwrap();
        let status = response.status();
        let body = json_body(response).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["batch"]["batch_id"], original.batch_id);
        assert_eq!(body["batch"]["qr_payload"], QR);
        assert_eq!(body["batch"]["produced_qty"], 6170.0);
        assert_eq!(body["batch"]["payload_json"]["stage_node_id"], "lam1");
        assert_eq!(
            body["batch"]["payload_json"]["next_stage_node_id"],
            "rezka_5"
        );
        assert!(body["batch"]["payload_json"]["wip_route_binding"].is_null());
        assert_eq!(body["input_route"]["source_stage_node_id"], "lam1");
        assert_eq!(body["input_route"]["stage_node_id"], "apparatus_6");
        assert_eq!(
            body["input_route"]["consumer_apparatus_ids"],
            json!([CUT, CUT2])
        );
        assert_eq!(body["input_route"]["remapped"], true);
        assert_eq!(
            body["input_route"]["map_fingerprint"]
                .as_str()
                .unwrap()
                .len(),
            64
        );
        assert!(body["input_route_error"].is_null());
        assert_eq!(body["validated_apparatus"], if scoped { CUT2 } else { "" });
        assert_eq!(body["validated_order_id"], if scoped { ORDER } else { "" });
    }
    let denied = router
        .oneshot(request_with_body(
            "POST",
            LOOKUP,
            &worker,
            &json!({"qr_payload":QR,"apparatus":CUT,"order_id":ORDER}).to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(
        denied.status(),
        StatusCode::FORBIDDEN,
        "effective alternative membership never grants worker assignment"
    );
    assert_eq!(
        store
            .progress_batch(&original.batch_id)
            .await
            .unwrap()
            .unwrap(),
        original
    );
}

#[tokio::test]
async fn unscoped_old_qr_reports_precise_route_failure_and_scoped_lookup_fails_closed() {
    for (case, code) in [
        ("missing_source", "wip_route_source_unresolved"),
        ("ambiguous", "wip_route_ambiguous"),
    ] {
        let (state, store, original) = fixture(case).await;
        let worker = session_for(&state, PrincipalRole::Aparatchi, "route-cut2-worker").await;
        let router = build_router(state);
        let response = router
            .clone()
            .oneshot(request_with_body(
                "POST",
                LOOKUP,
                &worker,
                &json!({"qr_payload":QR}).to_string(),
            ))
            .await
            .unwrap();
        let status = response.status();
        let body = json_body(response).await;
        assert_eq!(status, StatusCode::OK, "{case}: {body}");
        assert_eq!(body["batch"]["qr_payload"], QR);
        assert!(body["input_route"].is_null());
        assert_eq!(body["input_route_error"], code);
        let scoped = router
            .oneshot(request_with_body(
                "POST",
                LOOKUP,
                &worker,
                &json!({"qr_payload":QR,"apparatus":CUT2,"order_id":ORDER}).to_string(),
            ))
            .await
            .unwrap();
        let scoped_status = scoped.status();
        let scoped_body = json_body(scoped).await;
        assert_eq!(
            scoped_status,
            StatusCode::BAD_REQUEST,
            "{case}: {scoped_body}"
        );
        assert_eq!(scoped_body["error"], code);
        assert_eq!(
            store
                .progress_batch(&original.batch_id)
                .await
                .unwrap()
                .unwrap(),
            original
        );
    }
}
