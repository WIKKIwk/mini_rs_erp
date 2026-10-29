use super::*;
use crate::core::apparatus_standard::service::CanonicalApparatusService;
use crate::core::production_map::{OrderProgressBatch, ProductionMapStorePort, QueueActionActor};
use crate::db::postgres::{
    apply_foundation_migration, apply_postgres_migrations_through_version,
    postgres_test_database_options,
};
use crate::db::postgres_canonical_apparatus::PostgresCanonicalApparatusRepository;
use crate::db::postgres_production_map::PostgresProductionMapStore;
use serde_json::{Value, json};
use sqlx::PgPool;

const BASE: &str = "/v1/mobile/admin/production-maps/paddons";
const CUT1: &str = "apparatus:test:http-rezka-1";
const CUT5: &str = "apparatus:test:http-rezka-5";
const CUT3: &str = "apparatus:test:http-rezka-3";
const WORKER: &str = "http-multi-cut-worker";

async fn post(router: &axum::Router, token: &str, path: &str, body: Value) -> Value {
    let response = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            &format!("{BASE}/{path}"),
            token,
            &body.to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    json_body(response).await
}

fn candidate(value: &Value, id: &str) -> bool {
    value["available_items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|b| b["batch_id"] == id)
}

async fn assign(state: &AppState, apparatuses: &[&str]) {
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: WORKER.into(),
            role_id: "aparatchi".into(),
            assigned_apparatus: apparatuses.iter().map(|id| (*id).into()).collect(),
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn paddon_cross_apparatus_http_uses_current_server_scope_and_refreshes_mutation_candidates() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
    let admin = PgPool::connect(&url).await.unwrap();
    let database = format!(
        "mini_rs_erp_test_http_paddon_cross_{:016x}",
        rand::random::<u64>()
    );
    sqlx::query(&format!("CREATE DATABASE {database}"))
        .execute(&admin)
        .await
        .unwrap();
    let pool = PgPool::connect_with(postgres_test_database_options(&url, &database))
        .await
        .unwrap();
    apply_postgres_migrations_through_version(&pool, "0121")
        .await
        .unwrap();
    let canonical = CanonicalApparatusService::new(Arc::new(
        PostgresCanonicalApparatusRepository::new(pool.clone()),
    ));
    for revision in crate::core::apparatus_standard::test_support::standard_revisions() {
        canonical
            .seed_for_test(revision.apparatus_id.clone(), revision.to_draft())
            .await
            .unwrap();
    }
    for (id, name) in [(CUT1, "Rezka 1"), (CUT5, "Rezka 5"), (CUT3, "Rezka 3")] {
        canonical
            .seed_for_test(
                ApparatusId::new(id).unwrap(),
                canonical_draft(&TestApparatusSpec::cut(id, name)),
            )
            .await
            .unwrap();
    }
    apply_foundation_migration(&pool).await.unwrap();
    let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
    let mut state = test_state();
    state.apparatus = canonical;
    state.production_maps = ProductionMapService::new(
        store.clone(),
        Arc::new(CanonicalServiceApparatusResolver::new(
            state.apparatus.clone(),
        )),
    );
    assign(&state, &[CUT1, CUT5]).await;
    let actor = QueueActionActor {
        role: "aparatchi".into(),
        ref_: WORKER.into(),
        display_name: "Worker".into(),
    };
    let source = state
        .production_maps
        .create_paddon_for_apparatus(Some(CUT1), "", "", &actor)
        .await
        .unwrap();
    let target = state
        .production_maps
        .create_paddon_for_apparatus(Some(CUT5), "", "", &actor)
        .await
        .unwrap();
    let mut originals = Vec::new();
    for (number, apparatus, ids) in [
        (9861, CUT1, vec!["http-output-a", "http-output-b"]),
        (9865, CUT5, vec!["http-output-other-machine"]),
        (9863, CUT3, vec!["http-output-unassigned"]),
    ] {
        let order = format!("http-paddon-cross-{number}");
        state.production_maps.upsert_map(serde_json::from_value(json!({
            "id":order,"product_code":order,"title":order,"order_number":number.to_string(),
            "nodes":[{"id":"start","kind":"start","title":"Start"},
                {"id":"cut","kind":"apparatus","title":"Cut","apparatus_id":apparatus,"rezka_kadr_count":1},
                {"id":"end","kind":"end","title":"End"}],
            "edges":[{"from":"start","to":"cut"},{"from":"cut","to":"end"}]
        })).unwrap()).await.unwrap();
        for id in ids {
            let batch: OrderProgressBatch = serde_json::from_value(json!({
                "batch_id":id,"session_id":format!("session-{id}"),"started_at_unix":0,"completed_at_unix":0,
                "apparatus":apparatus,"current_apparatus":apparatus,"order_id":order,
                "action":"roll_complete","status":"completed","produced_qty":125.0,"uom":"m",
                "qr_payload":format!("PROGRESS:{id}"),"label_item_code":"PRODUCT-1","label_item_name":"Output",
                "executor_name":"Worker","worker_role":"aparatchi","worker_ref":WORKER,"worker_display_name":"Worker",
                "wip_status":"waiting","finished_goods_kg":20.0,"bobina_kg":0.5,"payload_json":{"stage_node_id":"cut"}
            })).unwrap();
            store.put_order_progress_batch(batch).await.unwrap();
            originals.push(store.progress_batch(id).await.unwrap().unwrap());
        }
    }
    state
        .production_maps
        .add_paddon_items(
            &source.code,
            &["http-output-a".into(), "http-output-b".into()],
            &actor,
        )
        .await
        .unwrap();
    let token = session_for(&state, PrincipalRole::Aparatchi, WORKER).await;
    let router = build_router(state.clone());
    let removed = post(
        &router,
        &token,
        "items/remove",
        json!({"code":source.code,"progress_batch_id":"http-output-a"}),
    )
    .await;
    assert!(candidate(&removed, "http-output-a"));
    assert!(
        candidate(&removed, "http-output-other-machine"),
        "single removal must retain cross-machine candidates"
    );
    assert!(!candidate(&removed, "http-output-unassigned"));
    let removed = post(
        &router,
        &token,
        "items/remove-batch",
        json!({"code":source.code,"progress_batch_ids":["http-output-b"]}),
    )
    .await;
    assert!(
        candidate(&removed, "http-output-other-machine"),
        "bulk removal must retain cross-machine candidates"
    );
    assert!(candidate(&removed, "http-output-b"));
    let added = post(
        &router,
        &token,
        "items/add",
        json!({"code":target.code,"qr_payload":"PROGRESS:http-output-a"}),
    )
    .await;
    assert_eq!(added["items"][0]["batch_id"], "http-output-a");
    assert!(candidate(&added, "http-output-b"));
    let added = post(&router, &token, "items/add-batch", json!({"code":target.code,"progress_batch_ids":["http-output-b","http-output-other-machine"]})).await;
    assert_eq!(added["items"].as_array().unwrap().len(), 3);
    post(&router, &token, "items/transfer", json!({"source_code":target.code,"target_code":source.code,"progress_batch_ids":["http-output-a"]})).await;

    // Sessions stay valid; every mutation must re-read the changed role assignment.
    assign(&state, &[CUT5]).await;
    for spoof in [false, true] {
        let mut body = json!({"source_code":source.code,"target_code":target.code,"progress_batch_ids":["http-output-a"]});
        if spoof {
            body["allowed_apparatuses"] = json!([CUT1, CUT5, CUT3]);
        }
        let denied = router
            .clone()
            .oneshot(request_with_body(
                "POST",
                &format!("{BASE}/items/transfer"),
                &token,
                &body.to_string(),
            ))
            .await
            .unwrap();
        assert!(
            !denied.status().is_success(),
            "client fields must not restore a revoked source assignment"
        );
    }
    let membership: String = sqlx::query_scalar("SELECT p.code FROM mini_paddon_items i JOIN mini_paddons p ON p.id=i.paddon_id WHERE i.progress_batch_id='http-output-a' AND i.removed_at IS NULL")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(membership, source.code);
    state
        .production_maps
        .remove_paddon_items(&source.code, &["http-output-a".into()], &actor)
        .await
        .unwrap();
    for body in [
        json!({"code":target.code,"qr_payload":"PROGRESS:http-output-a","allowed_apparatuses":[CUT1,CUT5]}),
        json!({"code":target.code,"progress_batch_ids":["http-output-a"],"allowed_apparatuses":[CUT1,CUT5]}),
    ] {
        let path = if body.get("qr_payload").is_some() {
            "items/add"
        } else {
            "items/add-batch"
        };
        let denied = router
            .clone()
            .oneshot(request_with_body(
                "POST",
                &format!("{BASE}/{path}"),
                &token,
                &body.to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(
            denied.status(),
            StatusCode::FORBIDDEN,
            "detached roll origins remain server-authorized"
        );
    }
    for original in originals {
        assert_eq!(
            store
                .progress_batch(&original.batch_id)
                .await
                .unwrap()
                .unwrap(),
            original
        );
    }
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {database} WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
