use super::*;
use crate::core::apparatus_groups::{ApparatusMasterData, ApparatusUpsert};
use crate::core::apparatus_standard::aasx::import_aasx;
use crate::core::apparatus_standard::{AASX_MEDIA_TYPE, ApparatusId};
use crate::http::handlers::admin::MAX_AASX_UPLOAD_BYTES;

fn aasx_path(id: &str) -> String {
    format!("/v1/mobile/admin/apparatus/{id}/aasx")
}

fn aasx_request(method: &str, uri: String, token: &str, body: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, AASX_MEDIA_TYPE)
        .body(Body::from(body))
        .expect("AASX request")
}

async fn seed_apparatus(state: &AppState, id: &str, name: &str) {
    state
        .apparatus_groups
        .upsert_apparatus(ApparatusUpsert {
            id: Some(id.to_string()),
            name: name.to_string(),
            master: ApparatusMasterData::default(),
        })
        .await
        .expect("seed apparatus");
}

#[tokio::test]
async fn admin_aasx_export_and_import_use_canonical_service_revision_flow() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let id = "apparatus:test:aasx-001";
    seed_apparatus(&state, id, "AASX boundary apparatus").await;

    let response = build_router(state.clone())
        .oneshot(aasx_request("GET", aasx_path(id), &token, Vec::new()))
        .await
        .expect("AASX export response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], AASX_MEDIA_TYPE);
    assert_eq!(
        response.headers()[header::CONTENT_DISPOSITION],
        "attachment; filename=\"apparatus-apparatus_test_aasx-001.aasx\""
    );
    let package = to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .expect("AASX package body")
        .to_vec();
    let exported = import_aasx(&package).expect("exported package parses");
    assert_eq!(exported.identity.id.as_str(), id);
    assert_eq!(exported.versioning.revision, 1);

    let mut live_notifications = state.production_maps.subscribe_live();
    let response = build_router(state.clone())
        .oneshot(aasx_request("POST", aasx_path(id), &token, package))
        .await
        .expect("AASX import response");
    assert_eq!(response.status(), StatusCode::OK);
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        live_notifications.recv(),
    )
    .await
    .expect("AASX mutation live notification")
    .expect("AASX live notification channel");
    let imported = json_body(response).await;
    assert_eq!(imported["identity"]["id"], id);
    assert_eq!(imported["versioning"]["revision"], 2);

    let apparatus_id = ApparatusId::new(id).expect("apparatus id");
    let canonical = state
        .apparatus_groups
        .canonical_apparatus_by_id(&apparatus_id)
        .await
        .expect("canonical lookup")
        .expect("canonical apparatus");
    assert_eq!(canonical.versioning.revision, 2);
}

#[tokio::test]
async fn admin_aasx_import_rejects_malformed_and_identity_conflicting_packages() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let target_id = "apparatus:test:aasx-target";
    let source_id = "apparatus:test:aasx-source";
    seed_apparatus(&state, target_id, "AASX target apparatus").await;
    seed_apparatus(&state, source_id, "AASX source apparatus").await;

    let malformed = build_router(state.clone())
        .oneshot(aasx_request(
            "POST",
            aasx_path(target_id),
            &token,
            b"not an AASX package".to_vec(),
        ))
        .await
        .expect("malformed response");
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(malformed).await["error"], "aasx_import_invalid");

    let exported = build_router(state.clone())
        .oneshot(aasx_request(
            "GET",
            aasx_path(source_id),
            &token,
            Vec::new(),
        ))
        .await
        .expect("source export response");
    assert_eq!(exported.status(), StatusCode::OK);
    let package = to_bytes(exported.into_body(), 16 * 1024 * 1024)
        .await
        .expect("source package body")
        .to_vec();

    let conflicting = build_router(state.clone())
        .oneshot(aasx_request("POST", aasx_path(target_id), &token, package))
        .await
        .expect("conflicting response");
    assert_eq!(conflicting.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(conflicting).await["error"],
        "aasx_identity_conflict"
    );

    let target = state
        .apparatus_groups
        .canonical_apparatus_by_id(&ApparatusId::new(target_id).expect("target apparatus id"))
        .await
        .expect("target lookup")
        .expect("target canonical apparatus");
    assert_eq!(target.versioning.revision, 1);
}

#[tokio::test]
async fn admin_aasx_replay_of_exported_revision_returns_conflict_without_mutation() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let id = "apparatus:test:aasx-replay";
    seed_apparatus(&state, id, "AASX replay apparatus").await;

    let exported = build_router(state.clone())
        .oneshot(aasx_request("GET", aasx_path(id), &token, Vec::new()))
        .await
        .expect("AASX export response");
    assert_eq!(exported.status(), StatusCode::OK);
    let package = to_bytes(exported.into_body(), MAX_AASX_UPLOAD_BYTES)
        .await
        .expect("AASX package body")
        .to_vec();

    let imported = build_router(state.clone())
        .oneshot(aasx_request("POST", aasx_path(id), &token, package.clone()))
        .await
        .expect("AASX import response");
    assert_eq!(imported.status(), StatusCode::OK);

    let after_import = state
        .apparatus_groups
        .canonical_apparatus_by_id(&ApparatusId::new(id).expect("apparatus id"))
        .await
        .expect("canonical lookup")
        .expect("canonical apparatus");
    assert_eq!(after_import.versioning.revision, 2);

    let replay = build_router(state.clone())
        .oneshot(aasx_request("POST", aasx_path(id), &token, package))
        .await
        .expect("stale AASX import response");
    assert_eq!(replay.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(replay).await["error"], "aasx_revision_conflict");

    let after_replay = state
        .apparatus_groups
        .canonical_apparatus_by_id(&ApparatusId::new(id).expect("apparatus id"))
        .await
        .expect("canonical lookup after replay")
        .expect("canonical apparatus after replay");
    assert_eq!(after_replay, after_import);
}

#[tokio::test]
async fn concurrent_stale_aasx_mutations_return_one_conflict_without_extra_revision() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let id = "apparatus:test:aasx-concurrent";
    seed_apparatus(&state, id, "AASX concurrent apparatus").await;

    let exported = build_router(state.clone())
        .oneshot(aasx_request("GET", aasx_path(id), &token, Vec::new()))
        .await
        .expect("AASX export response");
    assert_eq!(exported.status(), StatusCode::OK);
    let package = to_bytes(exported.into_body(), MAX_AASX_UPLOAD_BYTES)
        .await
        .expect("AASX package body")
        .to_vec();

    let (first, second) = tokio::join!(
        build_router(state.clone()).oneshot(aasx_request(
            "POST",
            aasx_path(id),
            &token,
            package.clone(),
        )),
        build_router(state.clone()).oneshot(aasx_request(
            "POST",
            aasx_path(id),
            &token,
            package,
        )),
    );
    let first = first.expect("first concurrent AASX response");
    let second = second.expect("second concurrent AASX response");
    let first_status = first.status();
    let second_status = second.status();
    assert_eq!(
        [first_status, second_status]
            .into_iter()
            .filter(|status| *status == StatusCode::OK)
            .count(),
        1
    );
    assert_eq!(
        [first_status, second_status]
            .into_iter()
            .filter(|status| *status == StatusCode::CONFLICT)
            .count(),
        1
    );
    let first_body = json_body(first).await;
    let second_body = json_body(second).await;
    let conflict_error = if first_status == StatusCode::CONFLICT {
        &first_body["error"]
    } else {
        &second_body["error"]
    };
    assert_eq!(conflict_error, "aasx_revision_conflict");

    let final_canonical = state
        .apparatus_groups
        .canonical_apparatus_by_id(&ApparatusId::new(id).expect("apparatus id"))
        .await
        .expect("final canonical lookup")
        .expect("final canonical apparatus");
    assert_eq!(final_canonical.versioning.revision, 2);
}

#[tokio::test]
async fn admin_apparatus_mutation_invalidates_live_snapshot() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let mut live_notifications = state.production_maps.subscribe_live();

    let response = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/apparatus",
            &token,
            r#"{
                "id": "apparatus:test:asset-999",
                "name": "Admin notification apparatus"
            }"#,
        ))
        .await
        .expect("admin apparatus mutation response");
    assert_eq!(response.status(), StatusCode::OK);
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        live_notifications.recv(),
    )
    .await
    .expect("admin mutation live notification")
    .expect("admin live notification channel");
}

#[tokio::test]
async fn admin_aasx_upload_over_limit_returns_413_without_mutation() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let id = "apparatus:test:aasx-too-large";
    seed_apparatus(&state, id, "AASX size apparatus").await;

    let before_upload = state
        .apparatus_groups
        .canonical_apparatus_by_id(&ApparatusId::new(id).expect("apparatus id"))
        .await
        .expect("canonical lookup")
        .expect("canonical apparatus");

    let response = build_router(state.clone())
        .oneshot(aasx_request(
            "POST",
            aasx_path(id),
            &token,
            vec![0u8; MAX_AASX_UPLOAD_BYTES + 1],
        ))
        .await
        .expect("oversized AASX response");
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let after_upload = state
        .apparatus_groups
        .canonical_apparatus_by_id(&ApparatusId::new(id).expect("apparatus id"))
        .await
        .expect("canonical lookup after oversized upload")
        .expect("canonical apparatus after oversized upload");
    assert_eq!(after_upload, before_upload);
}
