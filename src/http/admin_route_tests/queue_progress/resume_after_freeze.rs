use super::*;
use crate::core::apparatus_standard::{
    ApparatusOperationalPolicies, CanonicalApparatusPatch, CanonicalCommandMetadata,
    QueueDiscipline,
};

#[tokio::test]
async fn unfrozen_order_resumes_saved_resources_without_scans_in_both_queue_modes() {
    const STATION: &str = "apparatus:default:bosma_7";
    const ORDER: &str = "zakaz-resume-no-scans";
    const OTHER: &str = "zakaz-resume-peer";
    for discipline in [QueueDiscipline::StrictSequence, QueueDiscipline::FreePick] {
        for other_active in [false, true] {
            for physical_molds in [true, false] {
                let state = test_state();
                let id = ApparatusId::new(STATION).unwrap();
                let current = state
                    .apparatus
                    .current_configuration(&id)
                    .await
                    .unwrap()
                    .unwrap();
                state
                    .apparatus
                    .patch(
                        id,
                        current.runtime.source_revision,
                        CanonicalApparatusPatch {
                            policies: Some(ApparatusOperationalPolicies {
                                queue: discipline,
                                material: current.material.policy.clone(),
                                tooling: current.material.tooling.clone(),
                            }),
                            ..Default::default()
                        },
                        CanonicalCommandMetadata::new("user:admin", "resume-no-scans-policy"),
                    )
                    .await
                    .unwrap();
                state
                    .admin
                    .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
                        principal_role: PrincipalRole::Aparatchi,
                        principal_ref: "resume-worker".into(),
                        role_id: "aparatchi".into(),
                        assigned_apparatus: vec![STATION.into()],
                        assigned_item_groups: Vec::new(),
                    })
                    .await
                    .unwrap();
                let admin = session(&state, PrincipalRole::Admin).await;
                let worker = session_for(&state, PrincipalRole::Aparatchi, "resume-worker").await;
                let router = build_router(state.clone());
                for order in [ORDER, OTHER] {
                    let response = router
                        .clone()
                        .oneshot(request_with_body(
                            "PUT",
                            "/v1/mobile/admin/production-maps",
                            &admin,
                            &pechat_order_map_json(order, order, order, STATION),
                        ))
                        .await
                        .unwrap();
                    assert_eq!(response.status(), StatusCode::OK);
                    if physical_molds {
                        provision_test_qolip(&router, &admin, order).await;
                    } else {
                        let map = state.production_maps.raw_map(order).await.unwrap().unwrap();
                        let response = router.clone().oneshot(request_with_body(
                        "POST", "/v1/mobile/qolip/product-specs", &admin,
                        &serde_json::json!({
                            "item_code": map.product_code, "item_name": map.title,
                                "item_group": "Tayyor mahsulot Test",
                            "warehouse": "Qolip ombor", "qolip_code": test_qolip_code(order),
                            "size": 42,
                        }).to_string(),
                    )).await.unwrap();
                        let status = response.status();
                        let body = json_body(response).await;
                        assert_eq!(status, StatusCode::OK, "{body}");
                    }
                }
                state
                    .production_maps
                    .set_apparatus_sequence(STATION, vec![ORDER.into(), OTHER.into()])
                    .await
                    .unwrap();

                let start_body = with_test_qolip(
                    &serde_json::json!({
                        "apparatus": STATION, "order_id": ORDER, "action": "start",
                    })
                    .to_string(),
                    ORDER,
                );
                let response = router
                    .clone()
                    .oneshot(request_with_body(
                        "POST",
                        "/v1/mobile/admin/production-maps/queue-action",
                        &worker,
                        &start_body,
                    ))
                    .await
                    .unwrap();
                let status = response.status();
                let started = json_body(response).await;
                assert_eq!(status, StatusCode::OK, "{started}");
                let session_id = started["session"]["session_id"].clone();
                let qolip_codes = started["session"]["payload_json"]["qolip_codes"].clone();
                assert_eq!(qolip_codes, serde_json::json!([test_qolip_code(ORDER)]));

                let freeze = serde_json::json!({
                    "apparatus": STATION, "order_id": ORDER, "action": "freeze",
                    "freeze_with_issue": true, "issue_note": "Temporary stop immediately after Start",
                });
                let response = router
                    .clone()
                    .oneshot(request_with_body(
                        "POST",
                        "/v1/mobile/admin/production-maps/queue-action",
                        &worker,
                        &freeze.to_string(),
                    ))
                    .await
                    .unwrap();
                let status = response.status();
                let frozen = json_body(response).await;
                assert_eq!(status, StatusCode::OK, "{frozen}");
                assert_eq!(frozen["session"]["session_id"], session_id);
                assert_eq!(frozen["session"]["status"], "frozen");

                let response = router
                    .clone()
                    .oneshot(request_with_body(
                        "POST",
                        "/v1/mobile/admin/production-maps/order-control",
                        &admin,
                        &serde_json::json!({"order_id": ORDER, "action": "unfreeze"}).to_string(),
                    ))
                    .await
                    .unwrap();
                let status = response.status();
                let unfrozen = json_body(response).await;
                assert_eq!(status, StatusCode::OK, "{unfrozen}");

                if other_active {
                    let response = router
                        .clone()
                        .oneshot(request_with_body(
                            "POST",
                            "/v1/mobile/admin/production-maps/queue-action",
                            &worker,
                            &with_test_qolip(
                                &serde_json::json!({
                                    "apparatus": STATION, "order_id": OTHER, "action": "start",
                                })
                                .to_string(),
                                OTHER,
                            ),
                        ))
                        .await
                        .unwrap();
                    let status = response.status();
                    let body = json_body(response).await;
                    assert_eq!(status, StatusCode::OK, "{body}");
                }

                let snapshot = state.production_maps.live_snapshot().await.unwrap();
                let control = &snapshot.queue_action_controls[STATION][ORDER];
                assert!(!control.interaction.material_scan_required);
                assert_eq!(
                    control.interaction.start_materials_mode,
                    crate::core::production_map::ApparatusQueueStartMaterialsMode::Hidden
                );
                assert_eq!(
                    control.interaction.qolip_mode,
                    crate::core::production_map::ApparatusQueueQolipMode::NotRequired
                );
                let ready = discipline == QueueDiscipline::FreePick && !other_active;
                assert_eq!(
                    control.allowed_actions.contains(
                        &crate::core::production_map::queue_state::ApparatusQueueAction::Resume,
                    ),
                    ready
                );
                // No material, mold or WIP scans are supplied on Resume.
                let resume = serde_json::json!({
                    "apparatus": STATION, "order_id": ORDER, "action": "resume",
                })
                .to_string();
                if !ready {
                    let rejected = router
                        .clone()
                        .oneshot(request_with_body(
                            "POST",
                            "/v1/mobile/admin/production-maps/queue-action",
                            &worker,
                            &resume,
                        ))
                        .await
                        .unwrap();
                    let status = rejected.status();
                    let body = json_body(rejected).await;
                    if discipline == QueueDiscipline::FreePick && other_active {
                        assert_eq!(status, StatusCode::CONFLICT, "{body}");
                        assert_eq!(body["error"], "capacity_conflict");
                    } else {
                        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
                        assert_eq!(body["error"], "queue_action_not_allowed");
                    }
                    let snapshot = state.production_maps.live_snapshot().await.unwrap();
                    assert_eq!(snapshot.queue_states[STATION][ORDER], "pending");
                    if other_active {
                        assert_eq!(snapshot.queue_states[STATION][OTHER], "in_progress");
                        let response = router
                            .clone()
                            .oneshot(request_with_body(
                                "POST",
                                "/v1/mobile/admin/production-maps/queue-action",
                                &worker,
                                &serde_json::json!({
                                    "apparatus": STATION, "order_id": OTHER, "action": "freeze",
                                    "freeze_with_issue": true, "issue_note": "Peer stop",
                                })
                                .to_string(),
                            ))
                            .await
                            .unwrap();
                        let status = response.status();
                        let body = json_body(response).await;
                        assert_eq!(status, StatusCode::OK, "{body}");
                    } else {
                        state
                            .production_maps
                            .set_apparatus_sequence(STATION, vec![ORDER.into(), OTHER.into()])
                            .await
                            .unwrap();
                    }
                }
                let response = router
                    .oneshot(request_with_body(
                        "POST",
                        "/v1/mobile/admin/production-maps/queue-action",
                        &worker,
                        &resume,
                    ))
                    .await
                    .unwrap();
                let status = response.status();
                let resumed = json_body(response).await;
                assert_eq!(
                    status,
                    StatusCode::OK,
                    "{discipline:?}, busy={other_active}: {resumed}"
                );
                assert_eq!(resumed["states"][ORDER], "in_progress");
                assert_eq!(resumed["session"]["session_id"], session_id);
                assert_eq!(resumed["session"]["status"], "active");
                assert_eq!(
                    resumed["session"]["payload_json"]["qolip_codes"],
                    qolip_codes
                );
                assert_eq!(resumed["session"]["payload_json"]["qolip_lock_owner"], true);
                assert_eq!(
                    resumed["session"]["payload_json"]["qolip_released_on_freeze"],
                    false
                );
            }
        }
    }
}
