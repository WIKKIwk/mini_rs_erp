use super::*;
use crate::core::apparatus_standard::{
    ApparatusOperationalPolicies, CanonicalApparatusPatch, CanonicalCommandMetadata,
    QueueDiscipline,
};
use crate::core::qolip::QolipService;
use serde_json::{Value, json};
const STATION: &str = "apparatus:default:bosma_8";
async fn call(
    router: &axum::Router,
    token: &str,
    uri: &str,
    method: &str,
    payload: Value,
) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(request_with_body(method, uri, token, &payload.to_string()))
        .await
        .unwrap();
    (response.status(), json_body(response).await)
}

#[tokio::test]
#[ignore = "requires MINI_ERP_TEST_ADMIN_DATABASE_URL on a disposable PostgreSQL server"]
async fn frozen_print_resume_rebinds_replacement_molds_and_preserves_output_history() {
    use crate::core::apparatus_standard::MaterialExecutionPolicy;
    use crate::db::{
        postgres, postgres_production_map::PostgresProductionMapStore,
        postgres_qolip::PostgresQolipStore,
    };
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .expect("explicit isolated PostgreSQL URL");
    let database = format!("qolip_resume_{:032x}", rand::random::<u128>());
    let admin_pool = sqlx::PgPool::connect(&admin_url).await.unwrap();
    sqlx::query(&format!("CREATE DATABASE {database}"))
        .execute(&admin_pool)
        .await
        .unwrap();
    let options = admin_url
        .parse::<sqlx::postgres::PgConnectOptions>()
        .unwrap()
        .database(&database);
    let owner = sqlx::PgPool::connect_with(options.clone()).await.unwrap();
    postgres::apply_postgres_migrations_through_version(&owner, "0121")
        .await
        .unwrap();
    let canonical = postgres::canonical_apparatus_service(owner.clone());
    canonical.bootstrap_factory_defaults().await.unwrap();
    postgres::apply_foundation_migration(&owner).await.unwrap();
    sqlx::raw_sql("INSERT INTO mini_item_groups(name,parent_item_group,is_group) VALUES('Tayyor mahsulot Test','All Item Groups',false) ON CONFLICT DO NOTHING;
        INSERT INTO mini_warehouses(id,name,parent_warehouse) VALUES('replacement-molds','Qolip ombor','') ON CONFLICT DO NOTHING;
        INSERT INTO mini_warehouse_assignments(warehouse,warehouse_name,assignment_kind,principal_role,principal_ref)
        VALUES('Qolip ombor','Qolip ombor','warehouse','qolipchi','replacement-clerk') ON CONFLICT DO NOTHING;").execute(&owner).await.unwrap();
    let id = ApparatusId::new(STATION).unwrap();
    let current = canonical.current_configuration(&id).await.unwrap().unwrap();
    canonical.patch(id,current.runtime.source_revision,CanonicalApparatusPatch {
        policies:Some(ApparatusOperationalPolicies {
            worker_material_assignment_allowed:false,queue:QueueDiscipline::FreePick,
            material:MaterialExecutionPolicy::NotRequired { item_group_ids:vec![] },
            tooling:serde_json::from_value(json!({"mode":"qolip_scan_required","tooling_class_id":"tooling-class:qolip"})).unwrap(),
        }),..Default::default()
    },CanonicalCommandMetadata::new("test","qolip-replacement-probe")).await.unwrap();
    let runtime = sqlx::postgres::PgPoolOptions::new()
        .max_connections(8)
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET ROLE mini_rs_erp")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect_with(options)
        .await
        .unwrap();
    let mut state = test_state();
    state.apparatus = postgres::canonical_apparatus_service(runtime.clone());
    state.production_maps = ProductionMapService::new(
        Arc::new(PostgresProductionMapStore::new(runtime.clone())),
        Arc::new(CanonicalServiceApparatusResolver::new(
            state.apparatus.clone(),
        )),
    );
    state.qolip = QolipService::new(Arc::new(PostgresQolipStore::new(runtime.clone())));
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "replacement-worker".into(),
            role_id: "aparatchi".into(),
            assigned_apparatus: vec![STATION.into()],
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
    let admin = session(&state, PrincipalRole::Admin).await;
    let worker = session_for(&state, PrincipalRole::Aparatchi, "replacement-worker").await;
    let clerk = session_for(&state, PrincipalRole::Qolipchi, "replacement-clerk").await;
    let router = build_router(state.clone());
    for (index, (method, frozen)) in [
        "product_molds",
        "set_edit",
        "rename_code",
        "automatic",
        "automatic_multi",
    ]
    .into_iter()
    .flat_map(|method| [false, true].map(|frozen| (method, frozen)))
    .enumerate()
    {
        if method.starts_with("automatic") && !frozen {
            continue;
        }
        let order = format!("zakaz-replace-{index}");
        let peer = format!("zakaz-replace-peer-{index}");
        let product = format!("REPLACE-{index}");
        let peer_product = format!("REPLACE-PEER-{index}");
        for (order, product) in [(&order, &product), (&peer, &peer_product)] {
            sqlx::query(
                "INSERT INTO mini_items(code,name,item_group) VALUES($1,$1,'Tayyor mahsulot Test')",
            )
            .bind(product)
            .execute(&owner)
            .await
            .unwrap();
            let mut map: Value = serde_json::from_str(&pechat_order_map_json(
                order,
                order,
                &format!("REPL-{index}-{product}"),
                STATION,
            ))
            .unwrap();
            map["product_code"] = json!(product);
            state
                .production_maps
                .upsert_map(serde_json::from_value(map).unwrap())
                .await
                .unwrap();
            new_mold(&router, &admin, product, &test_qolip_code(order)).await;
        }
        state
            .production_maps
            .set_apparatus_sequence(STATION, vec![order.clone(), peer.clone()])
            .await
            .unwrap();
        let (status, started) = action(&router, &worker, &order, "start").await;
        assert_eq!(status, StatusCode::OK, "start {method} {frozen}: {started}");
        let old = test_qolip_code(&order);
        let old_output = if frozen {
            let (status, output) = action(&router, &worker, &order, "pause").await;
            assert_eq!(status, StatusCode::OK, "{output}");
            let (status, resumed) = action(&router, &worker, &order, "resume").await;
            assert_eq!(status, StatusCode::OK, "{resumed}");
            Some(output["progress_batch"].clone())
        } else {
            None
        };
        let new = format!("REPLACEMENT-MOLD-{index}");
        let mut frozen_stage = Value::Null;
        if frozen {
            let (status,body)=call(&router,&worker,"/v1/mobile/admin/production-maps/queue-action","POST",json!({
                "apparatus":STATION,"order_id":order,"action":"freeze","freeze_with_issue":true,"issue_note":"Replacement probe",
            })).await;
            assert_eq!(status, StatusCode::OK, "freeze: {body}");
            frozen_stage = body["session"]["stage_node_id"].clone();
        } else {
            let (status, body) = action(&router, &worker, &order, "pause").await;
            assert_eq!(status, StatusCode::OK, "pause: {body}");
        }
        for act in ["start", "pause"] {
            let (status, body) = action(&router, &worker, &peer, act).await;
            assert_eq!(status, StatusCode::OK, "peer {act}: {body}");
        }
        let (uri, payload) = match method {
            "product_molds" | "automatic" | "automatic_multi" => (
                "/v1/mobile/qolip/product-molds",
                json!({"changes":[{
                    "item_code":product,"warehouse":"Qolip ombor","remove":[old],
                    "add":[{"qolip_code":new,"size":42,"color":"Black"}],
                }]}),
            ),
            "set_edit" => {
                let set_id = started["session"]["payload_json"]["qolip_set_id"]
                    .as_str()
                    .unwrap();
                let set = state
                    .qolip
                    .resolve_shared_set(set_id)
                    .await
                    .unwrap()
                    .unwrap();
                (
                    "/v1/mobile/qolip/sets",
                    json!({"set_id":set_id,"fingerprint":set.fingerprint,
                    "remove":[old],"add":[{"qolip_code":new,"size":42,"color":"Black"}]}),
                )
            }
            _ => (
                "/v1/mobile/qolip/product-specs",
                json!({
                    "item_code":product,"item_name":product,"item_group":"Tayyor mahsulot Test",
                    "warehouse":"Qolip ombor","previous_qolip_code":old,"qolip_code":new,"size":42,"color":"Black",
                }),
            ),
        };
        let (changed, change_body) = call(&router, &clerk, uri, "POST", payload).await;
        if frozen {
            assert_eq!(
                changed,
                StatusCode::OK,
                "replacement {method}: {change_body}"
            );
            let map = state
                .production_maps
                .raw_map(&order)
                .await
                .unwrap()
                .unwrap();
            let effective = state
                .qolip
                .required_qolips_for_production_map(&map)
                .await
                .unwrap();
            assert_eq!(
                effective
                    .iter()
                    .map(|m| m.qolip_code.as_str())
                    .collect::<Vec<_>>(),
                vec![new.as_str()]
            );
            println!(
                "WHILE FROZEN: replacement succeeded; current valid molds are {:?}",
                effective
                    .iter()
                    .map(|m| m.qolip_code.as_str())
                    .collect::<Vec<_>>()
            );
            let (status, body) = call(
                &router,
                &admin,
                "/v1/mobile/admin/production-maps/order-control",
                "POST",
                json!({
                    "apparatus":STATION,"order_id":order,"action":"unfreeze","scope":"stage",
                    "stage_node_id":frozen_stage,
                }),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "unfreeze after replacement: {body}");
            let fresh = format!("zakaz-fresh-replacement-{index}");
            let mut fresh_map: Value = serde_json::from_str(&pechat_order_map_json(
                &fresh,
                &fresh,
                &format!("FRESH-{index}"),
                STATION,
            ))
            .unwrap();
            fresh_map["product_code"] = json!(product);
            state
                .production_maps
                .upsert_map(serde_json::from_value(fresh_map).unwrap())
                .await
                .unwrap();
            let (status, body) = call(
                &router,
                &worker,
                "/v1/mobile/admin/production-maps/queue-action",
                "POST",
                json!({
                    "apparatus":STATION,"order_id":fresh,"action":"start","qolip_codes":[new],
                }),
            )
            .await;
            assert_eq!(
                status,
                StatusCode::OK,
                "new mold must actually start work for this product: {body}"
            );
            assert_eq!(body["session"]["payload_json"]["qolip_codes"], json!([new]));
            println!("NEW MOLD VALID: fresh order with same product and new mold -> {status}");
            let (status,body)=call(&router,&worker,"/v1/mobile/admin/production-maps/queue-action","POST",json!({
                "apparatus":STATION,"order_id":fresh,"action":"freeze","freeze_with_issue":true,"issue_note":"Release verified replacement mold",
            })).await;
            assert_eq!(
                status,
                StatusCode::OK,
                "release new mold after positive control: {body}"
            );
            assert_eq!(body["session"]["payload_json"]["qolip_lock_owner"], false);
            assert_eq!(
                state
                    .production_maps
                    .qolip_codes_for_resume(STATION, &order)
                    .await
                    .unwrap(),
                vec![old.clone()]
            );

            let multiple = method == "automatic_multi";
            if multiple {
                new_mold(&router, &admin, &product, &format!("A-ALT-{index}")).await;
                new_mold(&router, &admin, &product, &format!("B-ALT-{index}")).await;
            }
            let expected_codes = if multiple {
                json!([format!("B-ALT-{index}"), new])
            } else {
                json!([new])
            };
            let automatic = method.starts_with("automatic");
            if automatic {
                state
                    .production_maps
                    .update_qolip_start_settings(
                        true,
                        &crate::core::production_map::QueueActionActor {
                            role: "admin".into(),
                            ref_: "admin".into(),
                            display_name: "Admin".into(),
                        },
                    )
                    .await
                    .unwrap();
            }
            let (status,requirements)=call(&router,&worker,"/v1/mobile/admin/production-maps/qolip-validate","POST",json!({
                "apparatus":STATION,"order_id":order,"qolip_code":"","automatic_confirmation":automatic,
            })).await;
            assert_eq!(status, StatusCode::OK, "{requirements}");
            if multiple {
                assert_eq!(requirements["qolip"]["required_qolip_count"], 3);
                assert_eq!(requirements["qolip"]["sets"].as_array().unwrap().len(), 2);
            } else {
                assert_eq!(requirements["qolip"]["required_qolip_codes"], json!([new]));
            }
            let mut payload = json!({"apparatus":STATION,"order_id":order,"action":"resume"});
            if automatic {
                let ready = &requirements["qolip"]["sets"][if multiple { 1 } else { 0 }];
                assert_eq!(ready["ready"], true);
                payload["qolip_confirmation"] = json!({"method":"automatic","set_id":ready["set_id"],
                    "fingerprint":ready["fingerprint"],"policy_revision":requirements["qolip"]["policy_revision"]});
                let mut stale = payload.clone();
                stale["qolip_confirmation"]["policy_revision"] = json!(999999);
                let (status, body) = call(
                    &router,
                    &worker,
                    "/v1/mobile/admin/production-maps/queue-action",
                    "POST",
                    stale,
                )
                .await;
                assert_eq!(status, StatusCode::CONFLICT, "{body}");
                assert_eq!(body["error"], "qolip_policy_changed");
            } else {
                assert_eq!(requirements["qolip"]["reused_qolip_codes"], json!([]));
                let (status, body) = call(
                    &router,
                    &worker,
                    "/v1/mobile/admin/production-maps/queue-action",
                    "POST",
                    payload.clone(),
                )
                .await;
                assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
                assert_eq!(body["error"], "qolip_scan_required");
                let mut wrong = payload.clone();
                wrong["qolip_codes"] = json!(["NOT-ASSIGNED"]);
                let (status, body) = call(
                    &router,
                    &worker,
                    "/v1/mobile/admin/production-maps/queue-action",
                    "POST",
                    wrong,
                )
                .await;
                assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
                payload["qolip_codes"] = json!([new]);
            }
            let (status, resumed) = call(
                &router,
                &worker,
                "/v1/mobile/admin/production-maps/queue-action",
                "POST",
                payload,
            )
            .await;
            assert_eq!(
                status,
                StatusCode::OK,
                "replacement Resume {method}: {resumed}"
            );
            assert_eq!(
                resumed["session"]["session_id"],
                started["session"]["session_id"]
            );
            assert_eq!(
                resumed["session"]["payload_json"]["qolip_codes"],
                expected_codes
            );
            let audit:Value=sqlx::query_scalar("SELECT payload_json->'qolip_replaced_on_resume' FROM mini_queue_action_events WHERE order_id=$1 AND payload_json ? 'qolip_replaced_on_resume'")
                .bind(&order).fetch_one(&owner).await.unwrap();
            assert_eq!(audit["previous"]["qolip_codes"], json!([old]));
            assert_eq!(audit["current"]["qolip_codes"], expected_codes);
            let old_output = old_output.unwrap();
            let saved: Value = sqlx::query_scalar(
                "SELECT payload_json FROM mini_progress_batches WHERE batch_id=$1",
            )
            .bind(old_output["batch_id"].as_str().unwrap())
            .fetch_one(&owner)
            .await
            .unwrap();
            assert_eq!(
                saved, old_output["payload_json"],
                "old WIP history must remain unchanged"
            );
            let (status, output) = action(&router, &worker, &order, "pause").await;
            assert_eq!(status, StatusCode::OK, "new output: {output}");
            assert_eq!(
                output["progress_batch"]["payload_json"]["qolip_codes"],
                expected_codes
            );
        } else {
            assert!(
                changed.is_client_error(),
                "normal pause must block replacement {method}: {changed} {change_body}"
            );
            assert!(
                change_body.to_string().contains("qolip_in_use"),
                "{change_body}"
            );
            let (status, body) = call(
                &router,
                &worker,
                "/v1/mobile/admin/production-maps/queue-action",
                "POST",
                json!({
                    "apparatus":STATION,"order_id":order,"action":"resume",
                }),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "resume {method}: {body}");
            assert_eq!(body["session"]["payload_json"]["qolip_codes"], json!([old]));
            println!("PAUSED {method}: replacement={changed} {change_body}; Resume={status}");
            let (status, body) = action(&router, &worker, &order, "pause").await;
            assert_eq!(status, StatusCode::OK, "release apparatus: {body}");
        }
    }
    runtime.close().await;
    owner.close().await;
    sqlx::query(&format!("DROP DATABASE {database} WITH (FORCE)"))
        .execute(&admin_pool)
        .await
        .unwrap();
    admin_pool.close().await;
}

async fn action(
    router: &axum::Router,
    token: &str,
    order: &str,
    action: &str,
) -> (StatusCode, Value) {
    let mut payload = json!({"apparatus":STATION,"order_id":order,"action":action});
    if action == "start" {
        payload["qolip_code"] = json!(test_qolip_code(order));
    }
    if action == "pause" {
        payload["finished_goods_kg"] = json!(12);
        payload["finished_goods_meter"] = json!(80);
        payload["printer"] = json!("zebra");
        payload["print_mode"] = json!("rfid");
        payload["print_transport"] = json!("offline");
    }
    call(
        router,
        token,
        "/v1/mobile/admin/production-maps/queue-action",
        "POST",
        payload,
    )
    .await
}

async fn new_mold(router: &axum::Router, token: &str, product: &str, code: &str) {
    let (status, body) = call(
        router,
        token,
        "/v1/mobile/qolip/product-specs",
        "POST",
        json!({
            "item_code":product,"item_name":product,"item_group":"Tayyor mahsulot Test",
            "warehouse":"Qolip ombor","qolip_code":code,"size":42,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}
