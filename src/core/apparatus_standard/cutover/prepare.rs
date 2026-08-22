use std::collections::{BTreeMap, BTreeSet};

use super::model::{
    CUTOVER_MANIFEST_VERSION, CutoverPreflightReport, LegacyApparatusInventory,
    LegacyCutoverDraftManifest, LegacyCutoverManifest, LegacyCutoverManifestEntry,
    ResolvedCutoverEntry, ResolvedCutoverManifest,
};
use crate::core::apparatus_standard::{
    ApparatusId, ApparatusProjectionSet, CanonicalAasxArtifact, CanonicalApparatusError,
    CanonicalApparatusRevision, RevisionMetadata, RevisionSource, export_canonical_aasx,
    parse_canonical_aasx, project_apparatus_revision,
};

pub(crate) struct PreparedCutoverEntry {
    pub legacy_apparatus_id: String,
    pub revision: CanonicalApparatusRevision,
    pub artifact: CanonicalAasxArtifact,
    pub projections: ApparatusProjectionSet,
}

pub(crate) struct PreparedCutoverPlan {
    pub preflight_fingerprint: super::super::AasxSha256,
    pub entries: Vec<PreparedCutoverEntry>,
}

pub fn build_cutover_manifest(
    report: &CutoverPreflightReport,
    mut draft_manifest: LegacyCutoverDraftManifest,
) -> Result<LegacyCutoverManifest, CanonicalApparatusError> {
    if draft_manifest.manifest_version != CUTOVER_MANIFEST_VERSION
        || draft_manifest.preflight_fingerprint != report.fingerprint
        || !report.blocking_issues.is_empty()
    {
        return blocked("draft manifest does not match a clean current preflight");
    }
    draft_manifest
        .entries
        .sort_by(|left, right| left.legacy_apparatus_id.cmp(&right.legacy_apparatus_id));
    let inventory = report
        .legacy_apparatuses
        .iter()
        .map(|item| (item.apparatus_id.as_str(), item))
        .collect::<BTreeMap<_, _>>();
    if inventory.len() != draft_manifest.entries.len() {
        return blocked("draft manifest does not cover every legacy apparatus exactly once");
    }
    let mut entries = Vec::with_capacity(draft_manifest.entries.len());
    for entry in draft_manifest.entries {
        let legacy = inventory
            .get(entry.legacy_apparatus_id.as_str())
            .ok_or_else(|| blocked_error("draft manifest contains an unknown legacy apparatus"))?;
        let apparatus_id = ApparatusId::new(entry.legacy_apparatus_id.clone())
            .map_err(|_| blocked_error("draft manifest contains an invalid stable apparatus ID"))?;
        let revision = CanonicalApparatusRevision::from_draft(
            apparatus_id,
            entry.canonical_draft,
            RevisionMetadata {
                revision: 1,
                committed_at_unix_ms: entry.committed_at_unix_ms,
                actor_id: entry.actor_id,
                command_id: entry.command_id,
                source: RevisionSource::LegacyMigration,
                source_reference: Some(format!("preflight-sha256:{}", report.fingerprint)),
            },
        )?;
        let artifact = export_canonical_aasx(&revision)
            .map_err(|_| CanonicalApparatusError::ArtifactIntegrity)?;
        entries.push(LegacyCutoverManifestEntry {
            legacy_apparatus_id: legacy.apparatus_id.clone(),
            acknowledged_identities: legacy.observed_identities.clone(),
            acknowledged_configuration_sources: source_hashes(&legacy.configuration_sources),
            canonical_revision: revision,
            expected_aasx_sha256: artifact.sha256(),
        });
    }
    let manifest = LegacyCutoverManifest {
        manifest_version: CUTOVER_MANIFEST_VERSION,
        preflight_fingerprint: report.fingerprint,
        acknowledged_global_configuration_sources: source_hashes(
            &report.global_configuration_sources,
        ),
        entries,
    };
    let _ = prepare_cutover(report, manifest.clone())?;
    Ok(manifest)
}

pub(crate) fn prepare_cutover(
    report: &CutoverPreflightReport,
    mut manifest: LegacyCutoverManifest,
) -> Result<(PreparedCutoverPlan, ResolvedCutoverManifest), CanonicalApparatusError> {
    validate_manifest_header(report, &manifest)?;
    manifest
        .entries
        .sort_by(|left, right| left.legacy_apparatus_id.cmp(&right.legacy_apparatus_id));
    let inventory = report
        .legacy_apparatuses
        .iter()
        .map(|item| (item.apparatus_id.as_str(), item))
        .collect::<BTreeMap<_, _>>();
    if inventory.len() != manifest.entries.len() {
        return blocked("manifest does not cover every legacy apparatus exactly once");
    }
    let mut seen = BTreeSet::new();
    let mut prepared = Vec::with_capacity(manifest.entries.len());
    let mut resolved = Vec::with_capacity(manifest.entries.len());
    for entry in manifest.entries {
        if !seen.insert(entry.legacy_apparatus_id.clone()) {
            return blocked("manifest contains a duplicate legacy apparatus identity");
        }
        let legacy = inventory
            .get(entry.legacy_apparatus_id.as_str())
            .ok_or_else(|| blocked_error("manifest references an unknown legacy apparatus"))?;
        validate_entry(report, legacy, &entry)?;
        let artifact = export_canonical_aasx(&entry.canonical_revision)
            .map_err(|_| CanonicalApparatusError::ArtifactIntegrity)?;
        if artifact.sha256() != entry.expected_aasx_sha256
            || parse_canonical_aasx(artifact.bytes())
                .map_err(|_| CanonicalApparatusError::ArtifactIntegrity)?
                != entry.canonical_revision
        {
            return Err(CanonicalApparatusError::ArtifactIntegrity);
        }
        let projections = project_apparatus_revision(&entry.canonical_revision, artifact.sha256());
        resolved.push(ResolvedCutoverEntry {
            legacy_apparatus_id: entry.legacy_apparatus_id.clone(),
            apparatus_id: entry.canonical_revision.apparatus_id.to_string(),
            canonical_revision: entry.canonical_revision.clone(),
            deterministic_aasx_sha256: artifact.sha256(),
            deterministic_aasx_size_bytes: u64::try_from(artifact.bytes().len())
                .map_err(|_| CanonicalApparatusError::ArtifactIntegrity)?,
        });
        prepared.push(PreparedCutoverEntry {
            legacy_apparatus_id: entry.legacy_apparatus_id,
            revision: entry.canonical_revision,
            artifact,
            projections,
        });
    }
    Ok((
        PreparedCutoverPlan {
            preflight_fingerprint: report.fingerprint,
            entries: prepared,
        },
        ResolvedCutoverManifest {
            manifest_version: CUTOVER_MANIFEST_VERSION,
            preflight_fingerprint: report.fingerprint,
            entries: resolved,
        },
    ))
}

fn validate_manifest_header(
    report: &CutoverPreflightReport,
    manifest: &LegacyCutoverManifest,
) -> Result<(), CanonicalApparatusError> {
    if manifest.manifest_version != CUTOVER_MANIFEST_VERSION
        || report.report_version != CUTOVER_MANIFEST_VERSION
        || manifest.preflight_fingerprint != report.fingerprint
        || !report.blocking_issues.is_empty()
    {
        return blocked("manifest does not match a clean current preflight");
    }
    let expected = source_hashes(&report.global_configuration_sources);
    if manifest.acknowledged_global_configuration_sources != expected {
        return blocked("global legacy configuration sources were not acknowledged exactly");
    }
    Ok(())
}

fn validate_entry(
    report: &CutoverPreflightReport,
    legacy: &LegacyApparatusInventory,
    entry: &super::model::LegacyCutoverManifestEntry,
) -> Result<(), CanonicalApparatusError> {
    let revision = &entry.canonical_revision;
    revision.validate()?;
    if revision.apparatus_id.as_str() != legacy.apparatus_id
        || entry.legacy_apparatus_id != legacy.apparatus_id
        || revision.revision_metadata.revision != 1
        || revision.revision_metadata.source != RevisionSource::LegacyMigration
        || revision.revision_metadata.source_reference.as_deref()
            != Some(&format!("preflight-sha256:{}", report.fingerprint))
    {
        return blocked("manifest attempts to replace stable identity or has invalid provenance");
    }
    let mut identities = entry.acknowledged_identities.clone();
    identities.sort();
    identities.dedup();
    if identities != legacy.observed_identities {
        return blocked("legacy identities were not acknowledged exactly");
    }
    if entry.acknowledged_configuration_sources != source_hashes(&legacy.configuration_sources) {
        return blocked("legacy configuration sources were not acknowledged exactly");
    }
    Ok(())
}

fn source_hashes(
    sources: &[super::model::CutoverConfigurationSource],
) -> BTreeMap<String, super::super::AasxSha256> {
    sources
        .iter()
        .map(|source| (source.source_key.clone(), source.sha256))
        .collect()
}

fn blocked<T>(message: &str) -> Result<T, CanonicalApparatusError> {
    Err(blocked_error(message))
}

fn blocked_error(message: &str) -> CanonicalApparatusError {
    CanonicalApparatusError::CutoverBlocked(message.to_string())
}
