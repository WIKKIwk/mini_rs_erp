use super::*;

const PRINT: &str = "apparatus:default:bosma_7";
const OTHER: &str = "apparatus:default:bosma_8";
const ORDER: &str = "zakaz-shared-control";
const WORKER: &str = "shared-control-worker";
const PATH: &str = "/v1/mobile/admin/production-maps/print-preflight";
static FIXTURE_LOCK: Mutex<()> = Mutex::const_new(());

async fn assign(state: &AppState, apparatus: &str) {
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: WORKER.into(),
            role_id: "aparatchi".into(),
            assigned_apparatus: vec![apparatus.into()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .unwrap();
}

async fn fixture() -> (AppState, String) {
    let state = test_state();
    assign(&state, PRINT).await;
    let token = session_for(&state, PrincipalRole::Aparatchi, WORKER).await;
    state
        .production_maps
        .upsert_map(
            serde_json::from_str(&pechat_order_map_json(
                ORDER,
                "Shared control",
                "9101",
                PRINT,
            ))
            .unwrap(),
        )
        .await
        .unwrap();
    state
        .production_maps
        .worker_snapshot_shared_with_revision(&[PRINT.into()], &[])
        .await
        .unwrap();
    (state, token)
}

async fn start_committed_hold(
    state: &AppState,
    token: &str,
) -> tokio::task::JoinHandle<axum::response::Response> {
    let mut commits = state.production_maps.subscribe_live();
    let revision = state.production_maps.snapshot_revision();
    let body = serde_json::json!({
        "apparatus": PRINT,
        "order_id": ORDER,
        "action": "hold",
        "hold_id": "shared-control-hold",
        "idempotency_key": "shared-control-hold",
        "include_control": true,
    });
    let request = request_with_body("POST", PATH, token, &body.to_string());
    let router = build_router(state.clone());
    let pending = tokio::spawn(async move { router.oneshot(request).await.unwrap() });
    tokio::time::timeout(Duration::from_secs(2), commits.recv())
        .await
        .expect("Mutation must commit before presentation waits")
        .unwrap();
    assert!(state.production_maps.snapshot_revision() > revision);
    pending
}

#[tokio::test]
async fn committed_control_waits_for_shared_snapshot_beyond_250ms() {
    let _fixture = FIXTURE_LOCK.lock().await;
    let (state, token) = fixture().await;
    let guard = state
        .production_maps
        .lock_worker_snapshot_for_test(&[PRINT.into()])
        .await;
    let pending = start_committed_hold(&state, &token).await;
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert!(
        !pending.is_finished(),
        "Shared computation must not cause an early null control at 250ms"
    );
    drop(guard);
    let response = tokio::time::timeout(Duration::from_secs(2), pending)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["ok"], true);
    assert!(body["control_state"].is_object());
    assert_eq!(body["control_state"]["order_id"], ORDER);
    assert_eq!(
        body["control_state"]["rev"],
        state.production_maps.snapshot_revision()
    );
    let (canonical, _) = state
        .production_maps
        .worker_snapshot_shared_with_revision(&[PRINT.into()], &[ORDER.into()])
        .await
        .unwrap();
    assert_eq!(
        body["control_state"]["control"],
        serde_json::to_value(&canonical.queue_action_controls[PRINT][ORDER]).unwrap()
    );
}

#[tokio::test]
async fn committed_control_still_acknowledges_when_shared_wait_budget_expires() {
    let _fixture = FIXTURE_LOCK.lock().await;
    let (state, token) = fixture().await;
    let guard = state
        .production_maps
        .lock_worker_snapshot_for_test(&[PRINT.into()])
        .await;
    let started = tokio::time::Instant::now();
    let pending = start_committed_hold(&state, &token).await;
    let response = tokio::time::timeout(Duration::from_secs(2), pending)
        .await
        .expect("Committed mutation must have a bounded presentation wait")
        .unwrap();
    assert!(started.elapsed() >= Duration::from_millis(900));
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["ok"], true);
    assert_eq!(body["hold"]["hold_id"], "shared-control-hold");
    assert!(body["control_state"].is_null());
    drop(guard);
    let (canonical, _) = state
        .production_maps
        .worker_snapshot_shared_with_revision(&[PRINT.into()], &[])
        .await
        .unwrap();
    assert!(canonical.queue_action_controls[PRINT].contains_key(ORDER));
}

#[tokio::test]
async fn committed_control_rechecks_assignment_after_shared_snapshot_wait() {
    let _fixture = FIXTURE_LOCK.lock().await;
    let (state, token) = fixture().await;
    let guard = state
        .production_maps
        .lock_worker_snapshot_for_test(&[PRINT.into()])
        .await;
    let pending = start_committed_hold(&state, &token).await;
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert!(!pending.is_finished());
    assign(&state, OTHER).await;
    drop(guard);
    let response = tokio::time::timeout(Duration::from_millis(500), pending)
        .await
        .expect("Revoked scope must be rejected before the presentation deadline")
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["ok"], true);
    assert!(body["control_state"].is_null());
}
