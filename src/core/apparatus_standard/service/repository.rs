use async_trait::async_trait;

use super::{CanonicalApparatusError, CanonicalApparatusPatch, CanonicalCommandMetadata};
use crate::core::apparatus_standard::{
    ApparatusId, CanonicalAasxArtifact, CanonicalApparatusDraft, CanonicalApparatusRevision,
    LifecycleState, RevisionMetadata, RevisionSource, RuntimeApparatusConfiguration,
    RuntimeApparatusProjection,
};

pub(crate) struct CanonicalWritePermit {
    _private: (),
}

impl CanonicalWritePermit {
    pub(super) fn new() -> Self {
        Self { _private: () }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum CanonicalRevisionIntent {
    Create {
        apparatus_id: ApparatusId,
        draft: CanonicalApparatusDraft,
        metadata: CanonicalCommandMetadata,
    },
    Update {
        apparatus_id: ApparatusId,
        expected_revision: u64,
        draft: CanonicalApparatusDraft,
        metadata: CanonicalCommandMetadata,
    },
    Patch {
        apparatus_id: ApparatusId,
        expected_revision: u64,
        patch: CanonicalApparatusPatch,
        metadata: CanonicalCommandMetadata,
    },
    ReplaceFromAasx {
        apparatus_id: ApparatusId,
        expected_revision: u64,
        draft: CanonicalApparatusDraft,
        metadata: CanonicalCommandMetadata,
    },
    Retire {
        apparatus_id: ApparatusId,
        expected_revision: u64,
        retirement_reason: String,
        metadata: CanonicalCommandMetadata,
    },
}

impl CanonicalRevisionIntent {
    pub fn apparatus_id(&self) -> &ApparatusId {
        match self {
            Self::Create { apparatus_id, .. }
            | Self::Update { apparatus_id, .. }
            | Self::Patch { apparatus_id, .. }
            | Self::ReplaceFromAasx { apparatus_id, .. }
            | Self::Retire { apparatus_id, .. } => apparatus_id,
        }
    }

    pub fn expected_revision(&self) -> Option<u64> {
        match self {
            Self::Create { .. } => None,
            Self::Update {
                expected_revision, ..
            }
            | Self::Patch {
                expected_revision, ..
            }
            | Self::ReplaceFromAasx {
                expected_revision, ..
            }
            | Self::Retire {
                expected_revision, ..
            } => Some(*expected_revision),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CommittedCanonicalApparatus {
    pub revision: CanonicalApparatusRevision,
    pub runtime_projection: RuntimeApparatusProjection,
    pub aasx_sha256: super::super::AasxSha256,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCanonicalAasx {
    pub apparatus_id: ApparatusId,
    pub revision: u64,
    pub artifact: CanonicalAasxArtifact,
}

#[async_trait]
pub(crate) trait CanonicalApparatusRepository: Send + Sync {
    async fn commit(
        &self,
        permit: &CanonicalWritePermit,
        intent: CanonicalRevisionIntent,
    ) -> Result<CommittedCanonicalApparatus, CanonicalApparatusError>;

    async fn current_projection(
        &self,
        apparatus_id: &ApparatusId,
    ) -> Result<Option<RuntimeApparatusProjection>, CanonicalApparatusError>;

    async fn current_aasx(
        &self,
        apparatus_id: &ApparatusId,
    ) -> Result<Option<StoredCanonicalAasx>, CanonicalApparatusError>;

    async fn current_configuration(
        &self,
        apparatus_id: &ApparatusId,
    ) -> Result<Option<RuntimeApparatusConfiguration>, CanonicalApparatusError>;

    async fn list_runtime_projections(
        &self,
    ) -> Result<Vec<RuntimeApparatusProjection>, CanonicalApparatusError>;
}

pub(crate) fn materialize_revision(
    current: Option<&CanonicalApparatusRevision>,
    intent: CanonicalRevisionIntent,
) -> Result<(CanonicalApparatusRevision, &'static str), CanonicalApparatusError> {
    let (apparatus_id, mut draft, metadata, source, event_type) = match intent {
        CanonicalRevisionIntent::Create {
            apparatus_id,
            draft,
            metadata,
        } => {
            if current.is_some() {
                return Err(CanonicalApparatusError::AlreadyExists);
            }
            (
                apparatus_id,
                draft,
                metadata,
                RevisionSource::Admin,
                "apparatus_created",
            )
        }
        CanonicalRevisionIntent::Update {
            apparatus_id,
            draft,
            metadata,
            ..
        } => (
            apparatus_id,
            draft,
            metadata,
            RevisionSource::Admin,
            "apparatus_updated",
        ),
        CanonicalRevisionIntent::Patch {
            apparatus_id,
            patch,
            metadata,
            ..
        } => {
            let current = current.ok_or(CanonicalApparatusError::NotFound)?;
            let mut draft = current.to_draft();
            apply_patch(&mut draft, patch);
            (
                apparatus_id,
                draft,
                metadata,
                RevisionSource::Admin,
                "apparatus_updated",
            )
        }
        CanonicalRevisionIntent::ReplaceFromAasx {
            apparatus_id,
            draft,
            metadata,
            ..
        } => (
            apparatus_id,
            draft,
            metadata,
            RevisionSource::AasxImport,
            "apparatus_updated",
        ),
        CanonicalRevisionIntent::Retire {
            apparatus_id,
            retirement_reason,
            metadata,
            ..
        } => {
            let current = current.ok_or(CanonicalApparatusError::NotFound)?;
            let mut draft = current.to_draft();
            draft.lifecycle.state = LifecycleState::Retired;
            draft.lifecycle.retirement_reason = Some(retirement_reason);
            (
                apparatus_id,
                draft,
                metadata,
                RevisionSource::Admin,
                "apparatus_retired",
            )
        }
    };
    let next_revision = match current {
        Some(current) => {
            if !current.is_active() {
                return Err(CanonicalApparatusError::Retired);
            }
            if current.apparatus_id != apparatus_id
                || current.physical_asset_id != draft.physical_asset_id
            {
                return Err(CanonicalApparatusError::IdentityConflict);
            }
            current
                .revision_metadata
                .revision
                .checked_add(1)
                .ok_or(CanonicalApparatusError::RevisionConflict)?
        }
        None => 1,
    };
    draft.normalize();
    let revision = CanonicalApparatusRevision::from_draft(
        apparatus_id,
        draft,
        RevisionMetadata {
            revision: next_revision,
            committed_at_unix_ms: metadata.committed_at_unix_ms,
            actor_id: metadata.actor_id,
            command_id: metadata.command_id,
            source,
            source_reference: metadata.source_reference,
        },
    )?;
    Ok((revision, event_type))
}

fn apply_patch(draft: &mut CanonicalApparatusDraft, patch: CanonicalApparatusPatch) {
    if let Some(value) = patch.display {
        draft.display = value;
    }
    if let Some(value) = patch.equipment_class_id {
        draft.equipment_class_id = value;
    }
    if let Some(value) = patch.hierarchy {
        draft.hierarchy = value;
    }
    if let Some(value) = patch.capabilities {
        draft.capabilities = value;
    }
    if let Some(value) = patch.execution_profile {
        draft.execution_profile = value;
    }
    if let Some(value) = patch.policies {
        draft.policies = value;
    }
    if let Some(value) = patch.capacity {
        draft.capacity = value;
    }
    if let Some(value) = patch.placement {
        draft.placement = value;
    }
    if let Some(value) = patch.training {
        draft.training = value;
    }
}
