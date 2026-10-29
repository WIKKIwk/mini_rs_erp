use super::*;
use crate::core::qolip::{
    MemoryQolipStore, QolipBlock, QolipLocation, QolipProduct, QolipProductSpec, QolipService,
    QolipStorePort,
};
use serde_json::{Value, json};

const PRINT: &str = "apparatus:default:bosma_7";
const OTHER_PRINT: &str = "apparatus:default:bosma_8";
const ORDER: &str = "zakaz-automatic-qolip";
const OTHER_ORDER: &str = "zakaz-automatic-qolip-other";
const ITEM: &str = "AUTOMATIC-QOLIP-ITEM";
const WORKER: &str = "automatic-qolip-worker";
const SETTINGS: &str = "/v1/mobile/admin/production-maps/qolip-start-settings";
const VALIDATE: &str = "/v1/mobile/admin/production-maps/qolip-validate";
const ACTION: &str = "/v1/mobile/admin/production-maps/queue-action";
const SET_A: &str = "950100000001";
const SET_B: &str = "950100000002";

struct Fixture {
    state: AppState,
    store: Arc<MemoryQolipStore>,
    admin: String,
    worker: String,
}

impl Fixture {
    async fn new(members: &[(&str, &str)]) -> Self {
        let store = Arc::new(MemoryQolipStore::new());
        store
            .seed_blocks(vec![QolipBlock {
                name: "Automatic block".into(),
                warehouse: "Automatic warehouse".into(),
            }])
            .await;
        store
            .seed_products(vec![QolipProduct {
                code: ITEM.into(),
                name: "Automatic label".into(),
                item_group: "Labels".into(),
                ..Default::default()
            }])
            .await;
        for (code, set) in members {
            store
                .put_product_spec(QolipProductSpec {
                    qolip_set_id: (*set).into(),
                    warehouse: "Automatic warehouse".into(),
                    item_code: ITEM.into(),
                    item_name: "Automatic label".into(),
                    item_group: "Labels".into(),
                    qolip_code: (*code).into(),
                    size: 420,
                    color: "BLACK".into(),
                    ..Default::default()
                })
                .await
                .unwrap();
        }
        let mut state = test_state();
        state.qolip = QolipService::new(store.clone());
        state
            .admin
            .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
                principal_role: PrincipalRole::Aparatchi,
                principal_ref: WORKER.into(),
                role_id: "aparatchi".into(),
                assigned_apparatus: vec![PRINT.into(), OTHER_PRINT.into()],
                assigned_item_groups: vec![],
            })
            .await
            .unwrap();
        for (order, apparatus) in [(ORDER, PRINT), (OTHER_ORDER, OTHER_PRINT)] {
            state
                .production_maps
                .upsert_map(
                    serde_json::from_str(&production_order_map_json_with_product(
                        order,
                        "Automatic label",
                        ITEM,
                        order,
                        apparatus,
                        1,
                        420.0,
                    ))
                    .unwrap(),
                )
                .await
                .unwrap();
        }
        let admin = session(&state, PrincipalRole::Admin).await;
        let worker = session_for(&state, PrincipalRole::Aparatchi, WORKER).await;
        Self {
            state,
            store,
            admin,
            worker,
        }
    }

    async fn call(
        &self,
        method: &str,
        path: &str,
        token: &str,
        body: Value,
    ) -> (StatusCode, Value) {
        let response = build_router(self.state.clone())
            .oneshot(request_with_body(method, path, token, &body.to_string()))
            .await
            .unwrap();
        (response.status(), json_body(response).await)
    }

    async fn enabled(&self, enabled: bool) -> Value {
        let (status, body) = self
            .call(
                "PUT",
                SETTINGS,
                &self.admin,
                json!({"automatic_confirmation_enabled": enabled}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["settings"].clone()
    }

    async fn requirements(&self, order: &str, apparatus: &str) -> Value {
        let (status, body) = self
            .call(
                "POST",
                VALIDATE,
                &self.worker,
                json!({
                    "apparatus": apparatus, "order_id": order, "qolip_code": "",
                    "automatic_confirmation": true,
                }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["qolip"].clone()
    }

    async fn start(
        &self,
        order: &str,
        apparatus: &str,
        confirmation: Value,
    ) -> (StatusCode, Value) {
        self.call(
            "POST",
            ACTION,
            &self.worker,
            json!({
                "apparatus": apparatus, "order_id": order, "action": "start",
                "qolip_confirmation": confirmation,
            }),
        )
        .await
    }

    async fn assert_unstarted(&self, order: &str) {
        assert!(
            self.state
                .production_maps
                .order_run_sessions_for_order(order)
                .await
                .unwrap()
                .is_empty()
        );
    }
}

fn confirmation(requirements: &Value, set_id: &str) -> Value {
    let selected = requirements["sets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|set| set["set_id"] == set_id)
        .expect("requested alternative");
    let fingerprint = selected["fingerprint"].as_str().unwrap();
    assert_eq!(fingerprint.len(), 64);
    json!({
        "method": "automatic", "set_id": set_id, "fingerprint": fingerprint,
        "policy_revision": requirements["policy_revision"],
    })
}

fn member<'a>(requirements: &'a Value, code: &str) -> &'a Value {
    requirements["required_qolips"]
        .as_array()
        .unwrap()
        .iter()
        .find(|mold| mold["qolip_code"] == code)
        .expect("required physical mold")
}

fn codes(payload: &Value) -> std::collections::BTreeSet<String> {
    payload["qolip_codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|code| code.as_str().unwrap().to_lowercase())
        .collect()
}

#[tokio::test]
async fn qolip_automatic_start_settings_default_roundtrip_and_strict_admin_access() {
    let fixture = Fixture::new(&[("AUTO-A1", SET_A)]).await;
    let (status, read) = fixture
        .call("GET", SETTINGS, &fixture.admin, Value::Null)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        read["settings"],
        json!({"automatic_confirmation_enabled": false, "revision": 0})
    );
    for role in [
        PrincipalRole::Aparatchi,
        PrincipalRole::Qolipchi,
        PrincipalRole::Customer,
    ] {
        let token = session_for(&fixture.state, role, "not-an-admin").await;
        for method in ["GET", "PUT"] {
            let (status, _) = fixture
                .call(
                    method,
                    SETTINGS,
                    &token,
                    json!({"automatic_confirmation_enabled": true}),
                )
                .await;
            assert_eq!(status, StatusCode::FORBIDDEN);
        }
    }
    let (status, _) = fixture
        .call(
            "PUT",
            SETTINGS,
            "",
            json!({"automatic_confirmation_enabled": true}),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    for invalid in [
        json!({}),
        json!({"automatic_confirmation_enabled": "true"}),
        json!({"automatic_confirmation_enabled": true, "actor_ref": "forged"}),
    ] {
        let (status, _) = fixture.call("PUT", SETTINGS, &fixture.admin, invalid).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    let revision_before = fixture.state.production_maps.snapshot_revision();
    let enabled = fixture.enabled(true).await;
    assert_eq!(enabled["automatic_confirmation_enabled"], true);
    assert!(enabled["revision"].as_u64().unwrap() > 0);
    assert!(
        fixture.state.production_maps.snapshot_revision() > revision_before,
        "Policy changes must invalidate cached worker controls"
    );
    let (_, read) = fixture
        .call("GET", SETTINGS, &fixture.admin, Value::Null)
        .await;
    assert_eq!(read["settings"], enabled);
    let disabled = fixture.enabled(false).await;
    assert_eq!(disabled["automatic_confirmation_enabled"], false);
    assert!(disabled["revision"].as_u64().unwrap() > enabled["revision"].as_u64().unwrap());
}

#[tokio::test]
async fn qolip_automatic_start_ready_read_is_non_mutating_and_start_records_physical_set() {
    let fixture = Fixture::new(&[("AUTO-A1", SET_A), ("AUTO-A2", SET_A)]).await;
    fixture.enabled(true).await;
    let requirements = fixture.requirements(ORDER, PRINT).await;
    assert_eq!(requirements["automatic_confirmation_enabled"], true);
    assert_eq!(requirements["blocking_reason_code"], "");
    assert_eq!(requirements["sets"].as_array().unwrap().len(), 1);
    assert_eq!(requirements["sets"][0]["ready"], true);
    for code in ["AUTO-A1", "AUTO-A2"] {
        assert_eq!(member(&requirements, code)["ready"], true);
    }
    fixture.assert_unstarted(ORDER).await;
    assert!(
        fixture
            .store
            .checkouts(None, None, "open", 100)
            .await
            .unwrap()
            .is_empty()
    );
    let automatic = confirmation(&requirements, SET_A);
    let (status, started) = fixture.start(ORDER, PRINT, automatic.clone()).await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let payload = &started["session"]["payload_json"];
    assert_eq!(payload["qolip_set_id"], SET_A);
    assert_eq!(payload["qolip_lock_owner"], true);
    assert_eq!(payload["qolip_confirmation"], automatic);
    assert_eq!(
        codes(payload),
        ["auto-a1".into(), "auto-a2".into()].into_iter().collect()
    );
    assert_eq!(
        payload["qolip_set_snapshot"]["members"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn qolip_automatic_start_bootstrap_negotiates_readiness_and_retains_legacy_scan_contract() {
    let fixture = Fixture::new(&[("AUTO-A1", SET_A)]).await;
    let settings = fixture.enabled(true).await;
    let path = format!(
        "/v1/mobile/admin/production-maps/order-scan-bootstrap?apparatus={}&order_id={ORDER}",
        urlencoding::encode(PRINT),
    );
    let (status, legacy) = fixture
        .call("GET", &path, &fixture.worker, Value::Null)
        .await;
    assert_eq!(status, StatusCode::OK, "{legacy}");
    let interaction = &legacy["control_state"]["control"]["interaction"];
    assert_eq!(interaction["qolip_mode"], "scan_required");
    assert_eq!(interaction["qolip_automatic_confirmation"], true);
    assert_eq!(interaction["qolip_policy_revision"], settings["revision"]);
    assert_ne!(
        legacy["sections"]["qolips"]["data"]["qolip"]["automatic_confirmation_enabled"],
        true
    );
    let automatic_path = format!("{path}&automatic_qolip_confirmation=true");
    let (status, automatic) = fixture
        .call("GET", &automatic_path, &fixture.worker, Value::Null)
        .await;
    assert_eq!(status, StatusCode::OK, "{automatic}");
    let section = &automatic["sections"]["qolips"];
    assert_eq!(section["status"], "ready");
    assert_eq!(
        section["data"]["qolip"]["automatic_confirmation_enabled"],
        true
    );
    assert_eq!(
        section["data"]["qolip"]["policy_revision"],
        settings["revision"]
    );
    let (status, sent) = fixture
        .call(
            "POST",
            "/v1/mobile/qolip/reworks/send",
            &fixture.admin,
            json!({"request_id": "bootstrap-away", "qolip_codes": ["AUTO-A1"]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    let (status, blocked) = fixture
        .call("GET", &automatic_path, &fixture.worker, Value::Null)
        .await;
    assert_eq!(status, StatusCode::OK, "{blocked}");
    assert_eq!(
        blocked["sections"]["qolips"]["status"], "ready",
        "A successful readiness read must preserve per-mold blockers"
    );
    assert_eq!(
        blocked["sections"]["qolips"]["data"]["qolip"]["blocking_reason_code"],
        "qolip_in_rework"
    );
    let (_, legacy_blocked) = fixture
        .call("GET", &path, &fixture.worker, Value::Null)
        .await;
    assert_eq!(legacy_blocked["sections"]["qolips"]["status"], "error");
    fixture.assert_unstarted(ORDER).await;
}

#[tokio::test]
async fn qolip_automatic_start_uses_one_selected_alternative_with_product_extensions() {
    let fixture = Fixture::new(&[("AUTO-A1", SET_A), ("AUTO-A2", SET_A), ("AUTO-B1", SET_B)]).await;
    let (status, added) = fixture
        .call(
            "POST",
            "/v1/mobile/qolip/product-molds",
            &fixture.admin,
            json!({"changes": [{"item_code": ITEM, "warehouse": "Automatic warehouse",
            "add": [{"qolip_code": "AUTO-EXT", "size": 420, "color": "PANTON 1"}]}]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{added}");
    fixture.enabled(true).await;
    let requirements = fixture.requirements(ORDER, PRINT).await;
    assert_eq!(requirements["sets"].as_array().unwrap().len(), 2);
    assert_eq!(member(&requirements, "AUTO-EXT")["is_extension"], true);
    let (status, started) = fixture
        .start(ORDER, PRINT, confirmation(&requirements, SET_B))
        .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let payload = &started["session"]["payload_json"];
    assert_eq!(payload["qolip_set_id"], SET_B);
    assert_eq!(
        codes(payload),
        ["auto-b1".into(), "auto-ext".into()].into_iter().collect()
    );
}

#[tokio::test]
async fn qolip_automatic_start_rework_blocks_every_alternative_until_all_molds_return() {
    let fixture = Fixture::new(&[("AUTO-A1", SET_A), ("AUTO-A2", SET_A), ("AUTO-B1", SET_B)]).await;
    fixture.enabled(true).await;
    let (status, sent) = fixture
        .call(
            "POST",
            "/v1/mobile/qolip/reworks/send",
            &fixture.admin,
            json!({"request_id": "automatic-away", "qolip_codes": ["AUTO-A1", "AUTO-A2"]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    let entries = sent["reworks"].as_array().unwrap();
    for returned in 0..=entries.len() {
        let requirements = fixture.requirements(ORDER, PRINT).await;
        assert_eq!(
            member(&requirements, "AUTO-B1")["ready"],
            true,
            "The intact alternative still renders its healthy physical mold"
        );
        let away = requirements["required_qolips"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|mold| mold["ready"] == false)
            .count();
        assert_eq!(away, entries.len() - returned);
        if returned < entries.len() {
            assert_eq!(requirements["blocking_reason_code"], "qolip_in_rework");
            assert!(
                requirements["sets"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|set| set["ready"] == false)
            );
            let (status, rejected) = fixture
                .start(ORDER, PRINT, confirmation(&requirements, SET_B))
                .await;
            assert!(!status.is_success());
            assert_eq!(rejected["error"], "qolip_in_rework");
            fixture.assert_unstarted(ORDER).await;
            let (status, response) = fixture
                .call(
                    "POST",
                    "/v1/mobile/qolip/reworks/return",
                    &fixture.admin,
                    json!({"request_id": format!("automatic-return-{returned}"),
                    "rework_ids": [entries[returned]["id"]]}),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "{response}");
        } else {
            assert_eq!(requirements["blocking_reason_code"], "");
            assert!(
                requirements["sets"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|set| set["ready"] == true)
            );
        }
    }
}

#[tokio::test]
async fn qolip_automatic_start_missing_molds_do_not_become_implicitly_ready() {
    let fixture = Fixture::new(&[]).await;
    fixture.enabled(true).await;
    let requirements = fixture.requirements(ORDER, PRINT).await;
    assert_eq!(requirements["sets"], json!([]));
    assert_eq!(requirements["required_qolips"], json!([]));
    assert_eq!(requirements["blocking_reason_code"], "qolip_scan_required");
    let (status, _) = fixture
        .start(
            ORDER,
            PRINT,
            json!({
                "method": "automatic", "set_id": SET_A, "fingerprint": "a".repeat(64),
                "policy_revision": requirements["policy_revision"],
            }),
        )
        .await;
    assert!(!status.is_success());
    fixture.assert_unstarted(ORDER).await;
}

#[tokio::test]
async fn qolip_automatic_start_rejects_disabled_stale_policy_changed_set_and_mixed_input() {
    let fixture = Fixture::new(&[("AUTO-A1", SET_A)]).await;
    fixture.enabled(true).await;
    let old = fixture.requirements(ORDER, PRINT).await;
    let old_confirmation = confirmation(&old, SET_A);
    fixture.enabled(false).await;
    let (status, rejected) = fixture.start(ORDER, PRINT, old_confirmation.clone()).await;
    assert!(!status.is_success());
    assert_eq!(rejected["error"], "qolip_automatic_confirmation_disabled");
    fixture.enabled(true).await;
    let (status, rejected) = fixture.start(ORDER, PRINT, old_confirmation).await;
    assert!(!status.is_success());
    assert_eq!(rejected["error"], "qolip_policy_changed");
    let current = fixture.requirements(ORDER, PRINT).await;
    let automatic = confirmation(&current, SET_A);
    let (status, rejected) = fixture
        .call(
            "POST",
            ACTION,
            &fixture.worker,
            json!({
                "apparatus": PRINT, "order_id": ORDER, "action": "start",
                "qolip_confirmation": automatic, "qolip_codes": ["AUTO-A1"],
            }),
        )
        .await;
    assert!(!status.is_success());
    assert_eq!(rejected["error"], "qolip_confirmation_invalid");
    let mut changed = fixture
        .store
        .product_spec_by_qolip_code("AUTO-A1")
        .await
        .unwrap()
        .unwrap();
    changed.size = 421;
    fixture
        .store
        .rename_product_spec("AUTO-A1", changed)
        .await
        .unwrap();
    let (status, rejected) = fixture
        .start(ORDER, PRINT, confirmation(&current, SET_A))
        .await;
    assert!(!status.is_success());
    assert_eq!(rejected["error"], "qolip_set_changed");
    fixture.assert_unstarted(ORDER).await;
}

#[tokio::test]
async fn qolip_automatic_start_cannot_claim_molds_from_another_active_order() {
    let fixture = Fixture::new(&[("AUTO-A1", SET_A)]).await;
    fixture.enabled(true).await;
    let requirements = fixture.requirements(ORDER, PRINT).await;
    let before_claim = fixture.requirements(OTHER_ORDER, OTHER_PRINT).await;
    let (status, started) = fixture
        .start(ORDER, PRINT, confirmation(&requirements, SET_A))
        .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let after_claim = fixture.requirements(OTHER_ORDER, OTHER_PRINT).await;
    assert_eq!(member(&after_claim, "AUTO-A1")["ready"], false);
    assert_eq!(after_claim["sets"][0]["ready"], false);
    let (status, rejected) = fixture
        .start(OTHER_ORDER, OTHER_PRINT, confirmation(&before_claim, SET_A))
        .await;
    assert!(!status.is_success());
    assert_eq!(rejected["error"], "qolip_already_in_use");
    fixture.assert_unstarted(OTHER_ORDER).await;
}

#[tokio::test]
async fn qolip_automatic_start_off_preserves_manual_scan_requirement_and_legacy_start() {
    let fixture = Fixture::new(&[("AUTO-A1", SET_A), ("AUTO-A2", SET_A)]).await;
    let requirements = fixture.requirements(ORDER, PRINT).await;
    assert_ne!(requirements["automatic_confirmation_enabled"], true);
    let (status, rejected) = fixture
        .call(
            "POST",
            ACTION,
            &fixture.worker,
            json!({"apparatus": PRINT, "order_id": ORDER, "action": "start"}),
        )
        .await;
    assert!(!status.is_success());
    assert_eq!(rejected["error"], "qolip_scan_required");
    let (status, started) = fixture
        .call(
            "POST",
            ACTION,
            &fixture.worker,
            json!({"apparatus": PRINT, "order_id": ORDER, "action": "start",
            "qolip_codes": ["AUTO-A1", "AUTO-A2"]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    assert_eq!(started["session"]["payload_json"]["qolip_set_id"], SET_A);
    assert_ne!(
        started["session"]["payload_json"]["qolip_confirmation"]["method"],
        "automatic"
    );
}

#[tokio::test]
async fn qolip_automatic_start_multi_product_requires_one_common_complete_set_and_extensions() {
    const SECOND_ITEM: &str = "AUTOMATIC-QOLIP-SECOND";
    const MULTI_ORDER: &str = "zakaz-automatic-qolip-multi";
    let fixture = Fixture::new(&[("AUTO-A1", SET_A), ("AUTO-A2", SET_A), ("AUTO-B1", SET_B)]).await;
    fixture
        .store
        .seed_products(vec![
            QolipProduct {
                code: ITEM.into(),
                name: "Automatic label".into(),
                item_group: "Labels".into(),
                ..Default::default()
            },
            QolipProduct {
                code: SECOND_ITEM.into(),
                name: "Second label".into(),
                item_group: "Labels".into(),
                ..Default::default()
            },
        ])
        .await;
    let shared = fixture
        .state
        .qolip
        .resolve_shared_set(SET_A)
        .await
        .unwrap()
        .unwrap();
    let (status, linked) = fixture
        .call(
            "POST",
            "/v1/mobile/qolip/shared-sets",
            &fixture.admin,
            json!({"set_id": SET_A, "item_code": SECOND_ITEM, "fingerprint": shared.fingerprint}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{linked}");
    let (status, added) = fixture
        .call(
            "POST",
            "/v1/mobile/qolip/product-molds",
            &fixture.admin,
            json!({"changes": [{"item_code": ITEM, "warehouse": "Automatic warehouse",
            "add": [{"qolip_code": "AUTO-COMMON-EXT", "size": 420, "color": "PANTON 1"}]}]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{added}");
    let (status, linked) = fixture
        .call(
            "POST",
            "/v1/mobile/qolip/product-molds",
            &fixture.admin,
            json!({"changes": [{"item_code": SECOND_ITEM,
            "add": [{"qolip_code": "AUTO-COMMON-EXT", "existing_only": true}]}]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{linked}");
    let mut map = fixture
        .state
        .production_maps
        .raw_map(ORDER)
        .await
        .unwrap()
        .unwrap();
    // Opened-order calculations are immutable. Build this multi-product
    // order as its own fresh definition rather than editing a live order.
    map.id = MULTI_ORDER.into();
    map.order_number = MULTI_ORDER.into();
    map.products = vec![
        crate::core::order_layout::OrderProduct {
            id: "first".into(),
            item_code: ITEM.into(),
            product: "Automatic label".into(),
            frame_product_size_mm: 210.0,
            frame_count: 1,
        },
        crate::core::order_layout::OrderProduct {
            id: "second".into(),
            item_code: SECOND_ITEM.into(),
            product: "Second label".into(),
            frame_product_size_mm: 210.0,
            frame_count: 1,
        },
    ];
    fixture.state.production_maps.upsert_map(map).await.unwrap();
    fixture.enabled(true).await;
    let requirements = fixture.requirements(MULTI_ORDER, PRINT).await;
    assert_eq!(requirements["blocking_reason_code"], "");
    assert_eq!(
        requirements["sets"].as_array().unwrap().len(),
        1,
        "A native alternative unavailable to the second product cannot be selected"
    );
    assert_eq!(requirements["sets"][0]["set_id"], SET_A);
    assert_eq!(requirements["required_qolips"].as_array().unwrap().len(), 3);
    assert_eq!(
        member(&requirements, "AUTO-COMMON-EXT")["is_extension"],
        true
    );
    let (status, started) = fixture
        .start(MULTI_ORDER, PRINT, confirmation(&requirements, SET_A))
        .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    assert_eq!(
        codes(&started["session"]["payload_json"]),
        ["auto-a1".into(), "auto-a2".into(), "auto-common-ext".into()]
            .into_iter()
            .collect()
    );
}

#[tokio::test]
async fn qolip_automatic_start_rechecks_physical_checkout_owned_by_another_worker() {
    let fixture = Fixture::new(&[("AUTO-A1", SET_A)]).await;
    fixture
        .store
        .put_location(QolipLocation {
            id: "physical-auto-location".into(),
            block: "Automatic block".into(),
            warehouse: "Automatic warehouse".into(),
            item_code: ITEM.into(),
            item_name: "Automatic label".into(),
            qolip_code: "AUTO-A1".into(),
            size: 420,
            quantity: 1,
            row_letter: "A".into(),
            column_number: Some(1),
            location_label: "A1".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    fixture.enabled(true).await;
    let ready_before_checkout = fixture.requirements(ORDER, PRINT).await;
    assert_eq!(member(&ready_before_checkout, "AUTO-A1")["ready"], true);
    assert!(
        fixture
            .store
            .checkouts(None, None, "open", 100)
            .await
            .unwrap()
            .is_empty(),
        "The readiness read must not take physical stock"
    );
    let principal = Principal {
        role: PrincipalRole::Admin,
        display_name: "Admin".into(),
        legal_name: "Admin".into(),
        ref_: "admin".into(),
        phone: String::new(),
        avatar_url: String::new(),
    };
    let checkout = fixture
        .state
        .qolip
        .checkout_qolip_code_for_order_start(
            "AUTO-A1",
            ITEM,
            "Automatic label",
            "another-worker",
            "Other worker",
            &principal,
        )
        .await
        .unwrap();
    let blocked = fixture.requirements(ORDER, PRINT).await;
    assert_eq!(member(&blocked, "AUTO-A1")["ready"], false);
    assert_eq!(
        member(&blocked, "AUTO-A1")["blocking_reason_code"],
        "qolip_checkout_assigned_to_another_worker"
    );
    let (status, rejected) = fixture
        .start(ORDER, PRINT, confirmation(&ready_before_checkout, SET_A))
        .await;
    assert!(!status.is_success());
    assert_eq!(
        rejected["error"],
        "qolip_checkout_assigned_to_another_worker"
    );
    fixture.assert_unstarted(ORDER).await;
    let open = fixture
        .store
        .checkouts(None, None, "open", 100)
        .await
        .unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].issued_to_ref, "another-worker");
    fixture
        .store
        .return_checkout(&checkout.id, "A", Some(1))
        .await
        .unwrap();
    let available_again = fixture.requirements(ORDER, PRINT).await;
    assert_eq!(member(&available_again, "AUTO-A1")["ready"], true);
}

#[tokio::test]
async fn qolip_automatic_start_confirmation_rejects_unsupported_actions_or_malformed_requests() {
    let fixture = Fixture::new(&[("AUTO-A1", SET_A)]).await;
    fixture.enabled(true).await;
    let requirements = fixture.requirements(ORDER, PRINT).await;
    let valid = confirmation(&requirements, SET_A);
    for action in ["resume", "pause", "complete", "freeze"] {
        let (status, rejected) = fixture
            .call(
                "POST",
                ACTION,
                &fixture.worker,
                json!({
                    "apparatus": PRINT, "order_id": ORDER, "action": action,
                    "qolip_confirmation": valid,
                }),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{rejected}");
        assert_eq!(rejected["error"], "qolip_confirmation_invalid");
    }
    for (field, value) in [
        ("method", json!("manual")),
        ("set_id", json!("")),
        ("policy_revision", json!(-1)),
        ("fingerprint", json!("not-a-fingerprint")),
    ] {
        let mut invalid = valid.clone();
        invalid[field] = value;
        let (status, rejected) = fixture.start(ORDER, PRINT, invalid).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{rejected}");
        assert_eq!(rejected["error"], "qolip_confirmation_invalid");
    }
    fixture.assert_unstarted(ORDER).await;
}

#[tokio::test]
async fn qolip_automatic_start_readiness_remains_independent_from_material_assignment_gate() {
    let fixture = Fixture::new(&[("AUTO-A1", SET_A)]).await;
    let policy = serde_json::from_str(&canonical_material_policy_body(
        PRINT,
        1,
        json!({"mode": "all_required", "item_group_ids": ["Kraska"]}),
        true,
    ))
    .unwrap();
    let (status, saved) = fixture
        .call(
            "PUT",
            "/v1/mobile/admin/raw-material-rules",
            &fixture.admin,
            policy,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    fixture.enabled(true).await;
    let controls = fixture
        .state
        .production_maps
        .queue_action_controls_for_apparatus(PRINT)
        .await
        .unwrap();
    let control = &controls[PRINT][ORDER];
    assert!(
        !control
            .allowed_actions
            .contains(&crate::core::production_map::queue_state::ApparatusQueueAction::Start)
    );
    assert_eq!(
        control.interaction.blocking_reason_code,
        "raw_material_assignment_required"
    );
    let requirements = fixture.requirements(ORDER, PRINT).await;
    assert_eq!(requirements["blocking_reason_code"], "");
    assert_eq!(
        member(&requirements, "AUTO-A1")["ready"],
        true,
        "A healthy mold remains individually ready while a separate material assignment is missing"
    );
    let (status, rejected) = fixture
        .start(ORDER, PRINT, confirmation(&requirements, SET_A))
        .await;
    assert!(!status.is_success());
    assert_eq!(rejected["error"], "queue_action_not_allowed");
    fixture.assert_unstarted(ORDER).await;
}

#[tokio::test]
async fn qolip_automatic_start_preserves_extension_only_catalog_policy() {
    let fixture = Fixture::new(&[]).await;
    let (status, added) = fixture
        .call(
            "POST",
            "/v1/mobile/qolip/product-molds",
            &fixture.admin,
            json!({"changes": [{"item_code": ITEM, "warehouse": "Automatic warehouse", "add": [
                {"qolip_code": "AUTO-ONLY-A", "size": 420, "color": "BLACK"},
                {"qolip_code": "AUTO-ONLY-B", "size": 420, "color": "PANTON 1"},
            ]}]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{added}");
    fixture.enabled(true).await;
    let requirements = fixture.requirements(ORDER, PRINT).await;
    assert_eq!(requirements["blocking_reason_code"], "");
    assert_eq!(requirements["sets"].as_array().unwrap().len(), 1);
    let molds = requirements["required_qolips"].as_array().unwrap();
    assert_eq!(molds.len(), 2);
    assert!(
        molds
            .iter()
            .all(|mold| mold["is_extension"] == true && mold["ready"] == true)
    );
    let selected_id = requirements["sets"][0]["set_id"].as_str().unwrap();
    assert_eq!(requirements["sets"][0]["ready"], true);
    let (status, started) = fixture
        .start(ORDER, PRINT, confirmation(&requirements, selected_id))
        .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let payload = &started["session"]["payload_json"];
    assert_eq!(payload["qolip_set_id"], selected_id);
    assert_eq!(
        codes(payload),
        ["auto-only-a".into(), "auto-only-b".into()]
            .into_iter()
            .collect()
    );
    assert!(
        payload["qolip_set_snapshot"]["members"]
            .as_array()
            .unwrap()
            .iter()
            .all(|mold| mold["is_extension"] == true)
    );
}
