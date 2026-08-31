use std::fmt::Write;

use super::{CanonicalAasxExportError, CanonicalAasxImportError};
use crate::{
    ApparatusCapacity, CanonicalApparatusRevision, CapacityAvailability, MaterialExecutionPolicy,
    ToolingExecutionPolicy,
};

const AAS_XML_NAMESPACE: &str = "https://admin-shell.io/aas/3/0";
const PAYLOAD_ID_SHORT: &str = "CanonicalPayloadJson";
const MAX_CANONICAL_PAYLOAD_BYTES: usize = 512 * 1024;

pub(super) fn canonical_aas_environment(
    revision: &CanonicalApparatusRevision,
    payload: &str,
) -> Result<String, CanonicalAasxExportError> {
    if payload.len() > MAX_CANONICAL_PAYLOAD_BYTES {
        return Err(CanonicalAasxExportError::Serialization);
    }
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<environment xmlns=\"{AAS_XML_NAMESPACE}\" xmlns:xs=\"http://www.w3.org/2001/XMLSchema\">\n  <assetAdministrationShells>\n    <assetAdministrationShell>\n      <id>{}</id>\n      <idShort>Apparatus</idShort>\n      <assetInformation>\n        <assetKind>Instance</assetKind>\n        <globalAssetId>{}</globalAssetId>\n      </assetInformation>\n      <submodels>\n        <reference>\n          <type>ModelReference</type>\n          <keys>\n            <key>\n              <type>Submodel</type>\n              <value>{}</value>\n            </key>\n          </keys>\n        </reference>\n      </submodels>\n    </assetAdministrationShell>\n  </assetAdministrationShells>\n  <submodels>\n    <submodel>\n      <id>{}</id>\n      <idShort>ApparatusConfiguration</idShort>\n      <kind>Instance</kind>\n      <semanticId>\n        <keys>\n          <key>\n            <type>GlobalReference</type>\n            <value>{}</value>\n          </key>\n        </keys>\n      </semanticId>\n      <submodelElements>\n",
        escape(&revision.aas_identity.shell_id),
        escape(revision.physical_asset_id.as_str()),
        escape(&revision.aas_identity.submodel_id),
        escape(&revision.aas_identity.submodel_id),
        escape(&revision.aas_identity.semantic_id),
    );
    push_identity(&mut xml, revision);
    push_hierarchy(&mut xml, revision);
    push_capabilities(&mut xml, revision);
    push_execution(&mut xml, revision);
    push_policies(&mut xml, revision);
    push_capacity(&mut xml, revision);
    push_lifecycle_and_context(&mut xml, revision);
    push_revision_and_aas(&mut xml, revision);
    collection_start(&mut xml, "CanonicalPayload");
    property(&mut xml, PAYLOAD_ID_SHORT, "xs:string", payload);
    collection_end(&mut xml);
    xml.push_str("      </submodelElements>\n    </submodel>\n  </submodels>\n</environment>\n");
    validate_xml_characters(&xml)?;
    Ok(xml)
}

fn push_identity(xml: &mut String, revision: &CanonicalApparatusRevision) {
    collection_start(xml, "Identity");
    property(
        xml,
        "ApparatusId",
        "xs:string",
        revision.apparatus_id.as_str(),
    );
    property(
        xml,
        "PhysicalAssetId",
        "xs:string",
        revision.physical_asset_id.as_str(),
    );
    property(
        xml,
        "DisplayName",
        "xs:string",
        &revision.display.display_name,
    );
    property(
        xml,
        "Description",
        "xs:string",
        &revision.display.description,
    );
    property(
        xml,
        "CatalogOrder",
        "xs:unsignedInt",
        &revision.display.catalog_order.to_string(),
    );
    collection_end(xml);
}

fn push_hierarchy(xml: &mut String, revision: &CanonicalApparatusRevision) {
    collection_start(xml, "Isa95Hierarchy");
    property(
        xml,
        "EquipmentClassId",
        "xs:string",
        revision.equipment_class_id.as_str(),
    );
    for (name, value) in [
        ("EnterpriseId", &revision.hierarchy.enterprise_id),
        ("SiteId", &revision.hierarchy.site_id),
        ("AreaId", &revision.hierarchy.area_id),
        ("WorkCenterId", &revision.hierarchy.work_center_id),
        ("WorkUnitId", &revision.hierarchy.work_unit_id),
    ] {
        property(xml, name, "xs:string", value.as_str());
    }
    collection_end(xml);
}

fn push_capabilities(xml: &mut String, revision: &CanonicalApparatusRevision) {
    collection_start(xml, "Capabilities");
    for (index, capability) in revision.capabilities.iter().enumerate() {
        collection_start(xml, &format!("Capability{}", index + 1));
        property(xml, "Code", "xs:string", &enum_json_name(&capability.code));
        property(
            xml,
            "Level",
            "xs:unsignedShort",
            &capability.level.to_string(),
        );
        collection_end(xml);
    }
    collection_end(xml);
}

fn push_execution(xml: &mut String, revision: &CanonicalApparatusRevision) {
    let profile = &revision.execution_profile;
    collection_start(xml, "ExecutionProfile");
    property(
        xml,
        "Operation",
        "xs:string",
        &enum_json_name(&profile.operation),
    );
    property(
        xml,
        "Technology",
        "xs:string",
        &enum_json_name(&profile.technology),
    );
    if let Some(stations) = profile.color_station_count {
        property(
            xml,
            "ColorStationCount",
            "xs:unsignedShort",
            &stations.to_string(),
        );
    }
    if let Some(min_web_width_mm) = profile.min_web_width_mm {
        property(
            xml,
            "MinWebWidthMm",
            "xs:unsignedInt",
            &min_web_width_mm.to_string(),
        );
    }
    if let Some(max_web_width_mm) = profile.max_web_width_mm {
        property(
            xml,
            "MaxWebWidthMm",
            "xs:unsignedInt",
            &max_web_width_mm.to_string(),
        );
    }
    property(
        xml,
        "VirtualTasks",
        "xs:string",
        &enum_json_name(&profile.virtual_tasks),
    );
    property(
        xml,
        "CapabilityCompatibleReroute",
        "xs:boolean",
        bool_name(profile.capability_compatible_reroute),
    );
    collection_end(xml);
}

fn push_policies(xml: &mut String, revision: &CanonicalApparatusRevision) {
    collection_start(xml, "OperationalPolicies");
    property(
        xml,
        "QueueDiscipline",
        "xs:string",
        &enum_json_name(&revision.policies.queue),
    );
    collection_start(xml, "MaterialPolicy");
    match &revision.policies.material {
        MaterialExecutionPolicy::NotRequired { item_group_ids } => {
            property(xml, "Mode", "xs:string", "not_required");
            if !item_group_ids.is_empty() {
                push_indexed_values(xml, "ItemGroups", "ItemGroup", item_group_ids);
            }
        }
        MaterialExecutionPolicy::AllRequired { item_group_ids } => {
            property(xml, "Mode", "xs:string", "all_required");
            push_indexed_values(xml, "ItemGroups", "ItemGroup", item_group_ids);
        }
        MaterialExecutionPolicy::RequirementSets { sets } => {
            property(xml, "Mode", "xs:string", "requirement_sets");
            collection_start(xml, "RequirementSets");
            for (index, set) in sets.iter().enumerate() {
                collection_start(xml, &format!("RequirementSet{}", index + 1));
                property(xml, "RequirementId", "xs:string", &set.requirement_id);
                property(
                    xml,
                    "MinimumRequiredCount",
                    "xs:unsignedShort",
                    &set.minimum_required_count.to_string(),
                );
                push_indexed_values(xml, "ItemGroups", "ItemGroup", &set.item_group_ids);
                collection_end(xml);
            }
            collection_end(xml);
        }
    }
    collection_end(xml);
    collection_start(xml, "ToolingPolicy");
    match &revision.policies.tooling {
        ToolingExecutionPolicy::NotRequired => property(xml, "Mode", "xs:string", "not_required"),
        ToolingExecutionPolicy::QolipScanRequired { tooling_class_id } => {
            property(xml, "Mode", "xs:string", "qolip_scan_required");
            property(xml, "ToolingClassId", "xs:string", tooling_class_id);
        }
    }
    collection_end(xml);
    collection_end(xml);
}

fn push_capacity(xml: &mut String, revision: &CanonicalApparatusRevision) {
    let ApparatusCapacity {
        capacity_slots,
        setup_minutes,
        cleanup_minutes,
        efficiency_percent,
        finite_capacity,
        availability,
    } = &revision.capacity;
    collection_start(xml, "Capacity");
    for (name, value) in [
        ("CapacitySlots", capacity_slots.to_string()),
        ("SetupMinutes", setup_minutes.to_string()),
        ("CleanupMinutes", cleanup_minutes.to_string()),
        ("EfficiencyPercent", efficiency_percent.to_string()),
    ] {
        property(xml, name, "xs:unsignedInt", &value);
    }
    property(
        xml,
        "FiniteCapacity",
        "xs:boolean",
        bool_name(*finite_capacity),
    );
    match availability {
        CapacityAvailability::Always => property(xml, "Availability", "xs:string", "always"),
        CapacityAvailability::Scheduled { working_windows } => {
            property(xml, "Availability", "xs:string", "scheduled");
            collection_start(xml, "WorkingWindows");
            for (index, window) in working_windows.iter().enumerate() {
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
        }
    }
    collection_end(xml);
}

fn push_lifecycle_and_context(xml: &mut String, revision: &CanonicalApparatusRevision) {
    collection_start(xml, "Lifecycle");
    property(
        xml,
        "State",
        "xs:string",
        &enum_json_name(&revision.lifecycle.state),
    );
    if let Some(reason) = revision.lifecycle.retirement_reason.as_deref() {
        property(xml, "RetirementReason", "xs:string", reason);
    }
    collection_end(xml);
    collection_start(xml, "Training");
    property(
        xml,
        "Enabled",
        "xs:boolean",
        bool_name(revision.training.enabled),
    );
    property(
        xml,
        "QueueEnabled",
        "xs:boolean",
        bool_name(revision.training.queue_enabled),
    );
    property(
        xml,
        "MaterialTrackingEnabled",
        "xs:boolean",
        bool_name(revision.training.material_tracking_enabled),
    );
    collection_end(xml);
    if let Some(placement) = &revision.placement {
        collection_start(xml, "Placement");
        property(
            xml,
            "FactoryMapObjectId",
            "xs:string",
            &placement.factory_map_object_id,
        );
        collection_end(xml);
    }
}

fn push_revision_and_aas(xml: &mut String, revision: &CanonicalApparatusRevision) {
    let metadata = &revision.revision_metadata;
    collection_start(xml, "RevisionMetadata");
    property(
        xml,
        "SchemaVersion",
        "xs:unsignedInt",
        &revision.schema_version.to_string(),
    );
    property(
        xml,
        "Revision",
        "xs:unsignedLong",
        &metadata.revision.to_string(),
    );
    property(
        xml,
        "CommittedAtUnixMs",
        "xs:long",
        &metadata.committed_at_unix_ms.to_string(),
    );
    property(xml, "ActorId", "xs:string", &metadata.actor_id);
    property(xml, "CommandId", "xs:string", &metadata.command_id);
    property(
        xml,
        "Source",
        "xs:string",
        &enum_json_name(&metadata.source),
    );
    if let Some(reference) = metadata.source_reference.as_deref() {
        property(xml, "SourceReference", "xs:string", reference);
    }
    collection_end(xml);
    let aas = &revision.aas_identity;
    collection_start(xml, "AasContract");
    for (name, value) in [
        ("ShellId", aas.shell_id.as_str()),
        ("SubmodelId", aas.submodel_id.as_str()),
        ("SemanticId", aas.semantic_id.as_str()),
        ("IdtaRelease", aas.idta_release.as_str()),
        ("AasMetamodelVersion", aas.aas_metamodel_version.as_str()),
        ("AasxPart5Version", aas.aasx_part_5_version.as_str()),
        ("PackageFormat", aas.package_format.as_str()),
        ("MediaType", aas.media_type.as_str()),
    ] {
        property(xml, name, "xs:string", value);
    }
    collection_end(xml);
}

fn push_indexed_values(xml: &mut String, collection: &str, prefix: &str, values: &[String]) {
    collection_start(xml, collection);
    for (index, value) in values.iter().enumerate() {
        property(xml, &format!("{prefix}{}", index + 1), "xs:string", value);
    }
    collection_end(xml);
}

fn collection_start(xml: &mut String, id_short: &str) {
    writeln!(xml, "        <submodelElementCollection>").unwrap();
    writeln!(xml, "          <idShort>{}</idShort>", escape(id_short)).unwrap();
    writeln!(xml, "          <value>").unwrap();
}

fn collection_end(xml: &mut String) {
    xml.push_str("          </value>\n        </submodelElementCollection>\n");
}

fn property(xml: &mut String, id_short: &str, value_type: &str, value: &str) {
    xml.push_str("            <property>\n");
    writeln!(xml, "              <idShort>{}</idShort>", escape(id_short)).unwrap();
    writeln!(
        xml,
        "              <valueType>{}</valueType>",
        escape(value_type)
    )
    .unwrap();
    writeln!(xml, "              <value>{}</value>", escape(value)).unwrap();
    xml.push_str("            </property>\n");
}

fn enum_json_name<T: serde::Serialize>(value: &T) -> String {
    match serde_json::to_value(value).expect("serializing enum is infallible") {
        serde_json::Value::String(value) => value,
        _ => unreachable!("canonical enum serializes as a string"),
    }
}

fn bool_name(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn validate_xml_characters(value: &str) -> Result<(), CanonicalAasxExportError> {
    if let Some(character) = value.chars().find(|&character| {
        !matches!(
            character,
            '\u{9}' | '\u{A}' | '\u{D}' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}'
                | '\u{10000}'..='\u{10FFFF}'
        )
    }) {
        return Err(CanonicalAasxExportError::InvalidXmlCharacter {
            code: character as u32,
        });
    }
    Ok(())
}

pub(super) fn extract_canonical_payload(
    specification: &str,
) -> Result<String, CanonicalAasxImportError> {
    let marker = format!("<idShort>{PAYLOAD_ID_SHORT}</idShort>");
    let marker_start = specification
        .find(&marker)
        .ok_or(CanonicalAasxImportError::InvalidCanonicalPayload)?;
    if specification[marker_start + marker.len()..].contains(&marker) {
        return Err(CanonicalAasxImportError::InvalidCanonicalPayload);
    }
    let property_end = specification[marker_start..]
        .find("</property>")
        .map(|offset| marker_start + offset)
        .ok_or(CanonicalAasxImportError::InvalidCanonicalPayload)?;
    let value_start = specification[marker_start..property_end]
        .find("<value>")
        .map(|offset| marker_start + offset + "<value>".len())
        .ok_or(CanonicalAasxImportError::InvalidCanonicalPayload)?;
    let value_end = specification[value_start..property_end]
        .find("</value>")
        .map(|offset| value_start + offset)
        .ok_or(CanonicalAasxImportError::InvalidCanonicalPayload)?;
    let escaped = &specification[value_start..value_end];
    if escaped.len() > MAX_CANONICAL_PAYLOAD_BYTES {
        return Err(CanonicalAasxImportError::InvalidCanonicalPayload);
    }
    unescape(escaped)
}

fn unescape(value: &str) -> Result<String, CanonicalAasxImportError> {
    let mut output = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(offset) = remaining.find('&') {
        output.push_str(&remaining[..offset]);
        remaining = &remaining[offset..];
        let (entity, character) = [
            ("&amp;", '&'),
            ("&lt;", '<'),
            ("&gt;", '>'),
            ("&quot;", '"'),
            ("&apos;", '\''),
        ]
        .into_iter()
        .find(|(entity, _)| remaining.starts_with(entity))
        .ok_or(CanonicalAasxImportError::InvalidCanonicalPayload)?;
        output.push(character);
        remaining = &remaining[entity.len()..];
    }
    output.push_str(remaining);
    Ok(output)
}
