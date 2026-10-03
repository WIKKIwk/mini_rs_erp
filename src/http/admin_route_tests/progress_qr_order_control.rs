use super::*;
use crate::core::production_map::{
    MaterialScanProgressAction, OrderControlState, ProductionMapDefinition, QueueActionActor,
    QueueProgressInput, TrustedQolipStartValidation, queue_state::ApparatusQueueAction,
};
use crate::core::qolip::{QolipOrderStartPreparation, QolipProductSpec};

const PRINT: &str = "apparatus:default:bosma_9";
const LAMINATION: &str = "apparatus:default:asset-007";
const ORDER: &str = "zakaz-frozen-wip-lookup";

#[tokio::test]
async fn worker_progress_qr_checks_global_freeze_before_downstream_route() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let map: ProductionMapDefinition = serde_json::from_value(serde_json::json!({
        "id": ORDER, "product_code": "FROZEN-WIP", "title": "Frozen waiting roll",
        "nodes": [
            {"id":"start", "kind":"start", "title":"Start"},
            {"id":"bosma", "kind":"apparatus", "title":"Bosma 9", "apparatus_id": PRINT},
            {"id":"lamination", "kind":"apparatus", "title":"Laminatsiya 1", "apparatus_id": LAMINATION},
            {"id":"rezka", "kind":"apparatus", "title":"Rezka", "apparatus_id":"apparatus:default:asset-010",
                "rezka_kadr_count":1, "rezka_label_length":100},
            {"id":"end", "kind":"end", "title":"End"}
        ],
        "edges": [
            {"from":"start", "to":"bosma"},
            {"from":"bosma", "to":"lamination"},
            {"from":"lamination", "to":"rezka"},
            {"from":"rezka", "to":"end"}
        ]
    })).unwrap();
    state.production_maps.upsert_map(map).await.unwrap();
    let actor = QueueActionActor {
        role: "worker".into(),
        ref_: "worker-qr".into(),
        display_name: "Worker".into(),
    };
    let assigned = [PRINT.to_string()];
    let qolip_validation = TrustedQolipStartValidation::from_preparations(
        &ApparatusId::new(PRINT).unwrap(),
        ORDER,
        &[QolipOrderStartPreparation {
            spec: QolipProductSpec {
                qolip_code: "QOLIP-QR-FREEZE".into(),
                ..Default::default()
            },
            checkout: None,
        }],
    );
    state
        .production_maps
        .apply_apparatus_queue_action_with_material_scan_and_progress(MaterialScanProgressAction {
            apparatus: PRINT,
            order_id: ORDER,
            action: ApparatusQueueAction::Start,
            assigned_apparatus: &assigned,
            actor: actor.clone(),
            material_barcodes: &[],
            state_material_barcodes: &[],
            progress: QueueProgressInput::default(),
            qolip_validation,
        })
        .await
        .unwrap();
    let output = state
        .production_maps
        .apply_apparatus_queue_action_with_progress(
            PRINT,
            ORDER,
            ApparatusQueueAction::DetachRoll,
            &assigned,
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(100.0),
                finished_goods_meter: Some(100.0),
                gross_qty: Some(11.0),
                finished_goods_kg: Some(10.0),
                bobina_kg: Some(1.0),
                uom: "m".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .progress_batch
        .unwrap();
    assert_eq!(output.next_apparatus, LAMINATION);
    let request = state
        .production_maps
        .request_order_freeze(
            ORDER,
            QueueActionActor {
                role: "admin".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(request.state, OrderControlState::Frozen);

    let router = build_router(state.clone());
    // Both FAB identification and scoped validation must report the global
    // control before routing. No downstream freeze fixture is provided.
    for scoped in [false, true] {
        let mut body =
            serde_json::json!({"qr_payload": output.qr_payload, "require_active_order":true});
        if scoped {
            body["apparatus"] = LAMINATION.into();
            body["order_id"] = ORDER.into();
        }
        let response = router
            .clone()
            .oneshot(request_with_body(
                "POST",
                "/v1/mobile/admin/production-maps/progress-qr/lookup",
                &token,
                &body.to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(json_body(response).await["error"], "order_frozen");
    }
    assert_eq!(
        state
            .production_maps
            .order_control_state(ORDER)
            .await
            .unwrap()
            .state,
        OrderControlState::Frozen
    );
    let snapshot = state.production_maps.live_snapshot().await.unwrap();
    assert!(
        snapshot
            .queue_action_controls
            .get(LAMINATION)
            .is_none_or(|orders| !orders.contains_key(ORDER))
    );
    // Read-only history lookup still works while the order is frozen.
    let response = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/progress-qr/lookup",
            &token,
            &serde_json::json!({"qr_payload":output.qr_payload}).to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["batch"]["batch_id"],
        output.batch_id
    );
    state
        .production_maps
        .unfreeze_order(
            ORDER,
            QueueActionActor {
                role: "admin".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let response = router
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/progress-qr/lookup",
            &token,
            &serde_json::json!({"qr_payload": output.qr_payload,
            "apparatus": LAMINATION, "order_id": ORDER, "require_active_order":true})
            .to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["active_order_validated"], true);
    assert_eq!(value["validated_apparatus"], LAMINATION);
    assert_eq!(value["batch"]["wip_status"], "waiting");
}
