use super::*;
use crate::core::apparatus_standard::{
    ApparatusId, CanonicalApparatusRevision, RevisionMetadata, RevisionSource,
    export_canonical_aasx,
    test_support::{TestApparatusSpec, canonical_draft},
};

fn revision(apparatus_id: &str) -> CanonicalApparatusRevision {
    let spec = TestApparatusSpec::cut(apparatus_id, "AASX package fixture");
    CanonicalApparatusRevision::from_draft(
        ApparatusId::new(apparatus_id).unwrap(),
        canonical_draft(&spec),
        RevisionMetadata {
            revision: 1,
            committed_at_unix_ms: 1_800_000_000_000,
            actor_id: "user:aasx-test".to_string(),
            command_id: "command:aasx-package-test".to_string(),
            source: RevisionSource::Admin,
            source_reference: None,
        },
    )
    .unwrap()
}

#[test]
fn canonical_package_is_byte_stable_and_round_trips_its_specification() {
    let revision = revision("apparatus:catalog:aasx-package-001");
    let first = export_canonical_aasx(&revision).unwrap();
    let second = export_canonical_aasx(&revision).unwrap();
    assert_eq!(first.bytes(), second.bytes());

    let specification = validated_aas_spec(first.bytes()).unwrap();
    assert!(specification.starts_with(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    assert!(
        specification
            .windows(revision.apparatus_id.as_str().len())
            .any(|window| { window == revision.apparatus_id.as_str().as_bytes() })
    );
}

#[test]
fn package_graph_is_exact_and_cannot_become_parallel_metadata_authority() {
    let package = package_from_aas_xml(b"<environment/>".to_vec()).unwrap();
    let mut parts = zip::read_zip_parts(&package).unwrap();
    assert_eq!(parts.len(), PACKAGE_PARTS.len());

    parts
        .get_mut(ROOT_RELATIONSHIPS_PATH)
        .unwrap()
        .extend_from_slice(b" ");
    let changed = zip::write_zip(
        &PACKAGE_PARTS
            .iter()
            .map(|path| ZipEntry::new(path, parts[*path].clone()))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    assert_eq!(
        validated_aas_spec(&changed),
        Err(AasxImportError::MalformedPackage(
            "canonical OPC package graph was modified"
        ))
    );
}

#[test]
fn missing_and_duplicate_parts_fail_closed() {
    let missing = zip::write_zip(&[
        ZipEntry::new(CONTENT_TYPES_PATH, opc::content_types_xml().into_bytes()),
        ZipEntry::new(
            ROOT_RELATIONSHIPS_PATH,
            opc::root_relationships_xml().into_bytes(),
        ),
    ])
    .unwrap();
    assert!(matches!(
        validated_aas_spec(&missing),
        Err(AasxImportError::MalformedPackage(_))
    ));

    let duplicate = zip::write_zip(&[
        ZipEntry::new(CONTENT_TYPES_PATH, Vec::new()),
        ZipEntry::new(CONTENT_TYPES_PATH, Vec::new()),
    ])
    .unwrap();
    assert_eq!(
        zip::read_zip_parts(&duplicate),
        Err(AasxImportError::MalformedPackage("duplicate ZIP entry"))
    );
}

#[test]
fn corrupt_crc_and_oversized_xml_are_rejected() {
    let mut package = package_from_aas_xml(b"<environment/>".to_vec()).unwrap();
    let payload_offset = 30 + CONTENT_TYPES_PATH.len();
    package[payload_offset] ^= 1;
    assert!(matches!(
        validated_aas_spec(&package),
        Err(AasxImportError::UnsupportedZip(_))
    ));

    assert_eq!(
        package_from_aas_xml(vec![0; MAX_AASX_PART_SIZE + 1]),
        Err(AasxExportError::XmlTooLarge)
    );
}
