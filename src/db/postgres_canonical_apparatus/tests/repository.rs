use crate::core::apparatus_standard::{
    ApparatusDisplay, CanonicalApparatusError, CanonicalApparatusPatch, RevisionSource,
    export_canonical_aasx, parse_canonical_aasx, project_apparatus_revision,
};

use super::fixtures::{TestDatabase, apparatus_state, draft, metadata};

#[tokio::test]
async fn canonical_repository_round_trips_artifact_and_all_projections() {
    let database = TestDatabase::create("roundtrip").await;
    let service = database.service();
    let created = service
        .create(
            draft("physical-asset:repository-01", "Shared display"),
            metadata("command:repository-create-01"),
        )
        .await
        .expect("create canonical apparatus");
    let apparatus_id = created.revision.apparatus_id.clone();

    let stored = service
        .current_aasx(&apparatus_id)
        .await
        .expect("read stored AASX")
        .expect("stored AASX exists");
    let regenerated = export_canonical_aasx(&created.revision).expect("regenerate canonical AASX");
    assert_eq!(stored.revision, 1);
    assert_eq!(stored.artifact.bytes(), regenerated.bytes());
    assert_eq!(stored.artifact.sha256(), regenerated.sha256());
    assert_eq!(stored.artifact.sha256(), created.aasx_sha256);
    assert_eq!(
        parse_canonical_aasx(stored.artifact.bytes()).unwrap(),
        created.revision
    );

    let expected_projection = project_apparatus_revision(&created.revision, created.aasx_sha256);
    assert_eq!(created.runtime_projection, expected_projection.runtime);
    assert_eq!(
        service
            .current_projection(&apparatus_id)
            .await
            .unwrap()
            .as_deref(),
        Some(&expected_projection.runtime)
    );
    assert_eq!(service.list_runtime_projections().await.unwrap().len(), 1);

    let state = apparatus_state(&database.pool, &apparatus_id).await;
    assert_eq!(state.identities, 1);
    assert_eq!(state.revisions, 1);
    assert_eq!(state.heads, 1);
    assert_eq!(state.runtime, 1);
    assert_eq!(state.queue, 1);
    assert_eq!(state.material, 1);
    assert_eq!(state.capacity, 1);
    assert_eq!(state.outbox, 1);
    assert_eq!(state.head_revision, Some(1));
    assert_eq!(state.runtime_revision, Some(1));
    assert_eq!(state.drift, 0);
    database.close().await;
}

#[tokio::test]
async fn display_names_are_non_unique_and_rename_preserves_identity() {
    let database = TestDatabase::create("display").await;
    let service = database.service();
    let first = service
        .create(
            draft("physical-asset:display-01", "Shared display"),
            metadata("command:display-create-01"),
        )
        .await
        .unwrap();
    let second = service
        .create(
            draft("physical-asset:display-02", "Shared display"),
            metadata("command:display-create-02"),
        )
        .await
        .unwrap();
    assert_ne!(first.revision.apparatus_id, second.revision.apparatus_id);

    let renamed = service
        .patch(
            first.revision.apparatus_id.clone(),
            1,
            CanonicalApparatusPatch {
                display: Some(ApparatusDisplay {
                    display_name: "Renamed display".to_string(),
                    description: "Renamed without changing identity".to_string(),
                    catalog_order: 9,
                }),
                ..CanonicalApparatusPatch::default()
            },
            metadata("command:display-rename-01"),
        )
        .await
        .unwrap();
    assert_eq!(renamed.revision.apparatus_id, first.revision.apparatus_id);
    assert_eq!(
        renamed.revision.physical_asset_id,
        first.revision.physical_asset_id
    );
    assert_eq!(renamed.revision.revision_metadata.revision, 2);
    assert_eq!(renamed.revision.display.display_name, "Renamed display");
    let state = apparatus_state(&database.pool, &renamed.revision.apparatus_id).await;
    assert_eq!(state.revisions, 2);
    assert_eq!(state.outbox, 2);
    assert_eq!(state.head_revision, Some(2));
    assert_eq!(state.runtime_revision, Some(2));
    assert_eq!(state.drift, 0);
    database.close().await;
}

#[tokio::test]
async fn expected_revision_serializes_parallel_updates() {
    let database = TestDatabase::create("cas").await;
    let service = database.service();
    let created = service
        .create(
            draft("physical-asset:cas-01", "CAS fixture"),
            metadata("command:cas-create-01"),
        )
        .await
        .unwrap();
    let first_service = service.clone();
    let second_service = service.clone();
    let first_id = created.revision.apparatus_id.clone();
    let second_id = first_id.clone();
    let (first, second) = tokio::join!(
        first_service.patch(
            first_id,
            1,
            CanonicalApparatusPatch {
                display: Some(ApparatusDisplay {
                    display_name: "CAS winner A".to_string(),
                    description: "first contender".to_string(),
                    catalog_order: 1,
                }),
                ..CanonicalApparatusPatch::default()
            },
            metadata("command:cas-update-a"),
        ),
        second_service.patch(
            second_id,
            1,
            CanonicalApparatusPatch {
                display: Some(ApparatusDisplay {
                    display_name: "CAS winner B".to_string(),
                    description: "second contender".to_string(),
                    catalog_order: 2,
                }),
                ..CanonicalApparatusPatch::default()
            },
            metadata("command:cas-update-b"),
        )
    );
    let outcomes = [first, second];
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(result, Err(CanonicalApparatusError::RevisionConflict)))
            .count(),
        1
    );
    let state = apparatus_state(&database.pool, &created.revision.apparatus_id).await;
    assert_eq!(state.revisions, 2);
    assert_eq!(state.outbox, 2);
    assert_eq!(state.head_revision, Some(2));
    assert_eq!(state.runtime_revision, Some(2));
    assert_eq!(state.drift, 0);
    database.close().await;
}

#[tokio::test]
async fn retired_identity_is_immutable_and_physical_asset_is_unique() {
    let database = TestDatabase::create("identity").await;
    let service = database.service();
    let created = service
        .create(
            draft("physical-asset:identity-01", "Identity fixture"),
            metadata("command:identity-create-01"),
        )
        .await
        .unwrap();
    let duplicate = service
        .create(
            draft("physical-asset:identity-01", "Different apparatus"),
            metadata("command:identity-create-02"),
        )
        .await;
    assert_eq!(duplicate, Err(CanonicalApparatusError::AlreadyExists));

    let retired = service
        .retire(
            created.revision.apparatus_id.clone(),
            1,
            "asset-decommissioned".to_string(),
            metadata("command:identity-retire-01"),
        )
        .await
        .unwrap();
    assert_eq!(retired.revision.revision_metadata.revision, 2);
    let update_after_retire = service
        .update(
            created.revision.apparatus_id.clone(),
            2,
            created.revision.to_draft(),
            metadata("command:identity-update-retired"),
        )
        .await;
    assert_eq!(update_after_retire, Err(CanonicalApparatusError::Retired));
    let state = apparatus_state(&database.pool, &created.revision.apparatus_id).await;
    assert_eq!(state.identities, 1);
    assert_eq!(state.revisions, 2);
    assert_eq!(state.outbox, 2);
    assert_eq!(state.head_revision, Some(2));
    assert_eq!(state.drift, 0);
    database.close().await;
}

#[tokio::test]
async fn full_update_and_aasx_replace_each_create_one_canonical_revision() {
    let database = TestDatabase::create("replace").await;
    let service = database.service();
    let created = service
        .create(
            draft("physical-asset:replace-01", "Replace fixture"),
            metadata("command:replace-create-01"),
        )
        .await
        .unwrap();
    let apparatus_id = created.revision.apparatus_id.clone();
    let mut updated_draft = created.revision.to_draft();
    updated_draft.display.description = "complete draft update".to_string();
    let updated = service
        .update(
            apparatus_id.clone(),
            1,
            updated_draft,
            metadata("command:replace-update-01"),
        )
        .await
        .unwrap();
    assert_eq!(updated.revision.revision_metadata.revision, 2);

    let upload = service
        .current_aasx(&apparatus_id)
        .await
        .unwrap()
        .unwrap()
        .artifact
        .bytes()
        .to_vec();
    let replaced = service
        .replace_from_aasx(
            apparatus_id.clone(),
            2,
            &upload,
            metadata("command:replace-aasx-01"),
        )
        .await
        .unwrap();
    assert_eq!(replaced.revision.revision_metadata.revision, 3);
    assert_eq!(
        replaced.revision.revision_metadata.source,
        RevisionSource::AasxImport
    );
    assert_eq!(
        replaced.revision.revision_metadata.source_reference,
        Some(format!(
            "sha256:{}",
            crate::core::apparatus_standard::AasxSha256::digest(&upload)
        ))
    );
    let stored = service.current_aasx(&apparatus_id).await.unwrap().unwrap();
    assert_ne!(stored.artifact.bytes(), upload);
    assert_eq!(
        parse_canonical_aasx(stored.artifact.bytes()).unwrap(),
        replaced.revision
    );
    let state = apparatus_state(&database.pool, &apparatus_id).await;
    assert_eq!(state.revisions, 3);
    assert_eq!(state.outbox, 3);
    assert_eq!(state.head_revision, Some(3));
    assert_eq!(state.runtime_revision, Some(3));
    assert_eq!(state.drift, 0);
    database.close().await;
}
