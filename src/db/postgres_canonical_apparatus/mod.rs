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
        #[cfg_attr(not(test), allow(unused_variables))] point: CommitFaultPoint,
    ) -> Result<(), crate::core::apparatus_standard::CanonicalApparatusError> {
        #[cfg(test)]
        if self.fault.lock().expect("fault lock").as_ref() == Some(&point) {
            return Err(
                crate::core::apparatus_standard::CanonicalApparatusError::InjectedFault(
                    point.name(),
                ),
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommitFaultPoint {
    AfterHeadLock,
    AfterExpectedRevision,
    AfterCandidateValidation,
    AfterArtifactGeneration,
    AfterProjection,
    AfterIdentityInsert,
    AfterRevisionInsert,
    AfterHeadCas,
    AfterRuntimeProjection,
    AfterDerivedProjections,
    AfterOutbox,
}

impl CommitFaultPoint {
    fn name(self) -> &'static str {
        match self {
            Self::AfterHeadLock => "after_head_lock",
            Self::AfterExpectedRevision => "after_expected_revision",
            Self::AfterCandidateValidation => "after_candidate_validation",
            Self::AfterArtifactGeneration => "after_artifact_generation",
            Self::AfterProjection => "after_projection",
            Self::AfterIdentityInsert => "after_identity_insert",
            Self::AfterRevisionInsert => "after_revision_insert",
            Self::AfterHeadCas => "after_head_cas",
            Self::AfterRuntimeProjection => "after_runtime_projection",
            Self::AfterDerivedProjections => "after_derived_projections",
            Self::AfterOutbox => "after_outbox",
        }
    }
}
