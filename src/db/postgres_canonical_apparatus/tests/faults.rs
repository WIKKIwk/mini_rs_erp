use crate::core::apparatus_standard::{CanonicalApparatusError, CanonicalApparatusPatch};

use super::super::{CommitFaultPoint, PostgresCanonicalApparatusRepository};
use super::fixtures::{TestDatabase, apparatus_state, draft, metadata};

const FAULT_POINTS: [CommitFaultPoint; 11] = [
    CommitFaultPoint::AfterHeadLock,
    CommitFaultPoint::AfterExpectedRevision,
    CommitFaultPoint::AfterCandidateValidation,
    CommitFaultPoint::AfterArtifactGeneration,
    CommitFaultPoint::AfterProjection,
    CommitFaultPoint::AfterIdentityInsert,
    CommitFaultPoint::AfterRevisionInsert,
    CommitFaultPoint::AfterHeadCas,
    CommitFaultPoint::AfterRuntimeProjection,
    CommitFaultPoint::AfterDerivedProjections,
    CommitFaultPoint::AfterOutbox,
];

#[tokio::test]
async fn every_transaction_boundary_rolls_back_without_partial_state() {
    let database = TestDatabase::create("faults").await;
    let seed_service = database.service();
    let created = seed_service
        .create(
            draft("physical-asset:fault-01", "Fault fixture"),
            metadata("command:fault-create-01"),
        )
        .await
        .unwrap();
    let apparatus_id = created.revision.apparatus_id.clone();
    let baseline = apparatus_state(&database.pool, &apparatus_id).await;

    for (index, point) in FAULT_POINTS.into_iter().enumerate() {
        let repository = PostgresCanonicalApparatusRepository::new(database.pool.clone());
        repository.inject_fault(point);
        let service = crate::core::apparatus_standard::CanonicalApparatusService::new(
            std::sync::Arc::new(repository),
        );
        let result = service
            .patch(
                apparatus_id.clone(),
                1,
                CanonicalApparatusPatch::default(),
                metadata(format!("command:fault-update-{index:02}")),
            )
            .await;
        assert!(matches!(
            result,
            Err(CanonicalApparatusError::InjectedFault(_))
        ));
        assert_eq!(
            apparatus_state(&database.pool, &apparatus_id).await,
            baseline
        );
    }
    database.close().await;
}
