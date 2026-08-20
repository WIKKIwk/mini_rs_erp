use super::*;
use crate::core::apparatus_standard::{
    aasx::{AAS_SPEC_PATH, package_from_aas_xml, validated_aas_spec},
    isa95::tests::revision_with,
};

fn fixture() -> CanonicalApparatusRevision {
    revision_with(
        "apparatus:test:aasx-01",
        "physical-asset:aasx-01",
        "AASX fixture",
    )
}

#[test]
fn revision_has_byte_identical_aasx_and_exact_hash() {
    let revision = fixture();
    let first = export_canonical_aasx(&revision).unwrap();
    let second = export_canonical_aasx(&revision).unwrap();

    assert_eq!(first, second);
    assert_eq!(first.sha256(), AasxSha256::digest(first.bytes()));
    assert_eq!(parse_canonical_aasx(first.bytes()).unwrap(), revision);
}

#[test]
fn uploaded_package_is_regenerated_instead_of_becoming_authority() {
    let revision = fixture();
    let artifact = export_canonical_aasx(&revision).unwrap();
    let mut uploaded = artifact.bytes().to_vec();
    let central_offset = u32::from_le_bytes(
        uploaded[uploaded.len() - 6..uploaded.len() - 2]
            .try_into()
            .unwrap(),
    ) as usize;
    uploaded[10] = 1;
    uploaded[central_offset + 12] = 1;
    assert_ne!(uploaded, artifact.bytes());

    let canonicalized = canonicalize_uploaded_aasx(&uploaded).unwrap();
    assert_eq!(canonicalized.revision, revision);
    assert_eq!(canonicalized.canonical_artifact, artifact);
}

#[test]
fn runtime_state_in_payload_is_rejected() {
    let artifact = export_canonical_aasx(&fixture()).unwrap();
    let specification = String::from_utf8(validated_aas_spec(artifact.bytes()).unwrap()).unwrap();
    let tampered = specification.replacen(
        "{&quot;schema_version&quot;:1,",
        "{&quot;queue_position&quot;:7,&quot;schema_version&quot;:1,",
        1,
    );
    assert_ne!(tampered, specification);
    let package = package_from_aas_xml(tampered.into_bytes()).unwrap();

    assert_eq!(
        parse_canonical_aasx(&package),
        Err(CanonicalAasxImportError::InvalidCanonicalPayload)
    );
}

#[test]
fn semantic_mirror_tampering_is_rejected() {
    let artifact = export_canonical_aasx(&fixture()).unwrap();
    let specification = String::from_utf8(validated_aas_spec(artifact.bytes()).unwrap()).unwrap();
    let tampered = specification.replacen(
        "<idShort>DisplayName</idShort>",
        "<idShort>DisplayTitle</idShort>",
        1,
    );
    let package = package_from_aas_xml(tampered.into_bytes()).unwrap();

    assert_eq!(
        parse_canonical_aasx(&package),
        Err(CanonicalAasxImportError::SemanticMismatch)
    );
}

#[test]
fn generic_package_guards_are_on_revision_import_path() {
    assert!(matches!(
        parse_canonical_aasx(b"not a zip"),
        Err(CanonicalAasxImportError::Package(_))
    ));
    assert_eq!(AAS_SPEC_PATH, "aasx/data.xml");
}
