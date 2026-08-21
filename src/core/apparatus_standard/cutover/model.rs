use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::super::{AasxSha256, CanonicalApparatusDraft, CanonicalApparatusRevision};

pub const CUTOVER_MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CutoverConfigurationSource {
    pub source_key: String,
    pub payload: serde_json::Value,
    pub sha256: AasxSha256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyApparatusInventory {
    pub apparatus_id: String,
    pub observed_identities: Vec<String>,
    pub configuration_sources: Vec<CutoverConfigurationSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CutoverReferenceCount {
    pub source_table: String,
    pub source_column: String,
    pub apparatus_id: String,
    pub row_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CutoverTextReference {
    pub source_table: String,
    pub source_column: String,
    pub observed_value: String,
    pub row_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CutoverDiagnostic {
    pub source: String,
    pub unresolved_rows: u64,
    pub orphan_rows: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CutoverPreflightReport {
    pub report_version: u32,
    pub required_migration_head: String,
    pub fingerprint: AasxSha256,
    pub legacy_apparatuses: Vec<LegacyApparatusInventory>,
    pub global_configuration_sources: Vec<CutoverConfigurationSource>,
    pub dependent_references: Vec<CutoverReferenceCount>,
    pub legacy_text_references: Vec<CutoverTextReference>,
    pub diagnostics: Vec<CutoverDiagnostic>,
    pub blocking_issues: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyCutoverManifestEntry {
    pub legacy_apparatus_id: String,
    pub acknowledged_identities: Vec<String>,
    pub acknowledged_configuration_sources: BTreeMap<String, AasxSha256>,
    pub canonical_revision: CanonicalApparatusRevision,
    pub expected_aasx_sha256: AasxSha256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyCutoverDraftEntry {
    pub legacy_apparatus_id: String,
    pub canonical_draft: CanonicalApparatusDraft,
    pub committed_at_unix_ms: i64,
    pub actor_id: String,
    pub command_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyCutoverDraftManifest {
    pub manifest_version: u32,
    pub preflight_fingerprint: AasxSha256,
    pub entries: Vec<LegacyCutoverDraftEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyCutoverManifest {
    pub manifest_version: u32,
    pub preflight_fingerprint: AasxSha256,
    pub acknowledged_global_configuration_sources: BTreeMap<String, AasxSha256>,
    pub entries: Vec<LegacyCutoverManifestEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedCutoverEntry {
    pub legacy_apparatus_id: String,
    pub apparatus_id: String,
    pub canonical_revision: CanonicalApparatusRevision,
    pub deterministic_aasx_sha256: AasxSha256,
    pub deterministic_aasx_size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedCutoverManifest {
    pub manifest_version: u32,
    pub preflight_fingerprint: AasxSha256,
    pub entries: Vec<ResolvedCutoverEntry>,
}
