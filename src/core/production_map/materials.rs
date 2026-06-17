use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::queue_state;
use super::{ProductionMapError, ProductionMapService, QueueActionActor, chain};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusMaterialRule {
    pub apparatus: String,
    pub item_groups: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusMaterialRuleUpsert {
    pub apparatus: String,
    #[serde(default)]
    pub item_groups: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawMaterialAssignmentInput {
    pub order_id: String,
    pub barcode: String,
    #[serde(default)]
    pub item_code: String,
    #[serde(default)]
    pub item_name: String,
    #[serde(default)]
    pub item_group: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawMaterialAssignment {
    pub order_id: String,
    pub apparatus: String,
    pub barcode: String,
    pub item_code: String,
    pub item_name: String,
    pub item_group: String,
    pub assigned_by_role: String,
    pub assigned_by_ref: String,
    pub assigned_by_display_name: String,
    pub assigned_at: String,
}

impl ProductionMapService {
    pub async fn apparatus_material_rules(
        &self,
    ) -> Result<Vec<ApparatusMaterialRule>, ProductionMapError> {
        self.store.apparatus_material_rules().await
    }

    pub async fn set_apparatus_material_rule(
        &self,
        input: ApparatusMaterialRuleUpsert,
    ) -> Result<ApparatusMaterialRule, ProductionMapError> {
        let rule = normalize_rule(input)?;
        self.store.put_apparatus_material_rule(rule.clone()).await?;
        self.notify_live();
        Ok(rule)
    }

    pub async fn raw_material_assignments(
        &self,
    ) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
        self.store.raw_material_assignments().await
    }

    pub async fn assign_raw_material_to_order(
        &self,
        input: RawMaterialAssignmentInput,
        actor: &QueueActionActor,
    ) -> Result<RawMaterialAssignment, ProductionMapError> {
        let order_id = input.order_id.trim().to_string();
        let barcode = normalize_barcode(&input.barcode);
        let item_code = input.item_code.trim().to_string();
        let item_group = input.item_group.trim().to_string();
        if order_id.is_empty()
            || barcode.is_empty()
            || item_code.is_empty()
            || item_group.is_empty()
        {
            return Err(ProductionMapError::RawMaterialInvalidInput);
        }
        let Some(map) = self.raw_map(&order_id).await? else {
            return Err(ProductionMapError::MapNotFound);
        };
        let apparatus = self.resolve_material_apparatus(&map, &item_group).await?;
        for existing in self.store.raw_material_assignments().await? {
            if same_barcode(&existing.barcode, &barcode) {
                if existing.order_id.trim() == order_id
                    && queue_state::apparatus_titles_match(&existing.apparatus, &apparatus)
                {
                    return Ok(existing);
                }
                return Err(ProductionMapError::RawMaterialAlreadyAssigned);
            }
        }
        let assignment = RawMaterialAssignment {
            order_id,
            apparatus,
            barcode,
            item_code,
            item_name: blank_default(&input.item_name, &input.item_code),
            item_group,
            assigned_by_role: actor.role.trim().to_string(),
            assigned_by_ref: actor.ref_.trim().to_string(),
            assigned_by_display_name: actor.display_name.trim().to_string(),
            assigned_at: now_rfc3339(),
        };
        self.store
            .put_raw_material_assignment(assignment.clone())
            .await?;
        self.notify_live();
        Ok(assignment)
    }

    pub async fn apply_apparatus_queue_action_with_material_scan(
        &self,
        apparatus: &str,
        order_id: &str,
        action: queue_state::ApparatusQueueAction,
        assigned_apparatus: &[String],
        actor: QueueActionActor,
        material_barcode: &str,
    ) -> Result<std::collections::BTreeMap<String, String>, ProductionMapError> {
        if !queue_state::apparatus_matches_assigned(apparatus, assigned_apparatus) {
            return Err(ProductionMapError::ApparatusNotAssigned);
        }
        self.validate_material_scan(apparatus, order_id, action, material_barcode)
            .await?;
        self.apply_apparatus_queue_action(apparatus, order_id, action, assigned_apparatus, actor)
            .await
    }

    async fn resolve_material_apparatus(
        &self,
        map: &super::ProductionMapDefinition,
        item_group: &str,
    ) -> Result<String, ProductionMapError> {
        let rules = self.store.apparatus_material_rules().await?;
        let mut matches = BTreeSet::new();
        for stage in chain::linear_work_stages(map) {
            if rules
                .iter()
                .any(|rule| rule_matches(rule, &stage.station_title, item_group))
            {
                matches.insert(stage.station_title);
            }
        }
        match matches.len() {
            0 => Err(ProductionMapError::RawMaterialGroupNotAllowed),
            1 => Ok(matches.into_iter().next().unwrap_or_default()),
            _ => Err(ProductionMapError::RawMaterialGroupAmbiguous),
        }
    }

    async fn validate_material_scan(
        &self,
        apparatus: &str,
        order_id: &str,
        action: queue_state::ApparatusQueueAction,
        material_barcode: &str,
    ) -> Result<(), ProductionMapError> {
        if !matches!(action, queue_state::ApparatusQueueAction::Start) {
            return Ok(());
        }
        let assignments = self
            .store
            .raw_material_assignments()
            .await?
            .into_iter()
            .filter(|assignment| {
                assignment.order_id.trim() == order_id.trim()
                    && queue_state::apparatus_titles_match(&assignment.apparatus, apparatus)
            })
            .collect::<Vec<_>>();
        if assignments.is_empty() {
            return Ok(());
        }
        let scanned = normalized_barcodes(material_barcode);
        if scanned.is_empty() {
            return Err(ProductionMapError::RawMaterialScanRequired);
        }
        let assigned = assignments
            .iter()
            .map(|assignment| normalize_barcode(&assignment.barcode))
            .collect::<BTreeSet<_>>();
        if scanned != assigned {
            return Err(ProductionMapError::RawMaterialMismatch);
        }
        Ok(())
    }
}

fn normalize_rule(
    input: ApparatusMaterialRuleUpsert,
) -> Result<ApparatusMaterialRule, ProductionMapError> {
    let apparatus = input.apparatus.trim().to_string();
    if apparatus.is_empty() {
        return Err(ProductionMapError::RawMaterialInvalidInput);
    }
    let mut seen = BTreeSet::new();
    let item_groups = input
        .item_groups
        .into_iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .filter(|item| seen.insert(item.to_lowercase()))
        .collect::<Vec<_>>();
    if item_groups.is_empty() {
        return Err(ProductionMapError::RawMaterialInvalidInput);
    }
    Ok(ApparatusMaterialRule {
        apparatus,
        item_groups,
    })
}

fn rule_matches(rule: &ApparatusMaterialRule, apparatus: &str, item_group: &str) -> bool {
    queue_state::apparatus_titles_match(&rule.apparatus, apparatus)
        && rule
            .item_groups
            .iter()
            .any(|group| group.trim().eq_ignore_ascii_case(item_group.trim()))
}

fn normalize_barcode(value: &str) -> String {
    value.trim().to_ascii_uppercase()
}

fn normalized_barcodes(value: &str) -> BTreeSet<String> {
    value
        .split(',')
        .map(normalize_barcode)
        .filter(|item| !item.is_empty())
        .collect()
}

fn same_barcode(left: &str, right: &str) -> bool {
    normalize_barcode(left) == normalize_barcode(right)
}

fn blank_default(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.trim().to_string()
    } else {
        value.to_string()
    }
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}
