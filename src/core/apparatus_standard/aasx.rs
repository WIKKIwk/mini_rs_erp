//! Bounded AASX engineering export for [`CanonicalApparatus`].
//!
//! The exporter maps only the canonical apparatus master/configuration fields
//! to an AAS 3.2 XML environment. It does not accept runtime state, and it
//! does not deserialize an arbitrary JSON value into the package. The package
//! uses OPC ZIP storage entries, which is sufficient for the bounded export;
//! import accepts stored or deflated parts and validates the package graph before
//! parsing the canonical XML contract.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

use flate2::{Decompress, FlushDecompress, Status};
use thiserror::Error;

use super::{
    AAS_APPARATUS_SUBMODEL_SEMANTIC_ID, AasPackageMetadata, ApparatusClassification,
    ApparatusDisplayMetadata, ApparatusFamily, ApparatusId, ApparatusKind, CanonicalApparatus,
    CapabilityCode, CapabilityProfile, CapacityConfiguration, CatalogSource, MaterialPolicy,
    MaterialRequirementGroup, OperationalPolicies, PlacementReference, Provenance, QueuePolicy,
    RawMaterialStartPolicy, ToolingPolicy, TrainingReference, Versioning, WorkingWindow,
};

pub const CONTENT_TYPES_PATH: &str = "[Content_Types].xml";
pub const ROOT_RELATIONSHIPS_PATH: &str = "_rels/.rels";
pub const AASX_ORIGIN_PATH: &str = "aasx/aasx-origin";
pub const AASX_ORIGIN_RELATIONSHIPS_PATH: &str = "aasx/_rels/aasx-origin.rels";
pub const AAS_SPEC_PATH: &str = "aasx/data.xml";

const AAS_XML_NAMESPACE: &str = "https://admin-shell.io/aas/3/0";
const OPC_CONTENT_TYPES_NAMESPACE: &str =
    "http://schemas.openxmlformats.org/package/2006/content-types";
const OPC_RELATIONSHIPS_NAMESPACE: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships";
const OPC_RELATIONSHIPS_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-package.relationships+xml";
const OPC_XML_CONTENT_TYPE: &str = "application/xml";
const AASX_ORIGIN_CONTENT_TYPE: &str = "application/asset-administration-shell-package+xml";
const AASX_ORIGIN_RELATIONSHIP: &str = "http://admin-shell.io/aasx/relationships/aasx-origin";
const AASX_SPEC_RELATIONSHIP: &str = "http://admin-shell.io/aasx/relationships/aas-spec";
const XML_SCHEMA_NAMESPACE: &str = "http://www.w3.org/2001/XMLSchema";
const MAX_AASX_PACKAGE_SIZE: usize = 16 * 1024 * 1024;
const MAX_AASX_PART_SIZE: u64 = 8 * 1024 * 1024;
const MAX_AASX_VALUE_BYTES: usize = 64 * 1024;
const MAX_AASX_TEXT_BYTES: usize = 512 * 1024;
const MAX_AASX_XML_MEMORY_BYTES: usize = 4 * 1024 * 1024;
const MAX_AASX_XML_DEPTH: usize = 64;
const MAX_AASX_XML_NODES: usize = 4_096;
const MAX_AASX_COLLECTION_DEPTH: usize = 32;
const MAX_AASX_COLLECTION_ITEMS: usize = 256;
const PACKAGE_PARTS: [&str; 5] = [
    CONTENT_TYPES_PATH,
    ROOT_RELATIONSHIPS_PATH,
    AASX_ORIGIN_PATH,
    AASX_ORIGIN_RELATIONSHIPS_PATH,
    AAS_SPEC_PATH,
];

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AasxExportError {
    #[error("canonical apparatus validation failed: {0}")]
    InvalidApparatus(#[from] super::ApparatusValidationError),
    #[error("AASX XML text contains a character forbidden by XML 1.0: U+{code:04X}")]
    InvalidXmlCharacter { code: u32 },
    #[error("AASX package is too large for the ZIP32 package format")]
    PackageTooLarge,
    #[error("AASX XML value exceeds the supported size")]
    ValueTooLarge,
    #[error("AASX collection exceeds the supported item count")]
    CollectionTooLarge,
    #[error("AASX XML part exceeds the supported size budget")]
    XmlTooLarge,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AasxImportError {
    #[error("AASX package is malformed: {0}")]
    MalformedPackage(&'static str),
    #[error("AASX package contains invalid XML: {0}")]
    InvalidXml(&'static str),
    #[error("AASX package contains an invalid XML character: U+{code:04X}")]
    InvalidXmlCharacter { code: u32 },
    #[error("AASX package contains unsupported ZIP data: {0}")]
    UnsupportedZip(&'static str),
    #[error("AASX payload does not match the canonical apparatus contract: {0}")]
    InvalidApparatus(#[from] super::ApparatusValidationError),
}

/// Import one bounded OPC/AASX package into the canonical apparatus contract.
///
/// Only the package graph and AAS environment emitted by [`export_aasx`] are
/// accepted. The importer never derives identity from display metadata and it
/// does not import runtime, order, queue, or scheduling state.
pub fn import_aasx(package: &[u8]) -> Result<CanonicalApparatus, AasxImportError> {
    let specification = validated_aas_spec(package)?;
    let apparatus = parse_aas_environment(&specification)?;
    apparatus.validate()?;
    Ok(apparatus)
}

/// Validate the bounded OPC graph and return the exact AAS specification part.
///
/// The canonical revision codec reuses this package boundary so both legacy
/// and revision packages have identical ZIP, traversal, relationship, content
/// type, duplicate-entry, decompression, and size protections.
pub(crate) fn validated_aas_spec(package: &[u8]) -> Result<Vec<u8>, AasxImportError> {
    let parts = read_zip_parts(package)?;
    let content_types = required_part(&parts, CONTENT_TYPES_PATH)?;
    validate_content_types(content_types)?;

    let root_relationships = parse_relationships(
        required_part(&parts, ROOT_RELATIONSHIPS_PATH)?,
        ROOT_RELATIONSHIPS_PATH,
    )?;
    if root_relationships.len() != 1 {
        return Err(AasxImportError::MalformedPackage(
            "root relationships must contain exactly one relationship",
        ));
    }
    let root_relationship = &root_relationships[0];
    if root_relationship.id != "aasxOrigin"
        || root_relationship.relationship_type != AASX_ORIGIN_RELATIONSHIP
    {
        return Err(AasxImportError::MalformedPackage(
            "root relationship graph does not identify the AASX origin",
        ));
    }
    let origin_path = resolve_relationship_target(None, &root_relationship.target)?;
    if origin_path != AASX_ORIGIN_PATH || !parts.contains_key(&origin_path) {
        return Err(AasxImportError::MalformedPackage(
            "root relationship target is missing",
        ));
    }

    if !parts[AASX_ORIGIN_PATH].is_empty() {
        return Err(AasxImportError::MalformedPackage(
            "AASX origin part must be empty",
        ));
    }

    let origin_relationships = parse_relationships(
        required_part(&parts, AASX_ORIGIN_RELATIONSHIPS_PATH)?,
        AASX_ORIGIN_RELATIONSHIPS_PATH,
    )?;
    if origin_relationships.len() != 1 {
        return Err(AasxImportError::MalformedPackage(
            "origin relationships must contain exactly one relationship",
        ));
    }
    let origin_relationship = &origin_relationships[0];
    if origin_relationship.id != "aasSpec"
        || origin_relationship.relationship_type != AASX_SPEC_RELATIONSHIP
    {
        return Err(AasxImportError::MalformedPackage(
            "origin relationship graph does not identify the AAS payload",
        ));
    }
    let specification_path =
        resolve_relationship_target(Some(AASX_ORIGIN_PATH), &origin_relationship.target)?;
    if specification_path != AAS_SPEC_PATH || !parts.contains_key(&specification_path) {
        return Err(AasxImportError::MalformedPackage(
            "origin relationship target is missing",
        ));
    }

    Ok(required_part(&parts, AAS_SPEC_PATH)?.to_vec())
}

fn read_zip_parts(package: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, AasxImportError> {
    if package.len() > MAX_AASX_PACKAGE_SIZE {
        return Err(AasxImportError::UnsupportedZip(
            "AASX package exceeds the supported size",
        ));
    }

    let eocd_offset = find_end_of_central_directory(package)?;
    let disk_number = zip_u16(package, eocd_offset + 4)?;
    let central_disk = zip_u16(package, eocd_offset + 6)?;
    let disk_entries = zip_u16(package, eocd_offset + 8)?;
    let total_entries = zip_u16(package, eocd_offset + 10)?;
    let central_size = usize::try_from(zip_u32(package, eocd_offset + 12)?)
        .map_err(|_| AasxImportError::UnsupportedZip("ZIP central directory is too large"))?;
    let central_offset = usize::try_from(zip_u32(package, eocd_offset + 16)?)
        .map_err(|_| AasxImportError::UnsupportedZip("ZIP central directory is too large"))?;
    if disk_number != 0
        || central_disk != 0
        || disk_entries != total_entries
        || usize::from(total_entries) > PACKAGE_PARTS.len()
    {
        return Err(AasxImportError::UnsupportedZip(
            "multi-disk or ZIP64 archives are not supported",
        ));
    }
    let central_end =
        central_offset
            .checked_add(central_size)
            .ok_or(AasxImportError::UnsupportedZip(
                "ZIP central directory range overflows",
            ))?;
    if central_end != eocd_offset || central_end > package.len() {
        return Err(AasxImportError::UnsupportedZip(
            "ZIP central directory range is invalid",
        ));
    }

    let mut parts = BTreeMap::new();
    let mut local_ranges = Vec::with_capacity(usize::from(total_entries));
    let mut central_cursor = central_offset;
    for _ in 0..usize::from(total_entries) {
        if zip_u32(package, central_cursor)? != 0x0201_4b50 {
            return Err(AasxImportError::UnsupportedZip(
                "invalid ZIP central directory entry",
            ));
        }
        let flags = zip_u16(package, central_cursor + 8)?;
        if flags & ((1 << 0) | (1 << 3) | (1 << 5) | (1 << 6) | (1 << 13)) != 0 {
            return Err(AasxImportError::UnsupportedZip(
                "encrypted or descriptor-based ZIP entries are not supported",
            ));
        }
        let compression = zip_u16(package, central_cursor + 10)?;
        if !matches!(compression, 0 | 8) {
            return Err(AasxImportError::UnsupportedZip(
                "ZIP compression method is not supported",
            ));
        }
        let crc = zip_u32(package, central_cursor + 16)?;
        let compressed_size = usize::try_from(zip_u32(package, central_cursor + 20)?)
            .map_err(|_| AasxImportError::UnsupportedZip("ZIP entry is too large"))?;
        let uncompressed_size = usize::try_from(zip_u32(package, central_cursor + 24)?)
            .map_err(|_| AasxImportError::UnsupportedZip("ZIP entry is too large"))?;
        if uncompressed_size > MAX_AASX_PART_SIZE as usize {
            return Err(AasxImportError::UnsupportedZip(
                "AASX part exceeds the supported size",
            ));
        }
        let name_length = usize::from(zip_u16(package, central_cursor + 28)?);
        let extra_length = usize::from(zip_u16(package, central_cursor + 30)?);
        let comment_length = usize::from(zip_u16(package, central_cursor + 32)?);
        let disk_start = zip_u16(package, central_cursor + 34)?;
        let central_end_for_entry = central_cursor
            .checked_add(46)
            .and_then(|value| value.checked_add(name_length))
            .and_then(|value| value.checked_add(extra_length))
            .and_then(|value| value.checked_add(comment_length))
            .ok_or(AasxImportError::UnsupportedZip(
                "ZIP central directory entry range overflows",
            ))?;
        if disk_start != 0 || central_end_for_entry > central_end {
            return Err(AasxImportError::UnsupportedZip(
                "ZIP central directory entry range is invalid",
            ));
        }
        let name_start = central_cursor + 46;
        let name_bytes = &package[name_start..name_start + name_length];
        let name = std::str::from_utf8(name_bytes)
            .map_err(|_| AasxImportError::UnsupportedZip("ZIP entry name is not UTF-8"))?
            .to_string();
        if name.ends_with('/') {
            return Err(AasxImportError::UnsupportedZip(
                "directory entries are not supported",
            ));
        }
        if !PACKAGE_PARTS.contains(&name.as_str()) {
            return Err(AasxImportError::UnsupportedZip(
                "unsupported AASX package structure",
            ));
        }
        if parts.contains_key(&name) {
            return Err(AasxImportError::MalformedPackage("duplicate ZIP entry"));
        }

        let local_offset = usize::try_from(zip_u32(package, central_cursor + 42)?)
            .map_err(|_| AasxImportError::UnsupportedZip("ZIP local entry offset is invalid"))?;
        if local_offset.checked_add(30).is_none() || local_offset >= central_offset {
            return Err(AasxImportError::UnsupportedZip(
                "ZIP local entry offset is invalid",
            ));
        }
        if zip_u32(package, local_offset)? != 0x0403_4b50 {
            return Err(AasxImportError::UnsupportedZip("invalid ZIP local entry"));
        }
        let local_flags = zip_u16(package, local_offset + 6)?;
        let local_compression = zip_u16(package, local_offset + 8)?;
        let local_crc = zip_u32(package, local_offset + 14)?;
        let local_compressed_size = usize::try_from(zip_u32(package, local_offset + 18)?)
            .map_err(|_| AasxImportError::UnsupportedZip("ZIP local entry is too large"))?;
        let local_uncompressed_size = usize::try_from(zip_u32(package, local_offset + 22)?)
            .map_err(|_| AasxImportError::UnsupportedZip("ZIP local entry is too large"))?;
        let local_name_length = usize::from(zip_u16(package, local_offset + 26)?);
        let local_extra_length = usize::from(zip_u16(package, local_offset + 28)?);
        let local_name_start = local_offset + 30;
        let local_data_start = local_name_start
            .checked_add(local_name_length)
            .and_then(|value| value.checked_add(local_extra_length))
            .ok_or(AasxImportError::UnsupportedZip(
                "ZIP local entry range overflows",
            ))?;
        if local_flags != flags
            || local_compression != compression
            || local_crc != crc
            || local_compressed_size != compressed_size
            || local_uncompressed_size != uncompressed_size
            || local_extra_length != extra_length
            || local_data_start > central_offset
            || local_name_length != name_length
            || package.get(local_name_start..local_name_start + local_name_length)
                != Some(name_bytes)
        {
            return Err(AasxImportError::UnsupportedZip(
                "ZIP local entry does not match its central directory record",
            ));
        }
        let local_data_end = local_data_start.checked_add(compressed_size).ok_or(
            AasxImportError::UnsupportedZip("ZIP local entry range overflows"),
        )?;
        if local_data_end > central_offset || local_data_end > package.len() {
            return Err(AasxImportError::UnsupportedZip(
                "ZIP local entry data range is invalid",
            ));
        }
        if local_ranges
            .iter()
            .any(|(start, end)| local_offset < *end && *start < local_data_end)
        {
            return Err(AasxImportError::UnsupportedZip(
                "ZIP local entries overlap",
            ));
        }
        local_ranges.push((local_offset, local_data_end));
        let compressed = &package[local_data_start..local_data_end];
        let contents = match compression {
            0 => compressed.to_vec(),
            8 => inflate_zip_entry(compressed, uncompressed_size)?,
            _ => unreachable!("compression method checked above"),
        };
        if contents.len() != uncompressed_size || crc32(&contents) != crc {
            return Err(AasxImportError::UnsupportedZip(
                "ZIP entry checksum or size does not match its contents",
            ));
        }
        parts.insert(name, contents);
        central_cursor = central_end_for_entry;
    }
    if central_cursor != central_end {
        return Err(AasxImportError::UnsupportedZip(
            "ZIP central directory contains trailing data",
        ));
    }
    Ok(parts)
}

fn find_end_of_central_directory(package: &[u8]) -> Result<usize, AasxImportError> {
    if package.len() < 22 {
        return Err(AasxImportError::UnsupportedZip("ZIP end record is missing"));
    }
    let start = package.len().saturating_sub(22 + usize::from(u16::MAX));
    for offset in (start..=package.len() - 22).rev() {
        if zip_u32(package, offset)? == 0x0605_4b50 {
            let comment_length = usize::from(zip_u16(package, offset + 20)?);
            if offset + 22 + comment_length == package.len() {
                return Ok(offset);
            }
        }
    }
    Err(AasxImportError::UnsupportedZip("ZIP end record is invalid"))
}

fn zip_u16(bytes: &[u8], offset: usize) -> Result<u16, AasxImportError> {
    let value = bytes
        .get(offset..offset + 2)
        .ok_or(AasxImportError::UnsupportedZip("truncated ZIP record"))?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn zip_u32(bytes: &[u8], offset: usize) -> Result<u32, AasxImportError> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or(AasxImportError::UnsupportedZip("truncated ZIP record"))?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn inflate_zip_entry(
    compressed: &[u8],
    expected_size: usize,
) -> Result<Vec<u8>, AasxImportError> {
    let mut decompressor = Decompress::new(false);
    let mut contents = Vec::with_capacity(expected_size);
    let mut input_offset = 0usize;
    let mut output_buffer = [0u8; 8 * 1024];

    loop {
        let input_before = decompressor.total_in();
        let output_before = decompressor.total_out();
        let status = decompressor
            .decompress(
                &compressed[input_offset..],
                &mut output_buffer,
                FlushDecompress::None,
            )
            .map_err(|_| AasxImportError::UnsupportedZip("could not deflate ZIP entry"))?;
        let input_consumed = usize::try_from(decompressor.total_in() - input_before)
            .map_err(|_| AasxImportError::UnsupportedZip("deflated ZIP entry is too large"))?;
        let output_produced = usize::try_from(decompressor.total_out() - output_before)
            .map_err(|_| AasxImportError::UnsupportedZip("deflated ZIP entry is too large"))?;
        input_offset = input_offset
            .checked_add(input_consumed)
            .ok_or(AasxImportError::UnsupportedZip(
                "deflated ZIP entry input range overflows",
            ))?;
        if input_offset > compressed.len() {
            return Err(AasxImportError::UnsupportedZip(
                "deflated ZIP entry input range is invalid",
            ));
        }
        if output_produced > 0 {
            let new_len = contents
                .len()
                .checked_add(output_produced)
                .ok_or(AasxImportError::UnsupportedZip(
                    "deflated ZIP entry is too large",
                ))?;
            if new_len > expected_size {
                return Err(AasxImportError::UnsupportedZip(
                    "deflated ZIP entry exceeds its declared size",
                ));
            }
            contents.extend_from_slice(&output_buffer[..output_produced]);
        }

        match status {
            Status::StreamEnd => break,
            Status::Ok if input_consumed > 0 || output_produced > 0 => {}
            _ => {
                return Err(AasxImportError::UnsupportedZip(
                    "deflated ZIP entry is truncated or cannot make progress",
                ));
            }
        }
    }

    if input_offset != compressed.len() {
        return Err(AasxImportError::UnsupportedZip(
            "deflated ZIP entry has trailing data",
        ));
    }
    if contents.len() != expected_size {
        return Err(AasxImportError::UnsupportedZip(
            "deflated ZIP entry size does not match its declaration",
        ));
    }
    Ok(contents)
}

fn required_part<'a>(
    parts: &'a BTreeMap<String, Vec<u8>>,
    path: &str,
) -> Result<&'a [u8], AasxImportError> {
    parts
        .get(path)
        .map(Vec::as_slice)
        .ok_or(AasxImportError::MalformedPackage(
            "required AASX part is missing",
        ))
}

fn validate_content_types(bytes: &[u8]) -> Result<(), AasxImportError> {
    let root = parse_xml_document(bytes)?;
    ensure_root(&root, "Types", OPC_CONTENT_TYPES_NAMESPACE, &["xmlns"])?;
    ensure_whitespace(&root)?;
    if root.children.iter().any(|child| {
        child.namespace != OPC_CONTENT_TYPES_NAMESPACE
            || !matches!(child.qname.as_str(), "Default" | "Override")
    }) {
        return Err(AasxImportError::InvalidXml(
            "content types document contains an unexpected element",
        ));
    }

    let mut defaults = BTreeMap::new();
    let mut overrides = BTreeMap::new();
    for child in &root.children {
        match child.qname.as_str() {
            "Default" => {
                ensure_attributes(child, &["Extension", "ContentType"])?;
                let extension = child.attributes["Extension"].clone();
                let content_type = child.attributes["ContentType"].clone();
                if defaults.insert(extension, content_type).is_some() {
                    return Err(AasxImportError::InvalidXml(
                        "duplicate content type default",
                    ));
                }
            }
            "Override" => {
                ensure_attributes(child, &["PartName", "ContentType"])?;
                let part_name = child.attributes["PartName"].clone();
                validate_part_name(&part_name)?;
                let content_type = child.attributes["ContentType"].clone();
                if overrides.insert(part_name, content_type).is_some() {
                    return Err(AasxImportError::InvalidXml(
                        "duplicate content type override",
                    ));
                }
            }
            _ => unreachable!("ensure_children checked content type element names"),
        }
        ensure_empty_element(child)?;
    }

    let expected_defaults = BTreeMap::from([
        (
            "rels".to_string(),
            OPC_RELATIONSHIPS_CONTENT_TYPE.to_string(),
        ),
        ("xml".to_string(), OPC_XML_CONTENT_TYPE.to_string()),
    ]);
    let expected_overrides = BTreeMap::from([
        (
            format!("/{AASX_ORIGIN_PATH}"),
            AASX_ORIGIN_CONTENT_TYPE.to_string(),
        ),
        (
            format!("/{AAS_SPEC_PATH}"),
            OPC_XML_CONTENT_TYPE.to_string(),
        ),
    ]);
    if defaults != expected_defaults || overrides != expected_overrides {
        return Err(AasxImportError::MalformedPackage(
            "[Content_Types].xml does not describe the supported AASX parts",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Relationship {
    id: String,
    relationship_type: String,
    target: String,
}

fn parse_relationships(
    bytes: &[u8],
    _part_path: &str,
) -> Result<Vec<Relationship>, AasxImportError> {
    let root = parse_xml_document(bytes)?;
    ensure_root(
        &root,
        "Relationships",
        OPC_RELATIONSHIPS_NAMESPACE,
        &["xmlns"],
    )?;
    ensure_whitespace(&root)?;

    let mut relationships = Vec::new();
    let mut ids = BTreeSet::new();
    for child in &root.children {
        if child.qname != "Relationship" || child.namespace != OPC_RELATIONSHIPS_NAMESPACE {
            return Err(AasxImportError::InvalidXml(
                "relationship document contains an unexpected element",
            ));
        }
        ensure_attributes(child, &["Id", "Type", "Target"])?;
        ensure_empty_element(child)?;
        let relationship = Relationship {
            id: child.attributes["Id"].clone(),
            relationship_type: child.attributes["Type"].clone(),
            target: child.attributes["Target"].clone(),
        };
        if relationship.id.is_empty() || !ids.insert(relationship.id.clone()) {
            return Err(AasxImportError::MalformedPackage(
                "relationship IDs must be present and unique",
            ));
        }
        relationships.push(relationship);
    }
    Ok(relationships)
}

fn resolve_relationship_target(
    source_part: Option<&str>,
    target: &str,
) -> Result<String, AasxImportError> {
    if target.is_empty()
        || target
            .chars()
            .any(|character| character.is_control() || matches!(character, '\\' | '%' | '?' | '#'))
        || target.contains(':')
    {
        return Err(AasxImportError::MalformedPackage(
            "relationship target is malformed",
        ));
    }

    if let Some(path) = target.strip_prefix('/') {
        if target.starts_with("//") {
            return Err(AasxImportError::MalformedPackage(
                "relationship target has an invalid absolute form",
            ));
        }
        if source_part.is_some() {
            return Err(AasxImportError::MalformedPackage(
                "relationship target must not be absolute",
            ));
        }
        validate_package_path(path)?;
        return Ok(path.to_string());
    }

    let source_part = source_part.ok_or(AasxImportError::MalformedPackage(
        "root relationship target must be package-absolute",
    ))?;
    let directory = source_part
        .rsplit_once('/')
        .map_or("", |(directory, _)| directory);
    validate_package_path(target)?;
    let resolved = if directory.is_empty() {
        target.to_string()
    } else {
        format!("{directory}/{target}")
    };
    validate_package_path(&resolved)?;
    Ok(resolved)
}

fn validate_package_path(path: &str) -> Result<(), AasxImportError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.ends_with('/')
        || path.contains('\\')
        || path.contains('%')
        || path
            .split('/')
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        return Err(AasxImportError::MalformedPackage(
            "relationship target contains an unsafe package path",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct XmlNode {
    qname: String,
    namespace: String,
    attributes: BTreeMap<String, String>,
    children: Vec<XmlNode>,
    text: String,
}

#[derive(Debug)]
struct OpenXmlNode {
    node: XmlNode,
    qname: String,
    namespaces: BTreeMap<String, String>,
}

#[derive(Default)]
struct XmlParseBudget {
    nodes: usize,
    text_bytes: usize,
    memory_bytes: usize,
}

impl XmlParseBudget {
    fn reserve_memory(&mut self, bytes: usize) -> Result<(), AasxImportError> {
        self.memory_bytes = self
            .memory_bytes
            .checked_add(bytes)
            .ok_or(AasxImportError::InvalidXml("XML memory budget overflow"))?;
        if self.memory_bytes > MAX_AASX_XML_MEMORY_BYTES {
            return Err(AasxImportError::InvalidXml(
                "XML aggregate memory budget exceeded",
            ));
        }
        Ok(())
    }

    fn reserve_node(&mut self, memory: usize) -> Result<(), AasxImportError> {
        self.nodes = self
            .nodes
            .checked_add(1)
            .ok_or(AasxImportError::InvalidXml("XML node budget overflow"))?;
        if self.nodes > MAX_AASX_XML_NODES {
            return Err(AasxImportError::InvalidXml(
                "XML node count budget exceeded",
            ));
        }
        self.reserve_memory(memory.saturating_add(64))
    }

    fn reserve_text(&mut self, bytes: usize) -> Result<(), AasxImportError> {
        self.text_bytes = self
            .text_bytes
            .checked_add(bytes)
            .ok_or(AasxImportError::InvalidXml("XML text budget overflow"))?;
        if self.text_bytes > MAX_AASX_TEXT_BYTES {
            return Err(AasxImportError::InvalidXml(
                "XML aggregate text budget exceeded",
            ));
        }
        self.reserve_memory(bytes)
    }
}

fn parse_xml_document(bytes: &[u8]) -> Result<XmlNode, AasxImportError> {
    if bytes.len() > MAX_AASX_PART_SIZE as usize {
        return Err(AasxImportError::InvalidXml(
            "XML part exceeds its size limit",
        ));
    }
    let source = std::str::from_utf8(bytes)
        .map_err(|_| AasxImportError::InvalidXml("XML must be valid UTF-8"))?;
    validate_xml_characters(source)?;

    let source_bytes = source.as_bytes();
    let mut budget = XmlParseBudget::default();
    budget.reserve_memory(source_bytes.len())?;
    let mut cursor = 0;
    let mut stack = Vec::new();
    let mut root = None;

    while cursor < source_bytes.len() {
        if source_bytes[cursor] != b'<' {
            let text_start = cursor;
            while cursor < source_bytes.len() && source_bytes[cursor] != b'<' {
                cursor += 1;
            }
            let value = decode_xml_text(&source_bytes[text_start..cursor], &mut budget)?;
            append_xml_text(&mut stack, &mut root, &value)?;
            continue;
        }

        if source_bytes[cursor..].starts_with(b"<!--") {
            let comment_end = find_xml_sequence(source_bytes, cursor + 4, b"-->")
                .ok_or(AasxImportError::InvalidXml("unterminated XML comment"))?;
            let comment = &source_bytes[cursor + 4..comment_end];
            if comment.windows(2).any(|window| window == b"--") || comment.last() == Some(&b'-') {
                return Err(AasxImportError::InvalidXml("malformed XML comment"));
            }
            cursor = comment_end + 3;
            continue;
        }

        if source_bytes[cursor..].starts_with(b"<?") {
            let instruction_end = find_xml_sequence(source_bytes, cursor + 2, b"?>").ok_or(
                AasxImportError::InvalidXml("unterminated XML processing instruction"),
            )?;
            let instruction = source_bytes[cursor + 2..instruction_end].trim_ascii();
            let name_end = parse_xml_name_end(instruction, 0)?;
            let target = &instruction[..name_end];
            if target.eq_ignore_ascii_case(b"xml")
                && (cursor != 0
                    || root.is_some()
                    || !stack.is_empty()
                    || instruction != b"xml version=\"1.0\" encoding=\"UTF-8\"")
            {
                return Err(AasxImportError::InvalidXml(
                    "malformed XML declaration",
                ));
            }
            cursor = instruction_end + 2;
            continue;
        }

        if source_bytes[cursor..].starts_with(b"<!") {
            return Err(AasxImportError::InvalidXml(
                "DOCTYPE and CDATA are not supported",
            ));
        }

        if source_bytes[cursor..].starts_with(b"</") {
            let name_start = cursor + 2;
            let name_end = parse_xml_name_end(source_bytes, name_start)?;
            let mut end = skip_xml_whitespace(source_bytes, name_end);
            if source_bytes.get(end) != Some(&b'>') {
                return Err(AasxImportError::InvalidXml("malformed XML closing tag"));
            }
            end += 1;
            let end_qname = qname_from_bytes(&source_bytes[name_start..name_end])?;
            let open = stack
                .pop()
                .ok_or(AasxImportError::InvalidXml("unexpected XML closing tag"))?;
            if open.qname != end_qname {
                return Err(AasxImportError::InvalidXml(
                    "XML closing tag does not match its opening tag",
                ));
            }
            attach_xml_node(&mut stack, &mut root, open.node)?;
            cursor = end;
            continue;
        }

        let name_start = cursor + 1;
        let name_end = parse_xml_name_end(source_bytes, name_start)?;
        let qname = qname_from_bytes(&source_bytes[name_start..name_end])?;
        let mut attributes = BTreeMap::new();
        let mut end = name_end;
        let empty = loop {
            let before_whitespace = end;
            end = skip_xml_whitespace(source_bytes, end);
            if source_bytes[end..].starts_with(b"/>") {
                end += 2;
                break true;
            }
            if source_bytes.get(end) == Some(&b'>') {
                end += 1;
                break false;
            }
            if end == before_whitespace {
                return Err(AasxImportError::InvalidXml(
                    "XML attributes must be separated by whitespace",
                ));
            }

            let attribute_start = end;
            let attribute_end = parse_xml_name_end(source_bytes, attribute_start)?;
            let attribute_name = qname_from_bytes(&source_bytes[attribute_start..attribute_end])?;
            end = skip_xml_whitespace(source_bytes, attribute_end);
            if source_bytes.get(end) != Some(&b'=') {
                return Err(AasxImportError::InvalidXml(
                    "XML attribute is missing its equals sign",
                ));
            }
            end = skip_xml_whitespace(source_bytes, end + 1);
            let quote = *source_bytes.get(end).ok_or(AasxImportError::InvalidXml(
                "XML attribute value is missing",
            ))?;
            if !matches!(quote, b'\'' | b'\"') {
                return Err(AasxImportError::InvalidXml(
                    "XML attribute value is not quoted",
                ));
            }
            let value_start = end + 1;
            end = value_start;
            while source_bytes.get(end).is_some_and(|byte| *byte != quote) {
                end += 1;
            }
            let value_end = end;
            if source_bytes.get(end) != Some(&quote) {
                return Err(AasxImportError::InvalidXml(
                    "unterminated XML attribute value",
                ));
            }
            if source_bytes[value_start..value_end].contains(&b'<') {
                return Err(AasxImportError::InvalidXml(
                    "XML attribute value contains an unescaped less-than character",
                ));
            }
            let value = decode_xml_text(&source_bytes[value_start..value_end], &mut budget)?;
            if attributes.insert(attribute_name, value).is_some() {
                return Err(AasxImportError::InvalidXml("duplicate XML attribute"));
            }
            end += 1;
        };

        if stack.len() + 1 > MAX_AASX_XML_DEPTH {
            return Err(AasxImportError::InvalidXml(
                "XML nesting depth budget exceeded",
            ));
        }
        let attribute_memory = attributes
            .iter()
            .map(|(name, value)| name.len().saturating_add(value.len()))
            .sum::<usize>();
        budget.reserve_node(qname.len().saturating_add(attribute_memory))?;
        let inherited = stack
            .last()
            .map_or_else(BTreeMap::new, |open: &OpenXmlNode| open.namespaces.clone());
        let open = build_xml_node(&qname, attributes, &inherited)?;
        if empty {
            attach_xml_node(&mut stack, &mut root, open.node)?;
        } else {
            stack.push(open);
        }
        cursor = end;
    }

    if !stack.is_empty() {
        return Err(AasxImportError::InvalidXml("unterminated XML element"));
    }
    root.ok_or(AasxImportError::InvalidXml(
        "XML document has no root element",
    ))
}

fn build_xml_node(
    qname: &str,
    attributes: BTreeMap<String, String>,
    inherited_namespaces: &BTreeMap<String, String>,
) -> Result<OpenXmlNode, AasxImportError> {
    let mut namespaces = inherited_namespaces.clone();
    for (name, value) in &attributes {
        if name == "xmlns" {
            namespaces.insert(String::new(), value.clone());
        } else if let Some(prefix) = name.strip_prefix("xmlns:")
            && (prefix.is_empty()
                || namespaces
                    .insert(prefix.to_string(), value.clone())
                    .is_some())
        {
            return Err(AasxImportError::InvalidXml(
                "duplicate XML namespace declaration",
            ));
        }
    }
    let (prefix, _) = split_qname(qname)?;
    if !prefix.is_empty() && prefix != "xml" && !namespaces.contains_key(prefix) {
        return Err(AasxImportError::InvalidXml(
            "XML element uses an undeclared namespace prefix",
        ));
    }
    for name in attributes.keys() {
        if name == "xmlns" || name.starts_with("xmlns:") {
            continue;
        }
        let (prefix, _) = split_qname(name)?;
        if !prefix.is_empty() && prefix != "xml" && !namespaces.contains_key(prefix) {
            return Err(AasxImportError::InvalidXml(
                "XML attribute uses an undeclared namespace prefix",
            ));
        }
    }
    let namespace = if prefix == "xml" {
        "http://www.w3.org/XML/1998/namespace".to_string()
    } else {
        namespaces.get(prefix).cloned().unwrap_or_default()
    };
    Ok(OpenXmlNode {
        node: XmlNode {
            qname: qname.to_string(),
            namespace,
            attributes,
            children: Vec::new(),
            text: String::new(),
        },
        qname: qname.to_string(),
        namespaces,
    })
}

fn find_xml_sequence(bytes: &[u8], start: usize, sequence: &[u8]) -> Option<usize> {
    bytes
        .get(start..)?
        .windows(sequence.len())
        .position(|window| window == sequence)
        .map(|offset| start + offset)
}

fn skip_xml_whitespace(bytes: &[u8], mut cursor: usize) -> usize {
    while bytes
        .get(cursor)
        .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
    {
        cursor += 1;
    }
    cursor
}

fn parse_xml_name_end(bytes: &[u8], start: usize) -> Result<usize, AasxImportError> {
    let Some(first) = bytes.get(start).copied() else {
        return Err(AasxImportError::InvalidXml("XML name is missing"));
    };
    if !is_xml_name_start(first) {
        return Err(AasxImportError::InvalidXml("malformed XML name"));
    }
    let mut end = start + 1;
    while bytes.get(end).is_some_and(|byte| is_xml_name_char(*byte)) {
        end += 1;
    }
    Ok(end)
}

fn is_xml_name_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || matches!(byte, b'_' | b':')
}

fn is_xml_name_char(byte: u8) -> bool {
    is_xml_name_start(byte) || byte.is_ascii_digit() || matches!(byte, b'-' | b'.')
}

fn decode_xml_text(bytes: &[u8], budget: &mut XmlParseBudget) -> Result<String, AasxImportError> {
    let mut decoded = String::new();
    let mut cursor = 0;
    while let Some(relative_ampersand) = bytes[cursor..].iter().position(|byte| *byte == b'&') {
        let ampersand = cursor + relative_ampersand;
        decoded.push_str(
            std::str::from_utf8(&bytes[cursor..ampersand])
                .map_err(|_| AasxImportError::InvalidXml("XML text is not valid UTF-8"))?,
        );
        let relative_semicolon = bytes[ampersand + 1..]
            .iter()
            .position(|byte| *byte == b';')
            .ok_or(AasxImportError::InvalidXml(
                "unterminated XML entity reference",
            ))?;
        let semicolon = ampersand + 1 + relative_semicolon;
        let entity = &bytes[ampersand + 1..semicolon];
        let character = match entity {
            b"amp" => '&',
            b"lt" => '<',
            b"gt" => '>',
            b"quot" => '"',
            b"apos" => '\'',
            _ => {
                let code = if let Some(hex) = entity.strip_prefix(b"#x") {
                    u32::from_str_radix(
                        std::str::from_utf8(hex).map_err(|_| {
                            AasxImportError::InvalidXml("XML entity is not valid UTF-8")
                        })?,
                        16,
                    )
                    .map_err(|_| AasxImportError::InvalidXml("invalid numeric XML entity"))?
                } else if let Some(decimal) = entity.strip_prefix(b"#") {
                    std::str::from_utf8(decimal)
                        .map_err(|_| AasxImportError::InvalidXml("XML entity is not valid UTF-8"))?
                        .parse::<u32>()
                        .map_err(|_| AasxImportError::InvalidXml("invalid numeric XML entity"))?
                } else {
                    return Err(AasxImportError::InvalidXml(
                        "unsupported XML entity reference",
                    ));
                };
                char::from_u32(code).ok_or(AasxImportError::InvalidXmlCharacter { code })?
            }
        };
        decoded.push(character);
        cursor = semicolon + 1;
    }
    decoded.push_str(
        std::str::from_utf8(&bytes[cursor..])
            .map_err(|_| AasxImportError::InvalidXml("XML text is not valid UTF-8"))?,
    );
    validate_xml_characters(&decoded)?;
    if decoded.len() > MAX_AASX_VALUE_BYTES {
        return Err(AasxImportError::InvalidXml(
            "XML text value exceeds its size budget",
        ));
    }
    budget.reserve_text(decoded.len())?;
    Ok(decoded)
}

fn attach_xml_node(
    stack: &mut [OpenXmlNode],
    root: &mut Option<XmlNode>,
    node: XmlNode,
) -> Result<(), AasxImportError> {
    if let Some(parent) = stack.last_mut() {
        parent.node.children.push(node);
    } else if root.replace(node).is_some() {
        return Err(AasxImportError::InvalidXml(
            "XML document contains multiple root elements",
        ));
    }
    Ok(())
}

fn append_xml_text(
    stack: &mut [OpenXmlNode],
    _root: &mut Option<XmlNode>,
    value: &str,
) -> Result<(), AasxImportError> {
    validate_xml_characters(value)?;
    if value.contains("]]>") {
        return Err(AasxImportError::InvalidXml(
            "XML character data contains the forbidden ]]> sequence",
        ));
    }
    if let Some(parent) = stack.last_mut() {
        parent.node.text.push_str(value);
    } else if !value.trim().is_empty() {
        return Err(AasxImportError::InvalidXml(
            "XML contains text outside its root element",
        ));
    }
    Ok(())
}

fn qname_from_bytes(bytes: &[u8]) -> Result<String, AasxImportError> {
    let qname = std::str::from_utf8(bytes)
        .map_err(|_| AasxImportError::InvalidXml("XML name is not valid UTF-8"))?;
    if qname.is_empty() {
        return Err(AasxImportError::InvalidXml("XML name is empty"));
    }
    Ok(qname.to_string())
}

fn split_qname(qname: &str) -> Result<(&str, &str), AasxImportError> {
    match qname.split_once(':') {
        None => Ok(("", qname)),
        Some((prefix, local))
            if !prefix.is_empty() && !local.is_empty() && !local.contains(':') =>
        {
            Ok((prefix, local))
        }
        _ => Err(AasxImportError::InvalidXml("malformed XML qualified name")),
    }
}

fn validate_xml_characters(value: &str) -> Result<(), AasxImportError> {
    if let Some(character) = value.chars().find(|&character| {
        !matches!(
            character,
            '\u{9}'
                | '\u{A}'
                | '\u{D}'
                | '\u{20}'..='\u{D7FF}'
                | '\u{E000}'..='\u{FFFD}'
                | '\u{10000}'..='\u{10FFFF}'
        )
    }) {
        return Err(AasxImportError::InvalidXmlCharacter {
            code: character as u32,
        });
    }
    Ok(())
}

fn ensure_root(
    node: &XmlNode,
    qname: &str,
    namespace: &str,
    attributes: &[&str],
) -> Result<(), AasxImportError> {
    if node.qname != qname || node.namespace != namespace {
        return Err(AasxImportError::InvalidXml("unexpected XML root element"));
    }
    ensure_attributes(node, attributes)?;
    if node.attributes.get("xmlns").map(String::as_str) != Some(namespace) {
        return Err(AasxImportError::InvalidXml(
            "XML root namespace declaration is invalid",
        ));
    }
    Ok(())
}

fn ensure_attributes(node: &XmlNode, expected: &[&str]) -> Result<(), AasxImportError> {
    if node.attributes.len() != expected.len()
        || expected
            .iter()
            .any(|name| !node.attributes.contains_key(*name))
    {
        return Err(AasxImportError::InvalidXml(
            "XML element attributes are invalid",
        ));
    }
    Ok(())
}

fn ensure_empty_element(node: &XmlNode) -> Result<(), AasxImportError> {
    ensure_whitespace(node)?;
    if !node.children.is_empty() {
        return Err(AasxImportError::InvalidXml(
            "XML element must not contain children",
        ));
    }
    Ok(())
}

fn ensure_whitespace(node: &XmlNode) -> Result<(), AasxImportError> {
    if !node.text.trim().is_empty() {
        return Err(AasxImportError::InvalidXml(
            "XML container contains unexpected text",
        ));
    }
    Ok(())
}

fn ensure_children(
    node: &XmlNode,
    namespace: &str,
    expected: &[&str],
) -> Result<(), AasxImportError> {
    ensure_whitespace(node)?;
    if node
        .children
        .iter()
        .any(|child| child.namespace != namespace || !expected.contains(&child.qname.as_str()))
        || expected.iter().any(|name| {
            node.children
                .iter()
                .filter(|child| child.qname == *name)
                .count()
                != 1
        })
    {
        return Err(AasxImportError::InvalidXml(
            "XML element children do not match the contract",
        ));
    }
    Ok(())
}

fn child<'a>(
    node: &'a XmlNode,
    qname: &str,
    namespace: &str,
) -> Result<&'a XmlNode, AasxImportError> {
    let matches = node
        .children
        .iter()
        .filter(|candidate| candidate.qname == qname && candidate.namespace == namespace)
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(AasxImportError::InvalidXml(
            "required XML element is missing or duplicated",
        ));
    }
    Ok(matches[0])
}

fn leaf_text(node: &XmlNode) -> Result<&str, AasxImportError> {
    ensure_attributes(node, &[])?;
    if !node.children.is_empty() {
        return Err(AasxImportError::InvalidXml(
            "XML leaf contains child elements",
        ));
    }
    Ok(&node.text)
}

fn validate_part_name(part_name: &str) -> Result<(), AasxImportError> {
    if !part_name.starts_with('/') {
        return Err(AasxImportError::MalformedPackage(
            "content type part name must be absolute",
        ));
    }
    validate_package_path(part_name.strip_prefix('/').unwrap_or_default())
}

#[derive(Debug, Clone)]
struct XmlProperty {
    id_short: String,
    value_type: String,
    value: String,
}

#[derive(Debug, Clone)]
struct XmlCollection {
    id_short: String,
    elements: Vec<XmlElement>,
}

#[derive(Debug, Clone)]
enum XmlElement {
    Property(XmlProperty),
    Collection(XmlCollection),
}

impl XmlElement {
    fn id_short(&self) -> &str {
        match self {
            Self::Property(property) => &property.id_short,
            Self::Collection(collection) => &collection.id_short,
        }
    }
}

impl XmlCollection {
    fn ensure_only(&self, expected: &[&str]) -> Result<(), AasxImportError> {
        if self
            .elements
            .iter()
            .any(|element| !expected.contains(&element.id_short()))
        {
            return Err(AasxImportError::InvalidXml(
                "AAS collection contains an unexpected element",
            ));
        }
        Ok(())
    }

    fn property(&self, id_short: &str, value_type: &str) -> Result<&str, AasxImportError> {
        match self
            .elements
            .iter()
            .find(|element| element.id_short() == id_short)
        {
            Some(XmlElement::Property(property)) if property.value_type == value_type => {
                Ok(&property.value)
            }
            Some(XmlElement::Property(_)) => Err(AasxImportError::InvalidXml(
                "AAS property value type is invalid",
            )),
            Some(XmlElement::Collection(_)) => Err(AasxImportError::InvalidXml(
                "AAS element must be a property",
            )),
            None => Err(AasxImportError::InvalidXml(
                "required AAS property is missing",
            )),
        }
    }

    fn optional_property(
        &self,
        id_short: &str,
        value_type: &str,
    ) -> Result<Option<&str>, AasxImportError> {
        match self
            .elements
            .iter()
            .find(|element| element.id_short() == id_short)
        {
            Some(XmlElement::Property(property)) if property.value_type == value_type => {
                Ok(Some(&property.value))
            }
            Some(XmlElement::Property(_)) => Err(AasxImportError::InvalidXml(
                "AAS property value type is invalid",
            )),
            Some(XmlElement::Collection(_)) => Err(AasxImportError::InvalidXml(
                "AAS element must be a property",
            )),
            None => Ok(None),
        }
    }

    fn collection(&self, id_short: &str) -> Result<&XmlCollection, AasxImportError> {
        match self
            .elements
            .iter()
            .find(|element| element.id_short() == id_short)
        {
            Some(XmlElement::Collection(collection)) => Ok(collection),
            Some(XmlElement::Property(_)) => Err(AasxImportError::InvalidXml(
                "AAS element must be a collection",
            )),
            None => Err(AasxImportError::InvalidXml(
                "required AAS collection is missing",
            )),
        }
    }
}

fn parse_property(node: &XmlNode) -> Result<XmlProperty, AasxImportError> {
    if node.qname != "property" || node.namespace != AAS_XML_NAMESPACE {
        return Err(AasxImportError::InvalidXml(
            "unexpected AAS submodel element",
        ));
    }
    ensure_attributes(node, &[])?;
    ensure_children(node, AAS_XML_NAMESPACE, &["idShort", "valueType", "value"])?;
    let id_short = leaf_text(child(node, "idShort", AAS_XML_NAMESPACE)?)?.to_string();
    let value_type = leaf_text(child(node, "valueType", AAS_XML_NAMESPACE)?)?.to_string();
    let value = leaf_text(child(node, "value", AAS_XML_NAMESPACE)?)?.to_string();
    for value in [&id_short, &value_type, &value] {
        if value.len() > MAX_AASX_VALUE_BYTES {
            return Err(AasxImportError::InvalidXml(
                "AAS property value exceeds its size budget",
            ));
        }
    }
    if id_short.is_empty() || value_type.is_empty() {
        return Err(AasxImportError::InvalidXml(
            "AAS property identity is empty",
        ));
    }
    Ok(XmlProperty {
        id_short,
        value_type,
        value,
    })
}

fn parse_collection(node: &XmlNode, depth: usize) -> Result<XmlCollection, AasxImportError> {
    if depth > MAX_AASX_COLLECTION_DEPTH {
        return Err(AasxImportError::InvalidXml(
            "AAS collection nesting depth budget exceeded",
        ));
    }
    if node.qname != "submodelElementCollection" || node.namespace != AAS_XML_NAMESPACE {
        return Err(AasxImportError::InvalidXml(
            "unexpected AAS submodel collection",
        ));
    }
    ensure_attributes(node, &[])?;
    ensure_children(node, AAS_XML_NAMESPACE, &["idShort", "value"])?;
    let id_short = leaf_text(child(node, "idShort", AAS_XML_NAMESPACE)?)?.to_string();
    if id_short.len() > MAX_AASX_VALUE_BYTES {
        return Err(AasxImportError::InvalidXml(
            "AAS collection identity exceeds its size budget",
        ));
    }
    if id_short.is_empty() {
        return Err(AasxImportError::InvalidXml(
            "AAS collection identity is empty",
        ));
    }
    let value = child(node, "value", AAS_XML_NAMESPACE)?;
    ensure_attributes(value, &[])?;
    ensure_whitespace(value)?;

    let mut elements = Vec::new();
    let mut ids = BTreeSet::new();
    if value.children.len() > MAX_AASX_COLLECTION_ITEMS {
        return Err(AasxImportError::InvalidXml(
            "AAS collection item budget exceeded",
        ));
    }
    for child in &value.children {
        let element = match child.qname.as_str() {
            "property" => XmlElement::Property(parse_property(child)?),
            "submodelElementCollection" => {
                XmlElement::Collection(parse_collection(child, depth + 1)?)
            }
            _ => {
                return Err(AasxImportError::InvalidXml(
                    "AAS collection contains an unexpected child",
                ));
            }
        };
        if !ids.insert(element.id_short().to_string()) {
            return Err(AasxImportError::InvalidXml(
                "duplicate AAS submodel element identity",
            ));
        }
        elements.push(element);
    }
    Ok(XmlCollection { id_short, elements })
}

fn parse_top_collections(node: &XmlNode) -> Result<Vec<XmlCollection>, AasxImportError> {
    ensure_attributes(node, &[])?;
    ensure_whitespace(node)?;
    let mut collections = Vec::new();
    let mut ids = BTreeSet::new();
    for child in &node.children {
        let collection = parse_collection(child, 1)?;
        if !ids.insert(collection.id_short.clone()) {
            return Err(AasxImportError::InvalidXml(
                "duplicate top-level AAS collection identity",
            ));
        }
        collections.push(collection);
    }
    Ok(collections)
}

fn parse_aas_environment(bytes: &[u8]) -> Result<CanonicalApparatus, AasxImportError> {
    let root = parse_xml_document(bytes)?;
    ensure_root(
        &root,
        "environment",
        AAS_XML_NAMESPACE,
        &["xmlns", "xmlns:xs"],
    )?;
    if root.attributes.get("xmlns:xs").map(String::as_str) != Some(XML_SCHEMA_NAMESPACE) {
        return Err(AasxImportError::InvalidXml(
            "AAS XML schema namespace declaration is invalid",
        ));
    }
    ensure_children(
        &root,
        AAS_XML_NAMESPACE,
        &["assetAdministrationShells", "submodels"],
    )?;

    let shells = child(&root, "assetAdministrationShells", AAS_XML_NAMESPACE)?;
    ensure_attributes(shells, &[])?;
    ensure_children(shells, AAS_XML_NAMESPACE, &["assetAdministrationShell"])?;
    let shell = child(shells, "assetAdministrationShell", AAS_XML_NAMESPACE)?;
    ensure_attributes(shell, &[])?;
    ensure_children(
        shell,
        AAS_XML_NAMESPACE,
        &["id", "idShort", "assetInformation", "submodels"],
    )?;
    let shell_id = leaf_text(child(shell, "id", AAS_XML_NAMESPACE)?)?.to_string();
    if leaf_text(child(shell, "idShort", AAS_XML_NAMESPACE)?)? != "Apparatus" {
        return Err(AasxImportError::InvalidXml("AAS shell idShort is invalid"));
    }

    let asset_information = child(shell, "assetInformation", AAS_XML_NAMESPACE)?;
    ensure_attributes(asset_information, &[])?;
    ensure_children(
        asset_information,
        AAS_XML_NAMESPACE,
        &["assetKind", "globalAssetId"],
    )?;
    if leaf_text(child(asset_information, "assetKind", AAS_XML_NAMESPACE)?)? != "Instance" {
        return Err(AasxImportError::InvalidXml("AAS asset kind is invalid"));
    }
    let global_asset_id = leaf_text(child(
        asset_information,
        "globalAssetId",
        AAS_XML_NAMESPACE,
    )?)?
    .to_string();

    let shell_submodels = child(shell, "submodels", AAS_XML_NAMESPACE)?;
    ensure_attributes(shell_submodels, &[])?;
    ensure_children(shell_submodels, AAS_XML_NAMESPACE, &["reference"])?;
    let shell_reference = parse_reference(
        child(shell_submodels, "reference", AAS_XML_NAMESPACE)?,
        "ModelReference",
        "Submodel",
    )?;

    let submodels = child(&root, "submodels", AAS_XML_NAMESPACE)?;
    ensure_attributes(submodels, &[])?;
    ensure_children(submodels, AAS_XML_NAMESPACE, &["submodel"])?;
    let submodel = child(submodels, "submodel", AAS_XML_NAMESPACE)?;
    ensure_attributes(submodel, &[])?;
    ensure_children(
        submodel,
        AAS_XML_NAMESPACE,
        &["id", "idShort", "kind", "semanticId", "submodelElements"],
    )?;
    let submodel_id = leaf_text(child(submodel, "id", AAS_XML_NAMESPACE)?)?.to_string();
    if leaf_text(child(submodel, "idShort", AAS_XML_NAMESPACE)?)? != "ApparatusConfiguration"
        || leaf_text(child(submodel, "kind", AAS_XML_NAMESPACE)?)? != "Instance"
    {
        return Err(AasxImportError::InvalidXml(
            "AAS submodel identity is invalid",
        ));
    }
    let semantic_id = parse_reference(
        child(submodel, "semanticId", AAS_XML_NAMESPACE)?,
        "",
        "GlobalReference",
    )?;
    if semantic_id != AAS_APPARATUS_SUBMODEL_SEMANTIC_ID {
        return Err(AasxImportError::InvalidXml(
            "AAS submodel semantic ID is invalid",
        ));
    }
    let top_collections =
        parse_top_collections(child(submodel, "submodelElements", AAS_XML_NAMESPACE)?)?;
    let required_top = [
        "Identity",
        "Classification",
        "Capabilities",
        "CapabilityProfiles",
        "Policies",
        "Capacity",
        "Training",
        "Provenance",
        "Versioning",
        "AasContract",
    ];
    if top_collections.iter().any(|collection| {
        collection.id_short != "Placement" && !required_top.contains(&collection.id_short.as_str())
    }) || required_top.iter().any(|name| {
        top_collections
            .iter()
            .filter(|collection| collection.id_short == *name)
            .count()
            != 1
    }) || top_collections.len() > required_top.len() + 1
    {
        return Err(AasxImportError::InvalidXml(
            "AAS top-level collections do not match the contract",
        ));
    }

    let identity_collection = find_collection(&top_collections, "Identity")?;
    identity_collection.ensure_only(&[
        "ApparatusId",
        "DisplayName",
        "Description",
        "CatalogOrder",
    ])?;
    let apparatus_id = ApparatusId::new(
        identity_collection
            .property("ApparatusId", "xs:string")?
            .to_string(),
    )?;
    let display = ApparatusDisplayMetadata {
        display_name: identity_collection
            .property("DisplayName", "xs:string")?
            .to_string(),
        description: identity_collection
            .property("Description", "xs:string")?
            .to_string(),
        catalog_order: parse_number(
            identity_collection.property("CatalogOrder", "xs:unsignedInt")?,
        )?,
    };

    let classification_collection = find_collection(&top_collections, "Classification")?;
    classification_collection.ensure_only(&["Family", "Kind", "ColorStations"])?;
    let classification = ApparatusClassification {
        family: parse_family(classification_collection.property("Family", "xs:string")?)?,
        kind: parse_kind(classification_collection.property("Kind", "xs:string")?)?,
        color_stations: classification_collection
            .optional_property("ColorStations", "xs:unsignedByte")?
            .map(parse_number)
            .transpose()?,
    };

    let capabilities = parse_capabilities(find_collection(&top_collections, "Capabilities")?)?;
    let capability_profiles =
        parse_capability_profiles(find_collection(&top_collections, "CapabilityProfiles")?)?;
    let policies = parse_policies(find_collection(&top_collections, "Policies")?)?;
    let capacity = parse_capacity(find_collection(&top_collections, "Capacity")?)?;
    let placement = match top_collections
        .iter()
        .find(|collection| collection.id_short == "Placement")
    {
        Some(collection) => {
            collection.ensure_only(&["FactoryMapObjectId"])?;
            Some(PlacementReference {
                factory_map_object_id: collection
                    .property("FactoryMapObjectId", "xs:string")?
                    .to_string(),
            })
        }
        None => None,
    };

    let training_collection = find_collection(&top_collections, "Training")?;
    training_collection.ensure_only(&["Enabled"])?;
    let training = TrainingReference {
        enabled: parse_bool(training_collection.property("Enabled", "xs:boolean")?)?,
    };

    let provenance_collection = find_collection(&top_collections, "Provenance")?;
    provenance_collection.ensure_only(&["Source", "SourceReference"])?;
    let provenance = Provenance {
        source: parse_catalog_source(provenance_collection.property("Source", "xs:string")?)?,
        source_ref: provenance_collection
            .optional_property("SourceReference", "xs:string")?
            .map(str::to_string),
    };

    let versioning_collection = find_collection(&top_collections, "Versioning")?;
    versioning_collection.ensure_only(&["Revision"])?;
    let versioning = Versioning {
        revision: parse_number(versioning_collection.property("Revision", "xs:unsignedLong")?)?,
    };

    let aas_collection = find_collection(&top_collections, "AasContract")?;
    aas_collection.ensure_only(&[
        "SemanticId",
        "IdtaRelease",
        "AasMetamodelVersion",
        "AasxPart5Version",
        "PackageFormat",
        "MediaType",
    ])?;
    let aas = AasPackageMetadata {
        submodel_id: submodel_id.clone(),
        semantic_id: aas_collection
            .property("SemanticId", "xs:string")?
            .to_string(),
        idta_release: aas_collection
            .property("IdtaRelease", "xs:string")?
            .to_string(),
        aas_metamodel_version: aas_collection
            .property("AasMetamodelVersion", "xs:string")?
            .to_string(),
        aasx_part_5_version: aas_collection
            .property("AasxPart5Version", "xs:string")?
            .to_string(),
        package_format: aas_collection
            .property("PackageFormat", "xs:string")?
            .to_string(),
        media_type: aas_collection
            .property("MediaType", "xs:string")?
            .to_string(),
    };

    if shell_id != apparatus_id.as_str()
        || global_asset_id != apparatus_id.as_str()
        || shell_reference != submodel_id
    {
        return Err(AasxImportError::InvalidXml(
            "AAS shell and submodel identities do not match the canonical payload",
        ));
    }

    Ok(CanonicalApparatus {
        identity: super::ApparatusIdentity {
            id: apparatus_id,
            display,
        },
        classification,
        capabilities,
        capability_profiles,
        policies,
        capacity,
        placement,
        training,
        provenance,
        versioning,
        aas,
    })
}

fn parse_reference(
    node: &XmlNode,
    reference_type: &str,
    key_type: &str,
) -> Result<String, AasxImportError> {
    if node.qname != "reference" && node.qname != "semanticId" {
        return Err(AasxImportError::InvalidXml("invalid AAS reference element"));
    }
    ensure_attributes(node, &[])?;
    if reference_type.is_empty() {
        ensure_children(node, AAS_XML_NAMESPACE, &["keys"])?;
    } else {
        ensure_children(node, AAS_XML_NAMESPACE, &["type", "keys"])?;
        if leaf_text(child(node, "type", AAS_XML_NAMESPACE)?)? != reference_type {
            return Err(AasxImportError::InvalidXml("AAS reference type is invalid"));
        }
    }
    let keys = child(node, "keys", AAS_XML_NAMESPACE)?;
    ensure_attributes(keys, &[])?;
    ensure_children(keys, AAS_XML_NAMESPACE, &["key"])?;
    let key = child(keys, "key", AAS_XML_NAMESPACE)?;
    ensure_attributes(key, &[])?;
    ensure_children(key, AAS_XML_NAMESPACE, &["type", "value"])?;
    if leaf_text(child(key, "type", AAS_XML_NAMESPACE)?)? != key_type {
        return Err(AasxImportError::InvalidXml(
            "AAS reference key type is invalid",
        ));
    }
    Ok(leaf_text(child(key, "value", AAS_XML_NAMESPACE)?)?.to_string())
}

fn find_collection<'a>(
    collections: &'a [XmlCollection],
    id_short: &str,
) -> Result<&'a XmlCollection, AasxImportError> {
    let matches = collections
        .iter()
        .filter(|collection| collection.id_short == id_short)
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(AasxImportError::InvalidXml(
            "required AAS collection is missing or duplicated",
        ));
    }
    Ok(matches[0])
}

fn parse_capabilities(collection: &XmlCollection) -> Result<Vec<CapabilityCode>, AasxImportError> {
    let mut capabilities = Vec::new();
    for (index, element) in collection.elements.iter().enumerate() {
        let nested = match element {
            XmlElement::Collection(collection) => collection,
            XmlElement::Property(_) => {
                return Err(AasxImportError::InvalidXml(
                    "capability entries must be collections",
                ));
            }
        };
        if nested.id_short != format!("Capability{}", index + 1) {
            return Err(AasxImportError::InvalidXml(
                "capability collection order is invalid",
            ));
        }
        nested.ensure_only(&["Code"])?;
        capabilities.push(parse_capability(nested.property("Code", "xs:string")?)?);
    }
    Ok(capabilities)
}

fn parse_capability_profiles(
    collection: &XmlCollection,
) -> Result<Vec<CapabilityProfile>, AasxImportError> {
    let mut profiles = Vec::new();
    for (index, element) in collection.elements.iter().enumerate() {
        let nested = match element {
            XmlElement::Collection(collection) => collection,
            XmlElement::Property(_) => {
                return Err(AasxImportError::InvalidXml(
                    "capability profile entries must be collections",
                ));
            }
        };
        if nested.id_short != format!("CapabilityProfile{}", index + 1) {
            return Err(AasxImportError::InvalidXml(
                "capability profile collection order is invalid",
            ));
        }
        nested.ensure_only(&["Code", "Level", "ValidFromUnix", "ValidToUnix", "Enabled"])?;
        profiles.push(CapabilityProfile {
            code: parse_capability(nested.property("Code", "xs:string")?)?,
            level: parse_number(nested.property("Level", "xs:unsignedShort")?)?,
            valid_from_unix: nested
                .optional_property("ValidFromUnix", "xs:long")?
                .map(parse_number)
                .transpose()?,
            valid_to_unix: nested
                .optional_property("ValidToUnix", "xs:long")?
                .map(parse_number)
                .transpose()?,
            enabled: parse_bool(nested.property("Enabled", "xs:boolean")?)?,
        });
    }
    Ok(profiles)
}

fn parse_policies(collection: &XmlCollection) -> Result<OperationalPolicies, AasxImportError> {
    collection.ensure_only(&["QueuePolicy", "ToolingPolicy", "MaterialPolicy"])?;
    let material_collection = collection.collection("MaterialPolicy")?;
    material_collection.ensure_only(&[
        "RequiresMaterial",
        "StartPolicy",
        "ItemGroups",
        "RequirementGroups",
    ])?;
    Ok(OperationalPolicies {
        queue: parse_queue_policy(collection.property("QueuePolicy", "xs:string")?)?,
        material: MaterialPolicy {
            requires_material: parse_bool(
                material_collection.property("RequiresMaterial", "xs:boolean")?,
            )?,
            start_policy: parse_raw_material_start_policy(
                material_collection.property("StartPolicy", "xs:string")?,
            )?,
            item_groups: parse_indexed_properties(
                material_collection.collection("ItemGroups")?,
                "ItemGroup",
                "xs:string",
            )?,
            requirement_groups: parse_requirement_groups(
                material_collection.collection("RequirementGroups")?,
            )?,
        },
        tooling: parse_tooling_policy(collection.property("ToolingPolicy", "xs:string")?)?,
    })
}

fn parse_requirement_groups(
    collection: &XmlCollection,
) -> Result<Vec<MaterialRequirementGroup>, AasxImportError> {
    let mut groups = Vec::new();
    for (index, element) in collection.elements.iter().enumerate() {
        let nested = match element {
            XmlElement::Collection(collection) => collection,
            XmlElement::Property(_) => {
                return Err(AasxImportError::InvalidXml(
                    "requirement group entries must be collections",
                ));
            }
        };
        if nested.id_short != format!("RequirementGroup{}", index + 1) {
            return Err(AasxImportError::InvalidXml(
                "requirement group collection order is invalid",
            ));
        }
        nested.ensure_only(&["Name", "MinimumRequiredCount", "ItemGroups"])?;
        groups.push(MaterialRequirementGroup {
            name: nested.property("Name", "xs:string")?.to_string(),
            item_groups: parse_indexed_properties(
                nested.collection("ItemGroups")?,
                "ItemGroup",
                "xs:string",
            )?,
            min_required_count: parse_number(
                nested.property("MinimumRequiredCount", "xs:unsignedShort")?,
            )?,
        });
    }
    Ok(groups)
}

fn parse_capacity(collection: &XmlCollection) -> Result<CapacityConfiguration, AasxImportError> {
    collection.ensure_only(&[
        "CapacitySlots",
        "SetupMinutes",
        "CleanupMinutes",
        "EfficiencyPercent",
        "FiniteCapacity",
        "WorkingWindows",
    ])?;
    Ok(CapacityConfiguration {
        capacity_slots: parse_number(collection.property("CapacitySlots", "xs:unsignedShort")?)?,
        setup_minutes: parse_number(collection.property("SetupMinutes", "xs:unsignedInt")?)?,
        cleanup_minutes: parse_number(collection.property("CleanupMinutes", "xs:unsignedInt")?)?,
        efficiency_percent: parse_number(
            collection.property("EfficiencyPercent", "xs:unsignedShort")?,
        )?,
        finite_capacity: parse_bool(collection.property("FiniteCapacity", "xs:boolean")?)?,
        working_windows: parse_working_windows(collection.collection("WorkingWindows")?)?,
    })
}

fn parse_working_windows(
    collection: &XmlCollection,
) -> Result<Vec<WorkingWindow>, AasxImportError> {
    let mut windows = Vec::new();
    for (index, element) in collection.elements.iter().enumerate() {
        let nested = match element {
            XmlElement::Collection(collection) => collection,
            XmlElement::Property(_) => {
                return Err(AasxImportError::InvalidXml(
                    "working window entries must be collections",
                ));
            }
        };
        if nested.id_short != format!("WorkingWindow{}", index + 1) {
            return Err(AasxImportError::InvalidXml(
                "working window collection order is invalid",
            ));
        }
        nested.ensure_only(&["Weekday", "StartMinute", "EndMinute"])?;
        windows.push(WorkingWindow {
            weekday: parse_number(nested.property("Weekday", "xs:unsignedByte")?)?,
            start_minute: parse_number(nested.property("StartMinute", "xs:unsignedShort")?)?,
            end_minute: parse_number(nested.property("EndMinute", "xs:unsignedShort")?)?,
        });
    }
    Ok(windows)
}

fn parse_indexed_properties(
    collection: &XmlCollection,
    prefix: &str,
    value_type: &str,
) -> Result<Vec<String>, AasxImportError> {
    let mut values = Vec::new();
    for (index, element) in collection.elements.iter().enumerate() {
        let property = match element {
            XmlElement::Property(property) => property,
            XmlElement::Collection(_) => {
                return Err(AasxImportError::InvalidXml(
                    "indexed AAS values must be properties",
                ));
            }
        };
        if property.id_short != format!("{prefix}{}", index + 1)
            || property.value_type != value_type
        {
            return Err(AasxImportError::InvalidXml(
                "indexed AAS property order is invalid",
            ));
        }
        if property.value.trim().is_empty() {
            return Err(AasxImportError::InvalidXml(
                "indexed AAS property value is empty",
            ));
        }
        values.push(property.value.clone());
    }
    Ok(values)
}

fn parse_number<T>(value: &str) -> Result<T, AasxImportError>
where
    T: std::str::FromStr,
{
    value
        .parse()
        .map_err(|_| AasxImportError::InvalidXml("AAS numeric property is invalid"))
}

fn parse_bool(value: &str) -> Result<bool, AasxImportError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(AasxImportError::InvalidXml(
            "AAS boolean property is invalid",
        )),
    }
}

fn parse_family(value: &str) -> Result<ApparatusFamily, AasxImportError> {
    match value {
        "pechat" => Ok(ApparatusFamily::Pechat),
        "laminatsiya" => Ok(ApparatusFamily::Laminatsiya),
        "rezka" => Ok(ApparatusFamily::Rezka),
        "paket" => Ok(ApparatusFamily::Paket),
        "kley" => Ok(ApparatusFamily::Kley),
        "other" => Ok(ApparatusFamily::Other),
        _ => Err(AasxImportError::InvalidXml("AAS family value is invalid")),
    }
}

fn parse_kind(value: &str) -> Result<ApparatusKind, AasxImportError> {
    match value {
        "color_pechat" => Ok(ApparatusKind::ColorPechat),
        "flexo" => Ok(ApparatusKind::Flexo),
        "laminatsiya" => Ok(ApparatusKind::Laminatsiya),
        "extruder_laminatsiya" => Ok(ApparatusKind::ExtruderLaminatsiya),
        "rezka" => Ok(ApparatusKind::Rezka),
        "paket" => Ok(ApparatusKind::Paket),
        "holodniy_kley" => Ok(ApparatusKind::HolodniyKley),
        "other" => Ok(ApparatusKind::Other),
        _ => Err(AasxImportError::InvalidXml("AAS kind value is invalid")),
    }
}

fn parse_capability(value: &str) -> Result<CapabilityCode, AasxImportError> {
    match value {
        "print" => Ok(CapabilityCode::Print),
        "pechat" => Ok(CapabilityCode::Pechat),
        "flexo" => Ok(CapabilityCode::Flexo),
        "laminate" => Ok(CapabilityCode::Laminate),
        "cut" => Ok(CapabilityCode::Cut),
        "package" => Ok(CapabilityCode::Package),
        "glue" => Ok(CapabilityCode::Glue),
        "apparatus" => Ok(CapabilityCode::Apparatus),
        _ => Err(AasxImportError::InvalidXml(
            "AAS capability value is invalid",
        )),
    }
}

fn parse_queue_policy(value: &str) -> Result<QueuePolicy, AasxImportError> {
    match value {
        "strict_sequence" => Ok(QueuePolicy::StrictSequence),
        "free_pick" => Ok(QueuePolicy::FreePick),
        _ => Err(AasxImportError::InvalidXml(
            "AAS queue policy value is invalid",
        )),
    }
}

fn parse_raw_material_start_policy(value: &str) -> Result<RawMaterialStartPolicy, AasxImportError> {
    match value {
        "state_all" => Ok(RawMaterialStartPolicy::StateAll),
        "requirement_groups" => Ok(RawMaterialStartPolicy::RequirementGroups),
        _ => Err(AasxImportError::InvalidXml(
            "AAS material start policy value is invalid",
        )),
    }
}

fn parse_tooling_policy(value: &str) -> Result<ToolingPolicy, AasxImportError> {
    match value {
        "qolip_scan_not_required" => Ok(ToolingPolicy::QolipScanNotRequired),
        "qolip_scan_required" => Ok(ToolingPolicy::QolipScanRequired),
        _ => Err(AasxImportError::InvalidXml(
            "AAS tooling policy value is invalid",
        )),
    }
}

fn parse_catalog_source(value: &str) -> Result<CatalogSource, AasxImportError> {
    match value {
        "default" => Ok(CatalogSource::Default),
        "custom" => Ok(CatalogSource::Custom),
        _ => Err(AasxImportError::InvalidXml(
            "AAS catalog source value is invalid",
        )),
    }
}

/// Export one validated canonical apparatus as an AASX byte package.
///
/// The returned bytes are an OPC ZIP package containing an AAS XML
/// environment. The AAS and submodel identifiers reuse the canonical
/// apparatus ID and the canonical submodel metadata respectively.
pub fn export_aasx(apparatus: &CanonicalApparatus) -> Result<Vec<u8>, AasxExportError> {
    apparatus.validate()?;
    validate_xml_text_fields(apparatus)?;

    let aas_xml = aas_environment_xml(apparatus).into_bytes();
    if aas_xml.len() > MAX_AASX_PART_SIZE as usize || aas_xml.len() > MAX_AASX_XML_MEMORY_BYTES {
        return Err(AasxExportError::XmlTooLarge);
    }

    package_from_aas_xml(aas_xml)
}

/// Build deterministic project AASX bytes around an already validated AAS XML
/// part. ZIP entry order, timestamps, flags, attributes, and compression are
/// fixed by [`write_zip`].
pub(crate) fn package_from_aas_xml(aas_xml: Vec<u8>) -> Result<Vec<u8>, AasxExportError> {
    if aas_xml.len() > MAX_AASX_PART_SIZE as usize || aas_xml.len() > MAX_AASX_XML_MEMORY_BYTES {
        return Err(AasxExportError::XmlTooLarge);
    }
    let entries = vec![
        ZipEntry {
            name: CONTENT_TYPES_PATH,
            contents: content_types_xml().into_bytes(),
        },
        ZipEntry {
            name: ROOT_RELATIONSHIPS_PATH,
            contents: root_relationships_xml().into_bytes(),
        },
        ZipEntry {
            name: AASX_ORIGIN_PATH,
            contents: Vec::new(),
        },
        ZipEntry {
            name: AASX_ORIGIN_RELATIONSHIPS_PATH,
            contents: origin_relationships_xml().into_bytes(),
        },
        ZipEntry {
            name: AAS_SPEC_PATH,
            contents: aas_xml,
        },
    ];

    write_zip(&entries)
}

fn validate_xml_text_fields(apparatus: &CanonicalApparatus) -> Result<(), AasxExportError> {
    if apparatus.capabilities.len() > MAX_AASX_COLLECTION_ITEMS
        || apparatus.capability_profiles.len() > MAX_AASX_COLLECTION_ITEMS
        || apparatus.policies.material.item_groups.len() > MAX_AASX_COLLECTION_ITEMS
        || apparatus.policies.material.requirement_groups.len() > MAX_AASX_COLLECTION_ITEMS
        || apparatus.capacity.working_windows.len() > MAX_AASX_COLLECTION_ITEMS
    {
        return Err(AasxExportError::CollectionTooLarge);
    }
    if apparatus
        .policies
        .material
        .requirement_groups
        .iter()
        .any(|group| group.item_groups.len() > MAX_AASX_COLLECTION_ITEMS)
    {
        return Err(AasxExportError::CollectionTooLarge);
    }

    let fields = [
        apparatus.identity.id.as_str(),
        apparatus.identity.display.display_name.as_str(),
        apparatus.identity.display.description.as_str(),
        apparatus.aas.submodel_id.as_str(),
        apparatus.aas.semantic_id.as_str(),
        apparatus.aas.idta_release.as_str(),
        apparatus.aas.aas_metamodel_version.as_str(),
        apparatus.aas.aasx_part_5_version.as_str(),
        apparatus.aas.package_format.as_str(),
        apparatus.aas.media_type.as_str(),
    ];
    let mut total_text_bytes = 0usize;
    for group in &apparatus.policies.material.requirement_groups {
        validate_export_text(group.name.as_str(), &mut total_text_bytes)?;
        for item_group in &group.item_groups {
            validate_export_text(item_group, &mut total_text_bytes)?;
        }
    }
    for field in fields {
        validate_export_text(field, &mut total_text_bytes)?;
    }
    for item_group in &apparatus.policies.material.item_groups {
        validate_export_text(item_group, &mut total_text_bytes)?;
    }
    if let Some(placement) = &apparatus.placement {
        validate_export_text(&placement.factory_map_object_id, &mut total_text_bytes)?;
    }
    if let Some(source_ref) = &apparatus.provenance.source_ref {
        validate_export_text(source_ref, &mut total_text_bytes)?;
    }
    Ok(())
}

fn validate_export_text(value: &str, total: &mut usize) -> Result<(), AasxExportError> {
    if value.len() > MAX_AASX_VALUE_BYTES {
        return Err(AasxExportError::ValueTooLarge);
    }
    *total = total
        .checked_add(value.len())
        .ok_or(AasxExportError::XmlTooLarge)?;
    if *total > MAX_AASX_TEXT_BYTES {
        return Err(AasxExportError::XmlTooLarge);
    }
    if let Some(character) = value.chars().find(|&character| {
        !matches!(
            character,
            '\u{9}'
                | '\u{A}'
                | '\u{D}'
                | '\u{20}'..='\u{D7FF}'
                | '\u{E000}'..='\u{FFFD}'
                | '\u{10000}'..='\u{10FFFF}'
        )
    }) {
        return Err(AasxExportError::InvalidXmlCharacter {
            code: character as u32,
        });
    }
    Ok(())
}

fn content_types_xml() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Types xmlns=\"{OPC_CONTENT_TYPES_NAMESPACE}\">\n  <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\n  <Default Extension=\"xml\" ContentType=\"application/xml\"/>\n  <Override PartName=\"/{AASX_ORIGIN_PATH}\" ContentType=\"application/asset-administration-shell-package+xml\"/>\n  <Override PartName=\"/{AAS_SPEC_PATH}\" ContentType=\"application/xml\"/>\n</Types>\n"
    )
}

fn root_relationships_xml() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Relationships xmlns=\"{OPC_RELATIONSHIPS_NAMESPACE}\">\n  <Relationship Id=\"aasxOrigin\" Type=\"{AASX_ORIGIN_RELATIONSHIP}\" Target=\"/{AASX_ORIGIN_PATH}\"/>\n</Relationships>\n"
    )
}

fn origin_relationships_xml() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Relationships xmlns=\"{OPC_RELATIONSHIPS_NAMESPACE}\">\n  <Relationship Id=\"aasSpec\" Type=\"{AASX_SPEC_RELATIONSHIP}\" Target=\"data.xml\"/>\n</Relationships>\n"
    )
}

fn aas_environment_xml(apparatus: &CanonicalApparatus) -> String {
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<environment xmlns=\"{AAS_XML_NAMESPACE}\" xmlns:xs=\"http://www.w3.org/2001/XMLSchema\">\n  <assetAdministrationShells>\n    <assetAdministrationShell>\n      <id>{}</id>\n      <idShort>Apparatus</idShort>\n      <assetInformation>\n        <assetKind>Instance</assetKind>\n        <globalAssetId>{}</globalAssetId>\n      </assetInformation>\n      <submodels>\n        <reference>\n          <type>ModelReference</type>\n          <keys>\n            <key>\n              <type>Submodel</type>\n              <value>{}</value>\n            </key>\n          </keys>\n        </reference>\n      </submodels>\n    </assetAdministrationShell>\n  </assetAdministrationShells>\n  <submodels>\n    <submodel>\n      <id>{}</id>\n      <idShort>ApparatusConfiguration</idShort>\n      <kind>Instance</kind>\n      <semanticId>\n        <keys>\n          <key>\n            <type>GlobalReference</type>\n            <value>{}</value>\n          </key>\n        </keys>\n      </semanticId>\n      <submodelElements>\n",
        xml_escape(apparatus.identity.id.as_str()),
        xml_escape(apparatus.identity.id.as_str()),
        xml_escape(&apparatus.aas.submodel_id),
        xml_escape(&apparatus.aas.submodel_id),
        xml_escape(AAS_APPARATUS_SUBMODEL_SEMANTIC_ID),
    );

    push_identity(&mut xml, apparatus);
    push_classification(&mut xml, &apparatus.classification);
    push_capabilities(&mut xml, apparatus);
    push_policies(&mut xml, &apparatus.policies.material, apparatus);
    push_capacity(&mut xml, apparatus);
    push_placement_training(&mut xml, apparatus);
    push_provenance_versioning(&mut xml, apparatus);
    push_aas_metadata(&mut xml, apparatus);

    xml.push_str("      </submodelElements>\n    </submodel>\n  </submodels>\n</environment>\n");
    xml
}

fn push_identity(xml: &mut String, apparatus: &CanonicalApparatus) {
    collection_start(xml, "Identity");
    property(
        xml,
        "ApparatusId",
        "xs:string",
        apparatus.identity.id.as_str(),
    );
    property(
        xml,
        "DisplayName",
        "xs:string",
        &apparatus.identity.display.display_name,
    );
    property(
        xml,
        "Description",
        "xs:string",
        &apparatus.identity.display.description,
    );
    property(
        xml,
        "CatalogOrder",
        "xs:unsignedInt",
        &apparatus.identity.display.catalog_order.to_string(),
    );
    collection_end(xml);
}

fn push_classification(xml: &mut String, classification: &ApparatusClassification) {
    collection_start(xml, "Classification");
    property(
        xml,
        "Family",
        "xs:string",
        apparatus_family_name(classification.family),
    );
    property(
        xml,
        "Kind",
        "xs:string",
        apparatus_kind_name(classification.kind),
    );
    if let Some(stations) = classification.color_stations {
        property(
            xml,
            "ColorStations",
            "xs:unsignedByte",
            &stations.to_string(),
        );
    }
    collection_end(xml);
}

fn push_capabilities(xml: &mut String, apparatus: &CanonicalApparatus) {
    collection_start(xml, "Capabilities");
    for (index, code) in apparatus.capabilities.iter().enumerate() {
        collection_start(xml, &format!("Capability{}", index + 1));
        property(xml, "Code", "xs:string", capability_name(*code));
        collection_end(xml);
    }
    collection_end(xml);

    collection_start(xml, "CapabilityProfiles");
    for (index, profile) in apparatus.capability_profiles.iter().enumerate() {
        push_capability_profile(xml, index + 1, profile);
    }
    collection_end(xml);
}

fn push_capability_profile(xml: &mut String, index: usize, profile: &CapabilityProfile) {
    collection_start(xml, &format!("CapabilityProfile{}", index));
    property(xml, "Code", "xs:string", capability_name(profile.code));
    property(xml, "Level", "xs:unsignedShort", &profile.level.to_string());
    if let Some(value) = profile.valid_from_unix {
        property(xml, "ValidFromUnix", "xs:long", &value.to_string());
    }
    if let Some(value) = profile.valid_to_unix {
        property(xml, "ValidToUnix", "xs:long", &value.to_string());
    }
    property(xml, "Enabled", "xs:boolean", bool_name(profile.enabled));
    collection_end(xml);
}

fn push_policies(xml: &mut String, material: &MaterialPolicy, apparatus: &CanonicalApparatus) {
    collection_start(xml, "Policies");
    property(
        xml,
        "QueuePolicy",
        "xs:string",
        queue_policy_name(apparatus.policies.queue),
    );
    property(
        xml,
        "ToolingPolicy",
        "xs:string",
        tooling_policy_name(apparatus.policies.tooling),
    );

    collection_start(xml, "MaterialPolicy");
    property(
        xml,
        "RequiresMaterial",
        "xs:boolean",
        bool_name(material.requires_material),
    );
    property(
        xml,
        "StartPolicy",
        "xs:string",
        raw_material_start_policy_name(material.start_policy),
    );
    collection_start(xml, "ItemGroups");
    for (index, item_group) in material.item_groups.iter().enumerate() {
        property(
            xml,
            &format!("ItemGroup{}", index + 1),
            "xs:string",
            item_group,
        );
    }
    collection_end(xml);
    collection_start(xml, "RequirementGroups");
    for (index, group) in material.requirement_groups.iter().enumerate() {
        collection_start(xml, &format!("RequirementGroup{}", index + 1));
        property(xml, "Name", "xs:string", &group.name);
        property(
            xml,
            "MinimumRequiredCount",
            "xs:unsignedShort",
            &group.min_required_count.to_string(),
        );
        collection_start(xml, "ItemGroups");
        for (item_index, item_group) in group.item_groups.iter().enumerate() {
            property(
                xml,
                &format!("ItemGroup{}", item_index + 1),
                "xs:string",
                item_group,
            );
        }
        collection_end(xml);
        collection_end(xml);
    }
    collection_end(xml);
    collection_end(xml);
    collection_end(xml);
}

fn push_capacity(xml: &mut String, apparatus: &CanonicalApparatus) {
    collection_start(xml, "Capacity");
    property(
        xml,
        "CapacitySlots",
        "xs:unsignedShort",
        &apparatus.capacity.capacity_slots.to_string(),
    );
    property(
        xml,
        "SetupMinutes",
        "xs:unsignedInt",
        &apparatus.capacity.setup_minutes.to_string(),
    );
    property(
        xml,
        "CleanupMinutes",
        "xs:unsignedInt",
        &apparatus.capacity.cleanup_minutes.to_string(),
    );
    property(
        xml,
        "EfficiencyPercent",
        "xs:unsignedShort",
        &apparatus.capacity.efficiency_percent.to_string(),
    );
    property(
        xml,
        "FiniteCapacity",
        "xs:boolean",
        bool_name(apparatus.capacity.finite_capacity),
    );
    collection_start(xml, "WorkingWindows");
    for (index, window) in apparatus.capacity.working_windows.iter().enumerate() {
        collection_start(xml, &format!("WorkingWindow{}", index + 1));
        property(
            xml,
            "Weekday",
            "xs:unsignedByte",
            &window.weekday.to_string(),
        );
        property(
            xml,
            "StartMinute",
            "xs:unsignedShort",
            &window.start_minute.to_string(),
        );
        property(
            xml,
            "EndMinute",
            "xs:unsignedShort",
            &window.end_minute.to_string(),
        );
        collection_end(xml);
    }
    collection_end(xml);
    collection_end(xml);
}

fn push_placement_training(xml: &mut String, apparatus: &CanonicalApparatus) {
    if let Some(placement) = &apparatus.placement {
        collection_start(xml, "Placement");
        property(
            xml,
            "FactoryMapObjectId",
            "xs:string",
            &placement.factory_map_object_id,
        );
        collection_end(xml);
    }
    collection_start(xml, "Training");
    property(
        xml,
        "Enabled",
        "xs:boolean",
        bool_name(apparatus.training.enabled),
    );
    collection_end(xml);
}

fn push_provenance_versioning(xml: &mut String, apparatus: &CanonicalApparatus) {
    collection_start(xml, "Provenance");
    property(
        xml,
        "Source",
        "xs:string",
        catalog_source_name(apparatus.provenance.source),
    );
    if let Some(source_ref) = &apparatus.provenance.source_ref {
        property(xml, "SourceReference", "xs:string", source_ref);
    }
    collection_end(xml);

    collection_start(xml, "Versioning");
    property(
        xml,
        "Revision",
        "xs:unsignedLong",
        &apparatus.versioning.revision.to_string(),
    );
    collection_end(xml);
}

fn push_aas_metadata(xml: &mut String, apparatus: &CanonicalApparatus) {
    collection_start(xml, "AasContract");
    property(xml, "SemanticId", "xs:string", &apparatus.aas.semantic_id);
    property(xml, "IdtaRelease", "xs:string", &apparatus.aas.idta_release);
    property(
        xml,
        "AasMetamodelVersion",
        "xs:string",
        &apparatus.aas.aas_metamodel_version,
    );
    property(
        xml,
        "AasxPart5Version",
        "xs:string",
        &apparatus.aas.aasx_part_5_version,
    );
    property(
        xml,
        "PackageFormat",
        "xs:string",
        &apparatus.aas.package_format,
    );
    property(xml, "MediaType", "xs:string", &apparatus.aas.media_type);
    collection_end(xml);
}

fn collection_start(xml: &mut String, id_short: &str) {
    let _ = writeln!(
        xml,
        "        <submodelElementCollection>\n          <idShort>{}</idShort>\n          <value>",
        xml_escape(id_short)
    );
}

fn collection_end(xml: &mut String) {
    xml.push_str("          </value>\n        </submodelElementCollection>\n");
}

fn property(xml: &mut String, id_short: &str, value_type: &str, value: &str) {
    let _ = writeln!(
        xml,
        "            <property>\n              <idShort>{}</idShort>\n              <valueType>{}</valueType>\n              <value>{}</value>\n            </property>",
        xml_escape(id_short),
        value_type,
        xml_escape(value)
    );
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn apparatus_family_name(value: ApparatusFamily) -> &'static str {
    match value {
        ApparatusFamily::Pechat => "pechat",
        ApparatusFamily::Laminatsiya => "laminatsiya",
        ApparatusFamily::Rezka => "rezka",
        ApparatusFamily::Paket => "paket",
        ApparatusFamily::Kley => "kley",
        ApparatusFamily::Other => "other",
    }
}

fn apparatus_kind_name(value: ApparatusKind) -> &'static str {
    match value {
        ApparatusKind::ColorPechat => "color_pechat",
        ApparatusKind::Flexo => "flexo",
        ApparatusKind::Laminatsiya => "laminatsiya",
        ApparatusKind::ExtruderLaminatsiya => "extruder_laminatsiya",
        ApparatusKind::Rezka => "rezka",
        ApparatusKind::Paket => "paket",
        ApparatusKind::HolodniyKley => "holodniy_kley",
        ApparatusKind::Other => "other",
    }
}

fn capability_name(value: CapabilityCode) -> &'static str {
    match value {
        CapabilityCode::Print => "print",
        CapabilityCode::Pechat => "pechat",
        CapabilityCode::Flexo => "flexo",
        CapabilityCode::Laminate => "laminate",
        CapabilityCode::Cut => "cut",
        CapabilityCode::Package => "package",
        CapabilityCode::Glue => "glue",
        CapabilityCode::Apparatus => "apparatus",
    }
}

fn queue_policy_name(value: super::QueuePolicy) -> &'static str {
    match value {
        super::QueuePolicy::StrictSequence => "strict_sequence",
        super::QueuePolicy::FreePick => "free_pick",
    }
}

fn raw_material_start_policy_name(value: RawMaterialStartPolicy) -> &'static str {
    match value {
        RawMaterialStartPolicy::StateAll => "state_all",
        RawMaterialStartPolicy::RequirementGroups => "requirement_groups",
    }
}

fn tooling_policy_name(value: ToolingPolicy) -> &'static str {
    match value {
        ToolingPolicy::QolipScanNotRequired => "qolip_scan_not_required",
        ToolingPolicy::QolipScanRequired => "qolip_scan_required",
    }
}

fn catalog_source_name(value: super::CatalogSource) -> &'static str {
    match value {
        super::CatalogSource::Default => "default",
        super::CatalogSource::Custom => "custom",
    }
}

fn bool_name(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

struct ZipEntry {
    name: &'static str,
    contents: Vec<u8>,
}

fn write_zip(entries: &[ZipEntry]) -> Result<Vec<u8>, AasxExportError> {
    if entries.len() > u16::MAX as usize {
        return Err(AasxExportError::PackageTooLarge);
    }

    let mut package = Vec::new();
    let mut central_directory = Vec::new();
    for entry in entries {
        if entry.contents.len() > MAX_AASX_PART_SIZE as usize {
            return Err(AasxExportError::PackageTooLarge);
        }
        let name = entry.name.as_bytes();
        let size =
            u32::try_from(entry.contents.len()).map_err(|_| AasxExportError::PackageTooLarge)?;
        let offset = u32::try_from(package.len()).map_err(|_| AasxExportError::PackageTooLarge)?;
        let name_length =
            u16::try_from(name.len()).map_err(|_| AasxExportError::PackageTooLarge)?;
        let crc = crc32(&entry.contents);

        push_u32(&mut package, 0x0403_4b50);
        push_u16(&mut package, 20);
        push_u16(&mut package, 0);
        push_u16(&mut package, 0);
        push_u16(&mut package, 0);
        push_u16(&mut package, 0);
        push_u32(&mut package, crc);
        push_u32(&mut package, size);
        push_u32(&mut package, size);
        push_u16(&mut package, name_length);
        push_u16(&mut package, 0);
        package.extend_from_slice(name);
        package.extend_from_slice(&entry.contents);

        push_u32(&mut central_directory, 0x0201_4b50);
        push_u16(&mut central_directory, 20);
        push_u16(&mut central_directory, 20);
        push_u16(&mut central_directory, 0);
        push_u16(&mut central_directory, 0);
        push_u16(&mut central_directory, 0);
        push_u16(&mut central_directory, 0);
        push_u32(&mut central_directory, crc);
        push_u32(&mut central_directory, size);
        push_u32(&mut central_directory, size);
        push_u16(&mut central_directory, name_length);
        push_u16(&mut central_directory, 0);
        push_u16(&mut central_directory, 0);
        push_u16(&mut central_directory, 0);
        push_u16(&mut central_directory, 0);
        push_u32(&mut central_directory, 0);
        push_u32(&mut central_directory, offset);
        central_directory.extend_from_slice(name);
    }

    let central_offset =
        u32::try_from(package.len()).map_err(|_| AasxExportError::PackageTooLarge)?;
    let central_size =
        u32::try_from(central_directory.len()).map_err(|_| AasxExportError::PackageTooLarge)?;
    package.extend_from_slice(&central_directory);
    push_u32(&mut package, 0x0605_4b50);
    push_u16(&mut package, 0);
    push_u16(&mut package, 0);
    push_u16(&mut package, entries.len() as u16);
    push_u16(&mut package, entries.len() as u16);
    push_u32(&mut package, central_size);
    push_u32(&mut package, central_offset);
    push_u16(&mut package, 0);
    if package.len() > MAX_AASX_PACKAGE_SIZE {
        return Err(AasxExportError::PackageTooLarge);
    }
    Ok(package)
}

fn push_u16(target: &mut Vec<u8>, value: u16) {
    target.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(target: &mut Vec<u8>, value: u32) {
    target.extend_from_slice(&value.to_le_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;

    fn valid_apparatus() -> CanonicalApparatus {
        serde_json::from_value(json!({
            "identity": {
                "id": "apparatus:catalog:stable-001",
                "display": {
                    "display_name": "7 ta rangli bosma aparat",
                    "description": "Static engineering catalog record",
                    "catalog_order": 1
                }
            },
            "classification": {
                "family": "pechat",
                "kind": "color_pechat",
                "color_stations": 7
            },
            "capabilities": ["print", "pechat"],
            "policies": {"queue": "strict_sequence"},
            "capacity": {
                "capacity_slots": 1,
                "setup_minutes": 10,
                "cleanup_minutes": 5,
                "efficiency_percent": 100,
                "finite_capacity": true,
                "working_windows": [{"weekday": 1, "start_minute": 480, "end_minute": 1020}]
            },
            "training": {"enabled": true},
            "provenance": {"source": "default"},
            "versioning": {"revision": 3},
            "aas": {
                "submodel_id": "urn:mini-rs-erp:submodel:apparatus:catalog:stable-001"
            }
        }))
        .unwrap()
    }

    fn stored_entries(package: &[u8]) -> BTreeMap<String, Vec<u8>> {
        let mut entries = BTreeMap::new();
        let mut cursor = 0;
        while cursor + 4 <= package.len() {
            let signature = u32::from_le_bytes(package[cursor..cursor + 4].try_into().unwrap());
            if signature == 0x0201_4b50 || signature == 0x0605_4b50 {
                break;
            }
            assert_eq!(signature, 0x0403_4b50);
            let name_length =
                u16::from_le_bytes(package[cursor + 26..cursor + 28].try_into().unwrap()) as usize;
            let extra_length =
                u16::from_le_bytes(package[cursor + 28..cursor + 30].try_into().unwrap()) as usize;
            let compressed_size =
                u32::from_le_bytes(package[cursor + 18..cursor + 22].try_into().unwrap()) as usize;
            let compression =
                u16::from_le_bytes(package[cursor + 8..cursor + 10].try_into().unwrap());
            assert_eq!(compression, 0);
            let name_start = cursor + 30;
            let data_start = name_start + name_length + extra_length;
            let name =
                String::from_utf8(package[name_start..data_start - extra_length].to_vec()).unwrap();
            let data_end = data_start + compressed_size;
            entries.insert(name, package[data_start..data_end].to_vec());
            cursor = data_end;
        }
        entries
    }

    #[test]
    fn exports_valid_opc_entries_and_aas_environment() {
        let package = export_aasx(&valid_apparatus()).unwrap();
        let entries = stored_entries(&package);

        assert_eq!(entries.len(), 5);
        assert!(entries.contains_key(CONTENT_TYPES_PATH));
        assert!(entries.contains_key(ROOT_RELATIONSHIPS_PATH));
        assert!(entries.contains_key(AASX_ORIGIN_PATH));
        assert!(entries.contains_key(AASX_ORIGIN_RELATIONSHIPS_PATH));
        assert!(entries.contains_key(AAS_SPEC_PATH));

        let content_types = String::from_utf8(entries[CONTENT_TYPES_PATH].clone()).unwrap();
        assert!(content_types.contains("application/asset-administration-shell-package+xml"));
        let relationships =
            String::from_utf8(entries[AASX_ORIGIN_RELATIONSHIPS_PATH].clone()).unwrap();
        assert!(relationships.contains(AASX_SPEC_RELATIONSHIP));

        let spec = String::from_utf8(entries[AAS_SPEC_PATH].clone()).unwrap();
        assert!(spec.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(spec.contains("<id>apparatus:catalog:stable-001</id>"));
        assert!(spec.contains(
            "<id>urn:mini-rs-erp:submodel:apparatus:catalog:stable-001</id>"
        ));
        assert!(spec.contains("<idShort>CapacitySlots</idShort>"));
        assert!(spec.contains("<value>7</value>"));
        assert!(spec.contains("<idShort>WorkingWindow1</idShort>"));

        for excluded in [
            "queuePosition",
            "currentWorker",
            "liveWip",
            "assignedOrderBarcode",
            "pauseState",
            "freezeState",
        ] {
            assert!(
                !spec.contains(excluded),
                "unexpected runtime field: {excluded}"
            );
        }
    }

    #[test]
    fn export_revalidates_canonical_input() {
        let mut apparatus = valid_apparatus();
        apparatus.identity.id =
            super::super::ApparatusId::new("apparatus:catalog:stable_001").unwrap();
        apparatus.identity.display.display_name = "stable 001".to_string();
        assert!(matches!(
            export_aasx(&apparatus),
            Err(AasxExportError::InvalidApparatus(
                super::super::ApparatusValidationError::TitleDerivedId
            ))
        ));
    }

    #[test]
    fn export_rejects_forbidden_xml_character_in_material_item_group() {
        let mut apparatus = valid_apparatus();
        apparatus.policies.material.requires_material = true;
        apparatus.policies.material.item_groups = vec!["paper\u{1}".to_string()];

        assert!(matches!(
            export_aasx(&apparatus),
            Err(AasxExportError::InvalidXmlCharacter { code: 0x1 })
        ));
    }

    #[test]
    fn export_rejects_forbidden_xml_character_in_requirement_group_name() {
        let mut apparatus = valid_apparatus();
        apparatus.policies.material.requires_material = true;
        apparatus.policies.material.start_policy = RawMaterialStartPolicy::RequirementGroups;
        apparatus.policies.material.requirement_groups =
            vec![super::super::MaterialRequirementGroup {
                name: "substrate\u{B}".to_string(),
                item_groups: vec!["paper".to_string()],
                min_required_count: 1,
            }];

        assert!(matches!(
            export_aasx(&apparatus),
            Err(AasxExportError::InvalidXmlCharacter { code: 0xB })
        ));
    }

    #[test]
    fn exporter_enforces_material_collection_and_value_budgets() {
        let mut too_many_groups = valid_apparatus();
        too_many_groups.policies.material.requires_material = true;
        too_many_groups.policies.material.item_groups = (0..=MAX_AASX_COLLECTION_ITEMS)
            .map(|index| format!("group-{index}"))
            .collect();
        assert!(matches!(
            export_aasx(&too_many_groups),
            Err(AasxExportError::CollectionTooLarge)
        ));

        let mut too_large_value = valid_apparatus();
        too_large_value.policies.material.requires_material = true;
        too_large_value.policies.material.item_groups = vec!["x".repeat(MAX_AASX_VALUE_BYTES + 1)];
        assert!(matches!(
            export_aasx(&too_large_value),
            Err(AasxExportError::ValueTooLarge)
        ));
    }

    #[test]
    fn importer_enforces_xml_depth_node_and_aggregate_text_budgets() {
        let mut deep = String::new();
        for index in 0..=MAX_AASX_XML_DEPTH {
            deep.push_str(&format!("<n{index}>"));
        }
        for index in (0..=MAX_AASX_XML_DEPTH).rev() {
            deep.push_str(&format!("</n{index}>"));
        }
        assert!(matches!(
            parse_xml_document(deep.as_bytes()),
            Err(AasxImportError::InvalidXml(_))
        ));

        let many_nodes = format!("<root>{}</root>", "<node/>".repeat(MAX_AASX_XML_NODES + 1));
        assert!(matches!(
            parse_xml_document(many_nodes.as_bytes()),
            Err(AasxImportError::InvalidXml(_))
        ));

        let large_text = format!("<root>{}</root>", "x".repeat(MAX_AASX_TEXT_BYTES + 1));
        assert!(matches!(
            parse_xml_document(large_text.as_bytes()),
            Err(AasxImportError::InvalidXml(_))
        ));
    }

    #[test]
    fn importer_rejects_malformed_xml_attributes_and_declarations() {
        for malformed in [
            r#"<root value="one"other="two"/>"#,
            r#"<root value="one<two"/>"#,
            r#"<?xml?><root/>"#,
            r#"<root>text]]></root>"#,
        ] {
            assert!(matches!(
                parse_xml_document(malformed.as_bytes()),
                Err(AasxImportError::InvalidXml(_))
            ));
        }
    }

    #[test]
    fn deflated_zip_entries_are_bounded_and_exact() {
        use flate2::{Compress, Compression, FlushCompress};

        let mut compressor = Compress::new(Compression::default(), false);
        let mut compressed = Vec::with_capacity(64);
        compressor
            .compress_vec(b"canonical", &mut compressed, FlushCompress::Finish)
            .expect("deflate canonical test payload");

        assert_eq!(
            inflate_zip_entry(&compressed, b"canonical".len()).expect("inflate payload"),
            b"canonical"
        );
        assert!(inflate_zip_entry(&compressed, b"canonica".len()).is_err());

        let mut trailing = compressed;
        trailing.push(0);
        assert!(inflate_zip_entry(&trailing, b"canonical".len()).is_err());
    }

    #[test]
    fn rejects_overlapping_zip_local_entries() {
        let nested = write_zip(&[ZipEntry {
            name: ROOT_RELATIONSHIPS_PATH,
            contents: b"root".to_vec(),
        }])
        .expect("nested ZIP entry");
        let nested_central_offset =
            u32::from_le_bytes(nested[nested.len() - 6..nested.len() - 2].try_into().unwrap())
                as usize;
        let nested_local = nested[..nested_central_offset].to_vec();
        let mut package = write_zip(&[
            ZipEntry {
                name: CONTENT_TYPES_PATH,
                contents: nested_local,
            },
            ZipEntry {
                name: ROOT_RELATIONSHIPS_PATH,
                contents: b"root".to_vec(),
            },
        ])
        .expect("outer ZIP package");

        let central_offset =
            u32::from_le_bytes(package[package.len() - 6..package.len() - 2].try_into().unwrap())
                as usize;
        let second_central_entry = central_offset + 46 + CONTENT_TYPES_PATH.len();
        let nested_offset = 30 + CONTENT_TYPES_PATH.len();
        package[second_central_entry + 42..second_central_entry + 46]
            .copy_from_slice(&(nested_offset as u32).to_le_bytes());

        assert!(matches!(
            read_zip_parts(&package),
            Err(AasxImportError::UnsupportedZip(_))
        ));
    }

    #[test]
    fn rejects_mismatched_zip_local_and_central_extra_lengths() {
        let mut package = write_zip(&[ZipEntry {
            name: ROOT_RELATIONSHIPS_PATH,
            contents: b"root".to_vec(),
        }])
        .expect("ZIP entry");
        let central_offset =
            u32::from_le_bytes(package[package.len() - 6..package.len() - 2].try_into().unwrap())
                as usize;
        let central_entry_end = central_offset + 46 + ROOT_RELATIONSHIPS_PATH.len();

        package[central_offset + 30..central_offset + 32].copy_from_slice(&1u16.to_le_bytes());
        package.insert(central_entry_end, 0);
        let eocd_offset = package.len() - 22;
        let central_size =
            u32::from_le_bytes(package[eocd_offset + 12..eocd_offset + 16].try_into().unwrap())
                + 1;
        package[eocd_offset + 12..eocd_offset + 16]
            .copy_from_slice(&central_size.to_le_bytes());

        assert!(matches!(
            read_zip_parts(&package),
            Err(AasxImportError::UnsupportedZip(_))
        ));
    }

    #[test]
    fn exporter_enforces_zip_part_and_package_budgets() {
        let too_large_part = vec![0; MAX_AASX_PART_SIZE as usize + 1];
        assert!(matches!(
            write_zip(&[ZipEntry {
                name: "part.bin",
                contents: too_large_part,
            }]),
            Err(AasxExportError::PackageTooLarge)
        ));

        let entries = (0..3)
            .map(|index| ZipEntry {
                name: match index {
                    0 => "part-0.bin",
                    1 => "part-1.bin",
                    _ => "part-2.bin",
                },
                contents: vec![0; MAX_AASX_PART_SIZE as usize],
            })
            .collect::<Vec<_>>();
        assert!(matches!(
            write_zip(&entries),
            Err(AasxExportError::PackageTooLarge)
        ));
    }

    fn package_from_entries(entries: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
        let zip_entries = PACKAGE_PARTS
            .iter()
            .filter_map(|&name| {
                entries.get(name).map(|contents| ZipEntry {
                    name,
                    contents: contents.clone(),
                })
            })
            .collect::<Vec<_>>();
        write_zip(&zip_entries).unwrap()
    }

    fn replace_part(package: &[u8], path: &str, replacement: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
        let mut entries = stored_entries(package);
        replacement(entries.get_mut(path).unwrap());
        package_from_entries(&entries)
    }

    #[test]
    fn imports_exported_package_with_canonical_equivalence() {
        let apparatus = valid_apparatus();
        let package = export_aasx(&apparatus).unwrap();

        let imported = import_aasx(&package).unwrap();
        assert_eq!(imported, apparatus);
        assert_eq!(export_aasx(&imported).unwrap(), package);
    }

    #[test]
    fn imports_exported_package_with_all_optional_configuration() {
        let mut apparatus = valid_apparatus();
        apparatus.capability_profiles = vec![CapabilityProfile {
            code: CapabilityCode::Print,
            level: 3,
            valid_from_unix: Some(10),
            valid_to_unix: Some(20),
            enabled: false,
        }];
        apparatus.policies.tooling = ToolingPolicy::QolipScanRequired;
        apparatus.policies.material = MaterialPolicy {
            requires_material: true,
            start_policy: RawMaterialStartPolicy::RequirementGroups,
            item_groups: Vec::new(),
            requirement_groups: vec![MaterialRequirementGroup {
                name: "substrate".to_string(),
                item_groups: vec!["paper".to_string(), "film".to_string()],
                min_required_count: 1,
            }],
        };
        apparatus.placement = Some(PlacementReference {
            factory_map_object_id: "factory-map:stable-001".to_string(),
        });
        apparatus.provenance = Provenance {
            source: CatalogSource::Custom,
            source_ref: Some("catalog:source:stable-001".to_string()),
        };
        apparatus.validate().unwrap();

        let package = export_aasx(&apparatus).unwrap();
        assert_eq!(import_aasx(&package).unwrap(), apparatus);
    }

    #[test]
    fn rejects_missing_required_package_parts() {
        for missing in [
            CONTENT_TYPES_PATH,
            ROOT_RELATIONSHIPS_PATH,
            AASX_ORIGIN_RELATIONSHIPS_PATH,
            AAS_SPEC_PATH,
        ] {
            let package = export_aasx(&valid_apparatus()).unwrap();
            let mut entries = stored_entries(&package);
            entries.remove(missing);

            assert!(
                import_aasx(&package_from_entries(&entries)).is_err(),
                "missing part was accepted: {missing}"
            );
        }
    }

    #[test]
    fn rejects_malformed_content_types_and_payload_xml() {
        let package = export_aasx(&valid_apparatus()).unwrap();
        let malformed_content_types = replace_part(&package, CONTENT_TYPES_PATH, |contents| {
            *contents = b"<Types".to_vec();
        });
        assert!(matches!(
            import_aasx(&malformed_content_types),
            Err(AasxImportError::InvalidXml(_))
        ));

        let malformed_payload = replace_part(&package, AAS_SPEC_PATH, |contents| {
            *contents = b"<environment".to_vec();
        });
        assert!(matches!(
            import_aasx(&malformed_payload),
            Err(AasxImportError::InvalidXml(_))
        ));
    }

    #[test]
    fn rejects_duplicate_relationships() {
        let package = export_aasx(&valid_apparatus()).unwrap();
        let duplicate = replace_part(&package, AASX_ORIGIN_RELATIONSHIPS_PATH, |contents| {
            let xml = String::from_utf8(contents.clone()).unwrap();
            let duplicate = xml
                .replace("</Relationships>", "  <Relationship Id=\"aasSpec\" Type=\"http://admin-shell.io/aasx/relationships/aas-spec\" Target=\"data.xml\"/>\n</Relationships>");
            *contents = duplicate.into_bytes();
        });

        assert!(matches!(
            import_aasx(&duplicate),
            Err(AasxImportError::MalformedPackage(_))
        ));
    }

    #[test]
    fn rejects_ambiguous_aas_shell_selection() {
        let package = export_aasx(&valid_apparatus()).unwrap();
        let ambiguous = replace_part(&package, AAS_SPEC_PATH, |contents| {
            let xml = String::from_utf8(contents.clone()).unwrap();
            let shell_start = xml
                .find("<assetAdministrationShell>")
                .expect("AAS shell start");
            let shell_end = xml
                .find("</assetAdministrationShell>")
                .map(|offset| offset + "</assetAdministrationShell>".len())
                .expect("AAS shell end");
            *contents = format!(
                "{}{}{}",
                &xml[..shell_end],
                &xml[shell_start..shell_end],
                &xml[shell_end..]
            )
            .into_bytes();
        });

        assert!(matches!(
            import_aasx(&ambiguous),
            Err(AasxImportError::InvalidXml(_))
        ));
    }

    #[test]
    fn rejects_absolute_and_path_traversal_relationship_targets() {
        let package = export_aasx(&valid_apparatus()).unwrap();
        for target in ["../data.xml", "/aasx/data.xml"] {
            let unsafe_target =
                replace_part(&package, AASX_ORIGIN_RELATIONSHIPS_PATH, |contents| {
                    let xml = String::from_utf8(contents.clone()).unwrap();
                    *contents = xml
                        .replace("Target=\"data.xml\"", &format!("Target=\"{target}\""))
                        .into_bytes();
                });
            assert!(matches!(
                import_aasx(&unsafe_target),
                Err(AasxImportError::MalformedPackage(_))
            ));
        }
    }

    #[test]
    fn rejects_invalid_xml_control_character() {
        let package = export_aasx(&valid_apparatus()).unwrap();
        let invalid_xml = replace_part(&package, AAS_SPEC_PATH, |contents| {
            contents.insert(0, 0x01);
        });

        assert!(matches!(
            import_aasx(&invalid_xml),
            Err(AasxImportError::InvalidXmlCharacter { code: 0x1 })
        ));
    }

    #[test]
    fn rejects_invalid_canonical_id_without_deriving_one_from_display_name() {
        let package = export_aasx(&valid_apparatus()).unwrap();
        let invalid_id = replace_part(&package, AAS_SPEC_PATH, |contents| {
            let xml = String::from_utf8(contents.clone()).unwrap();
            *contents = xml
                .replace("apparatus:catalog:stable-001", "apparatus:invalid id")
                .into_bytes();
        });

        assert!(matches!(
            import_aasx(&invalid_id),
            Err(AasxImportError::InvalidApparatus(
                super::super::ApparatusValidationError::InvalidIdCharacters
            ))
        ));
    }

    #[test]
    fn rejects_empty_material_item_group_from_aasx() {
        let mut apparatus = valid_apparatus();
        apparatus.policies.material.requires_material = true;
        apparatus.policies.material.item_groups = vec!["paper".to_string()];
        let package = export_aasx(&apparatus).unwrap();
        let malformed = replace_part(&package, AAS_SPEC_PATH, |contents| {
            let xml = String::from_utf8(contents.clone()).unwrap();
            *contents = xml
                .replace("<value>paper</value>", "<value></value>")
                .into_bytes();
        });

        assert!(matches!(
            import_aasx(&malformed),
            Err(AasxImportError::InvalidXml(_))
        ));
    }
}
