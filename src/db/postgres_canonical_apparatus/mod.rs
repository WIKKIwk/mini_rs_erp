mod cutover;
mod mutations;
mod projections;
mod reads;

use sqlx::PgPool;

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(crate) struct PostgresCanonicalApparatusRepository {
    pool: PgPool,
    #[cfg(test)]
    fault: std::sync::Arc<std::sync::Mutex<Option<CommitFaultPoint>>>,
}

impl PostgresCanonicalApparatusRepository {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self {
            pool,
            #[cfg(test)]
            fault: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    #[cfg(test)]
    pub(crate) fn inject_fault(&self, point: CommitFaultPoint) {
        *self.fault.lock().expect("fault lock") = Some(point);
    }

    fn fault_at(
        &self,
        point: CommitFaultPoint,
    ) -> Result<(), crate::core::apparatus_standard::CanonicalApparatusError> {
        #[cfg(test)]
        if self.fault.lock().expect("fault lock").as_ref() == Some(&point) {
            return Err(
                crate::core::apparatus_standard::CanonicalApparatusError::InjectedFault(
                    point.name(),
                ),
            );
        }
        #[cfg(not(test))]
        let _ = point;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommitFaultPoint {
    HeadLock,
    ExpectedRevision,
    CandidateValidation,
    ArtifactGeneration,
    Projection,
    IdentityInsert,
    RevisionInsert,
    HeadCas,
    RuntimeProjection,
    DerivedProjections,
    Outbox,
}

impl CommitFaultPoint {
    #[cfg(test)]
    fn name(self) -> &'static str {
        match self {
            Self::HeadLock => "after_head_lock",
            Self::ExpectedRevision => "after_expected_revision",
            Self::CandidateValidation => "after_candidate_validation",
            Self::ArtifactGeneration => "after_artifact_generation",
            Self::Projection => "after_projection",
            Self::IdentityInsert => "after_identity_insert",
            Self::RevisionInsert => "after_revision_insert",
            Self::HeadCas => "after_head_cas",
            Self::RuntimeProjection => "after_runtime_projection",
            Self::DerivedProjections => "after_derived_projections",
            Self::Outbox => "after_outbox",
        }
    }
}
