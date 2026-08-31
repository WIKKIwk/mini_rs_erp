//! Canonical apparatus contract and its deterministic AASX representation.
//!
//! Runtime state is deliberately outside this module. Runtime consumers read
//! PostgreSQL projections produced from an immutable canonical revision.

pub mod cutover;
mod factory_defaults;
pub mod service;
#[cfg(test)]
pub(crate) mod test_support;

pub use cutover::{
    CutoverConfigurationSource, CutoverDiagnostic, CutoverPreflightReport, CutoverReferenceCount,
    CutoverTextReference, LegacyApparatusInventory, LegacyCutoverDraftEntry,
    LegacyCutoverDraftManifest, LegacyCutoverManifest, LegacyCutoverManifestEntry,
    ResolvedCutoverEntry, ResolvedCutoverManifest, build_cutover_manifest,
};
pub(crate) use factory_defaults::canonical_factory_apparatus_id_for_legacy;
pub use mini_rs_apparatus_contract::*;
pub use service::{
    CanonicalApparatusError, CanonicalApparatusPatch, CanonicalApparatusService,
    CanonicalCommandMetadata, CommittedCanonicalApparatus, StoredCanonicalAasx,
};
