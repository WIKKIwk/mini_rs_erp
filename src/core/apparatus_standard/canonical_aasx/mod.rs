//! Deterministic portable AASX representation of one canonical revision.

mod xml;

use thiserror::Error;

use super::aasx::{AasxExportError, AasxImportError, package_from_aas_xml, validated_aas_spec};
use super::{AasxSha256, CanonicalApparatusRevision, isa95::CanonicalApparatusValidationError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalAasxArtifact {
    bytes: Vec<u8>,
    sha256: AasxSha256,
}

impl CanonicalAasxArtifact {
    pub fn new(bytes: Vec<u8>) -> Self {
        let sha256 = AasxSha256::digest(&bytes);
        Self { bytes, sha256 }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub fn sha256(&self) -> AasxSha256 {
        self.sha256
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalizedAasxUpload {
    pub revision: CanonicalApparatusRevision,
    pub canonical_artifact: CanonicalAasxArtifact,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CanonicalAasxExportError {
    #[error("canonical apparatus revision is invalid: {0}")]
    InvalidRevision(#[from] CanonicalApparatusValidationError),
    #[error("canonical apparatus revision could not be serialized")]
    Serialization,
    #[error("canonical AAS XML contains a character forbidden by XML 1.0: U+{code:04X}")]
    InvalidXmlCharacter { code: u32 },
    #[error("canonical AASX package could not be built: {0}")]
    Package(#[from] AasxExportError),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CanonicalAasxImportError {
    #[error("canonical AASX package is invalid: {0}")]
    Package(#[from] AasxImportError),
    #[error("canonical AAS XML is not valid UTF-8")]
    InvalidUtf8,
    #[error("canonical revision payload is missing, duplicated, or malformed")]
    InvalidCanonicalPayload,
    #[error("canonical revision payload is invalid: {0}")]
    InvalidRevision(#[from] CanonicalApparatusValidationError),
    #[error("AAS semantic representation does not match the canonical revision payload")]
    SemanticMismatch,
}

/// Generate byte-stable project-canonical AASX and its exact SHA-256.
pub fn export_canonical_aasx(
    revision: &CanonicalApparatusRevision,
) -> Result<CanonicalAasxArtifact, CanonicalAasxExportError> {
    revision.validate()?;
    let payload =
        serde_json::to_string(revision).map_err(|_| CanonicalAasxExportError::Serialization)?;
    let specification = xml::canonical_aas_environment(revision, &payload)?;
    let bytes = package_from_aas_xml(specification.into_bytes())?;
    Ok(CanonicalAasxArtifact::new(bytes))
}

/// Parse and verify the canonical payload against its complete semantic AAS
/// representation. Runtime fields are rejected by the revision's strict serde
/// contract and never enter the candidate model.
pub fn parse_canonical_aasx(
    package: &[u8],
) -> Result<CanonicalApparatusRevision, CanonicalAasxImportError> {
    let specification = validated_aas_spec(package)?;
    let source =
        std::str::from_utf8(&specification).map_err(|_| CanonicalAasxImportError::InvalidUtf8)?;
    let payload = xml::extract_canonical_payload(source)?;
    let revision = serde_json::from_str::<CanonicalApparatusRevision>(&payload)
        .map_err(|_| CanonicalAasxImportError::InvalidCanonicalPayload)?;
    revision.validate()?;
    let canonical_payload = serde_json::to_string(&revision)
        .map_err(|_| CanonicalAasxImportError::InvalidCanonicalPayload)?;
    let expected = xml::canonical_aas_environment(&revision, &canonical_payload)
        .map_err(|_| CanonicalAasxImportError::SemanticMismatch)?;
    if source != expected {
        return Err(CanonicalAasxImportError::SemanticMismatch);
    }
    Ok(revision)
}

/// Treat uploaded bytes as untrusted transport, then return a validated model
/// and newly generated project-canonical bytes. The uploaded bytes themselves
/// are never promoted to authority.
pub fn canonicalize_uploaded_aasx(
    uploaded: &[u8],
) -> Result<CanonicalizedAasxUpload, CanonicalAasxImportError> {
    let revision = parse_canonical_aasx(uploaded)?;
    let canonical_artifact =
        export_canonical_aasx(&revision).map_err(|_| CanonicalAasxImportError::SemanticMismatch)?;
    Ok(CanonicalizedAasxUpload {
        revision,
        canonical_artifact,
    })
}

#[cfg(test)]
mod tests;
