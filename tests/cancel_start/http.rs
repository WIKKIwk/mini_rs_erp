use super::*;
use axum::{body::Body, http::Request};
use mini_rs_erp::{
    app::AppState,
    config::AppConfig,
    core::{
        auth::models::{Principal, PrincipalRole},
        authz::RoleAssignmentUpsert,
    },
    http::router::build_router,
};
use tower::ServiceExt;

#[tokio::test]
#[ignore = "requires CANCEL_START_TEST_DATABASE_URL pointing at a disposable PostgreSQL server"]
async fn endpoint_requires_worker_authority_and_rejects_client_metrics() {
    let f = Fixture::new(MACHINES[0]).await;
    let control = f.start().await;
    let dir = tempfile::tempdir().unwrap();
    struct RestoreDirectory(std::path::PathBuf);
    impl Drop for RestoreDirectory {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.0).unwrap();
        }
    }
    let _cwd = RestoreDirectory(std::env::current_dir().unwrap());
    std::env::set_current_dir(dir.path()).unwrap();
    let mut state = AppState::verification(AppConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        default_target_warehouse: String::new(),
        http_timeout: std::time::Duration::from_secs(5),
        session_store_path: dir.path().join("sessions"),
        profile_store_path: dir.path().join("profiles"),
        push_token_store_path: dir.path().join("push"),
        session_ttl_seconds: Some(3600),
        supplier_prefix: "10".into(),
        werka_prefix: "20".into(),
        werka_code: String::new(),
        werka_name: String::new(),
        werka_phone: String::new(),
        material_taminotchi_code: String::new(),
        material_taminotchi_name: String::new(),
        material_taminotchi_phone: String::new(),
        admin_phone: String::new(),
        admin_name: String::new(),
        admin_code: String::new(),
    });
    state.apparatus = postgres::canonical_apparatus_service(f.pool.clone());
    state.production_maps = f.service.clone();
    state
        .admin
        .upsert_role_assignment(RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: actor().ref_,
            role_id: "aparatchi".into(),
            assigned_apparatus: vec![f.machine.clone()],
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
    let mut tokens = vec![];
    for (role, who) in [
        (PrincipalRole::Aparatchi, actor().ref_),
        (PrincipalRole::Admin, "admin".into()),
    ] {
        tokens.push(
            state
                .sessions
                .create(Principal {
                    role,
                    ref_: who,
                    display_name: "Worker".into(),
                    legal_name: String::new(),
                    avatar_url: String::new(),
                    phone: String::new(),
                })
                .await
                .unwrap(),
        );
    }
    let app = build_router(state);
    let body = json!({"apparatus":f.machine,"order_id":f.order,"session_id":control.session_id,"token":control.token});
    for (token, payload, status) in [
        (None, body.clone(), 401),
        (Some(&tokens[1]), body.clone(), 403),
        (
            Some(&tokens[0]),
            {
                let mut b = body.clone();
                b["produced_qty"] = json!(0);
                b
            },
            400,
        ),
        (Some(&tokens[0]), body.clone(), 200),
        (Some(&tokens[0]), body.clone(), 200),
    ] {
        let mut request = Request::builder()
            .method("POST")
            .uri("/v1/mobile/admin/production-maps/cancel-start")
            .header("Content-Type", "application/json");
        if let Some(token) = token {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::from(payload.to_string())).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status);
    }
    assert_eq!(
        f.batch().await.wip_status,
        OrderProgressBatchWipStatus::Waiting
    );

    // The history is an independent, non-cached read. Its return command uses
    // only server-issued authority and rejects client-supplied work metrics.
    let fresh=f.start().await;
    let uri=format!("/v1/mobile/admin/production-maps/scanned-wips?apparatus={}&order_id={}&stage_node_id=work",f.machine,f.order);
    for (token,status) in [(None,401),(Some(&tokens[1]),403)] {
        let mut request=Request::builder().uri(&uri);
        if let Some(token)=token { request=request.header("Authorization",format!("Bearer {token}")); }
        let response=app.clone().oneshot(request.body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status().as_u16(),status);
    }
    let response=app.clone().oneshot(Request::builder().uri(&uri)
        .header("Authorization",format!("Bearer {}",tokens[0])).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status().as_u16(),200);
    assert_eq!(response.headers()["cache-control"],"no-store");
    let bytes=axum::body::to_bytes(response.into_body(),1024*1024).await.unwrap();
    let history:Value=serde_json::from_slice(&bytes).unwrap();
    let item=history["items"].as_array().unwrap().iter()
        .find(|row|row["session_id"]==fresh.session_id).unwrap();
    let target=json!({"apparatus":f.machine,"order_id":f.order,"stage_node_id":"work",
        "session_id":fresh.session_id,"batch_id":f.original.batch_id,"token":item["return_token"]});
    for (token,body,status) in [
        (None,target.clone(),401),
        (Some(&tokens[1]),target.clone(),403),
        (Some(&tokens[0]),{let mut b=target.clone();b["produced_qty"]=json!(0);b},400),
        (Some(&tokens[0]),target.clone(),200),
        (Some(&tokens[0]),target.clone(),200),
    ] {
        let mut request=Request::builder().method("POST")
            .uri("/v1/mobile/admin/production-maps/scanned-wips/return").header("Content-Type","application/json");
        if let Some(token)=token { request=request.header("Authorization",format!("Bearer {token}")); }
        let response=app.clone().oneshot(request.body(Body::from(body.to_string())).unwrap()).await.unwrap();
        assert_eq!(response.status().as_u16(),status);
    }
    assert_eq!(f.batch().await.wip_status,OrderProgressBatchWipStatus::Waiting);
}
