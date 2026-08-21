//! Bounded deterministic OPC package codec for canonical apparatus AASX.

mod opc;
mod zip;

use thiserror::Error;

use self::opc::{content_types_xml, origin_relationships_xml, root_relationships_xml};
use self::zip::{ZipEntry, read_zip_parts, write_zip};

pub const CONTENT_TYPES_PATH: &str = "[Content_Types].xml";
pub const ROOT_RELATIONSHIPS_PATH: &str = "_rels/.rels";
pub const AASX_ORIGIN_PATH: &str = "aasx/aasx-origin";
pub const AASX_ORIGIN_RELATIONSHIPS_PATH: &str = "aasx/_rels/aasx-origin.rels";
pub const AAS_SPEC_PATH: &str = "aasx/data.xml";

pub(super) const MAX_AASX_PACKAGE_SIZE: usize = 16 * 1024 * 1024;
pub(super) const MAX_AASX_PART_SIZE: usize = 8 * 1024 * 1024;
pub(super) const PACKAGE_PARTS: [&str; 5] = [
    CONTENT_TYPES_PATH,
    ROOT_RELATIONSHIPS_PATH,
    AASX_ORIGIN_PATH,
    AASX_ORIGIN_RELATIONSHIPS_PATH,
    AAS_SPEC_PATH,
];

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AasxExportError {
    #[error("AASX package is too large for the bounded ZIP32 profile")]
    PackageTooLarge,
    #[error("AASX XML part exceeds the supported size budget")]
    XmlTooLarge,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AasxImportError {
    #[error("AASX package is malformed: {0}")]
    MalformedPackage(&'static str),
    #[error("AASX package contains unsupported ZIP data: {0}")]
    UnsupportedZip(&'static str),
}

/// Validate the project OPC graph and return the exact AAS specification part.
pub(crate) fn validated_aas_spec(package: &[u8]) -> Result<Vec<u8>, AasxImportError> {
    let parts = read_zip_parts(package)?;
    if parts.len() != PACKAGE_PARTS.len() {
        return Err(AasxImportError::MalformedPackage(
            "AASX package must contain exactly the canonical parts",
        ));
    }
    require_exact(&parts, CONTENT_TYPES_PATH, content_types_xml().as_bytes())?;
    require_exact(
        &parts,
        ROOT_RELATIONSHIPS_PATH,
        root_relationships_xml().as_bytes(),
    )?;
    require_exact(&parts, AASX_ORIGIN_PATH, &[])?;
    require_exact(
        &parts,
        AASX_ORIGIN_RELATIONSHIPS_PATH,
        origin_relationships_xml().as_bytes(),
    )?;
    parts
        .get(AAS_SPEC_PATH)
        .cloned()
        .ok_or(AasxImportError::MalformedPackage(
            "canonical AAS specification part is missing",
        ))
}

fn require_exact(
    parts: &std::collections::BTreeMap<String, Vec<u8>>,
    path: &str,
    expected: &[u8],
) -> Result<(), AasxImportError> {
    match parts.get(path) {
        Some(actual) if actual == expected => Ok(()),
        Some(_) => Err(AasxImportError::MalformedPackage(
            "canonical OPC package graph was modified",
        )),
        None => Err(AasxImportError::MalformedPackage(
            "required canonical OPC part is missing",
        )),
    }
}

/// Build deterministic project AASX bytes around validated canonical AAS XML.
pub(crate) fn package_from_aas_xml(aas_xml: Vec<u8>) -> Result<Vec<u8>, AasxExportError> {
    if aas_xml.len() > MAX_AASX_PART_SIZE {
        return Err(AasxExportError::XmlTooLarge);
    }
    write_zip(&[
        ZipEntry::new(CONTENT_TYPES_PATH, content_types_xml().into_bytes()),
        ZipEntry::new(
            ROOT_RELATIONSHIPS_PATH,
            root_relationships_xml().into_bytes(),
        ),
        ZipEntry::new(AASX_ORIGIN_PATH, Vec::new()),
        ZipEntry::new(
            AASX_ORIGIN_RELATIONSHIPS_PATH,
            origin_relationships_xml().into_bytes(),
        ),
        ZipEntry::new(AAS_SPEC_PATH, aas_xml),
    ])
}

#[cfg(test)]
mod tests;
