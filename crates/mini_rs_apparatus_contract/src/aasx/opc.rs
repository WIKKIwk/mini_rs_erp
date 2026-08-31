use super::{AAS_SPEC_PATH, AASX_ORIGIN_PATH};

const OPC_CONTENT_TYPES_NAMESPACE: &str =
    "http://schemas.openxmlformats.org/package/2006/content-types";
const OPC_RELATIONSHIPS_NAMESPACE: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships";
const AASX_ORIGIN_RELATIONSHIP: &str = "http://admin-shell.io/aasx/relationships/aasx-origin";
const AASX_SPEC_RELATIONSHIP: &str = "http://admin-shell.io/aasx/relationships/aas-spec";

pub(super) fn content_types_xml() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Types xmlns=\"{OPC_CONTENT_TYPES_NAMESPACE}\">\n  <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\n  <Default Extension=\"xml\" ContentType=\"application/xml\"/>\n  <Override PartName=\"/{AASX_ORIGIN_PATH}\" ContentType=\"application/asset-administration-shell-package+xml\"/>\n  <Override PartName=\"/{AAS_SPEC_PATH}\" ContentType=\"application/xml\"/>\n</Types>\n"
    )
}

pub(super) fn root_relationships_xml() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Relationships xmlns=\"{OPC_RELATIONSHIPS_NAMESPACE}\">\n  <Relationship Id=\"aasxOrigin\" Type=\"{AASX_ORIGIN_RELATIONSHIP}\" Target=\"/{AASX_ORIGIN_PATH}\"/>\n</Relationships>\n"
    )
}

pub(super) fn origin_relationships_xml() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Relationships xmlns=\"{OPC_RELATIONSHIPS_NAMESPACE}\">\n  <Relationship Id=\"aasSpec\" Type=\"{AASX_SPEC_RELATIONSHIP}\" Target=\"data.xml\"/>\n</Relationships>\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aasx::{CONTENT_TYPES_PATH, ROOT_RELATIONSHIPS_PATH};

    #[test]
    fn package_graph_has_only_fixed_canonical_targets() {
        assert!(content_types_xml().contains(&format!("/{AASX_ORIGIN_PATH}")));
        assert!(content_types_xml().contains(&format!("/{AAS_SPEC_PATH}")));
        assert!(root_relationships_xml().contains(&format!("/{AASX_ORIGIN_PATH}")));
        assert!(origin_relationships_xml().contains("Target=\"data.xml\""));
        assert!(!origin_relationships_xml().contains(".."));
        assert_eq!(CONTENT_TYPES_PATH, "[Content_Types].xml");
        assert_eq!(ROOT_RELATIONSHIPS_PATH, "_rels/.rels");
    }
}
