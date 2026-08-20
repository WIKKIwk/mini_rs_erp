use super::*;

#[tokio::test]
async fn pechat_task_rezka_persists_qolip_lineage_into_downstream_start() {
    let mut state = test_state();
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-qolip-task-rezka".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec![
                "apparatus:default:bosma_7".to_string(),
                "apparatus:default:asset-010".to_string(),
            ],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token =
        session_for(&state, PrincipalRole::Aparatchi, "worker-qolip-task-rezka").await;
    let router = build_router(state);
    let order_id = "zakaz-qolip-task-rezka";

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_task_rezka_order_map_json(order_id, "Qolip chain order", "9601"),
        ))
        .await
        .expect("save map");
    assert_eq!(saved.status(), StatusCode::OK);
    provision_test_qolip(&router, &admin_token, order_id).await;

    let started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &with_test_qolip(
                &format!(
                    r#"{{
                        "apparatus":"apparatus:default:bosma_7",
                        "order_id":"{order_id}",
                        "action":"start"
                    }}"#
                ),
                order_id,
            ),
        ))
        .await
        .expect("start pechat");
    assert_eq!(started.status(), StatusCode::OK);

    let completed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:bosma_7",
                    "order_id":"{order_id}",
                    "action":"complete",
                    "returned_paint_items":[
                        {{"usage":"rasxot","category":"colors","name":"Oq","values":{{"Mix":9,"Oq":0,"Qora":0}}}},
                        {{"usage":"astatka","category":"colors","name":"Oq","values":{{"Mix":0.75,"Oq":0.25}}}},
                        {{"usage":"astatka","category":"solvents","name":"Spirtlar","values":{{"Etil":0.25}}}}
                    ],
                    "total_waste":2.5,
                    "finished_goods_kg":18.75,
                    "finished_goods_meter":125.5,
                    "printer":"zebra",
                    "print_mode":"rfid"
                }}"#
            ),
        ))
        .await
        .expect("complete pechat");
    let completed_status = completed.status();
    let completed_body = json_body(completed).await;
    assert_eq!(completed_status, StatusCode::OK, "{completed_body:?}");
    assert_eq!(
        completed_body["progress_batch"]["payload_json"]["qolip_code"],
        test_qolip_code(order_id)
    );
    assert_eq!(
        completed_body["progress_batch"]["next_apparatus"],
        "apparatus:default:asset-010"
    );
    let qr_payload = completed_body["progress_batch"]["qr_payload"]
        .as_str()
        .expect("pechat output qr")
        .to_string();

    let rezka_started = router
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:asset-010",
                    "order_id":"{order_id}",
                    "action":"start",
                    "qr_payload":"{qr_payload}"
                }}"#
            ),
        ))
        .await
        .expect("start rezka from pechat wip");
    let rezka_status = rezka_started.status();
    let rezka_body = json_body(rezka_started).await;
    assert_eq!(rezka_status, StatusCode::OK, "{rezka_body:?}");
    assert_eq!(
        rezka_body["session"]["payload_json"]["qolip_code"],
        test_qolip_code(order_id)
    );
    assert_eq!(
        rezka_body["session"]["payload_json"]["input_progress_apparatus"],
        "apparatus:default:bosma_7"
    );
}
