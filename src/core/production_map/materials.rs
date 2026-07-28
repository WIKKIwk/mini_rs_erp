use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::materials_support::*;
use super::queue_state;
use super::{
    ApparatusQueueActionResult, OrderControlState, PreparedApparatusQueueAction,
    ProductionMapError, ProductionMapService, QueueActionActor, QueueProgressInput, chain,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusMaterialRequirementGroup {
    pub name: String,
    #[serde(default)]
    pub item_groups: Vec<String>,
    #[serde(default = "default_min_required_count")]
    pub min_required_count: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RawMaterialStartPolicy {
    #[default]
    StateAll,
    RequirementGroups,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusMaterialRule {
    pub apparatus: String,
    #[serde(default)]
    pub requires_material: bool,
    #[serde(default)]
    pub start_policy: RawMaterialStartPolicy,
    #[serde(default)]
    pub item_groups: Vec<String>,
    #[serde(default)]
    pub requirement_groups: Vec<ApparatusMaterialRequirementGroup>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusMaterialRuleUpsert {
    pub apparatus: String,
    #[serde(default)]
    pub requires_material: bool,
    #[serde(default)]
    pub start_policy: RawMaterialStartPolicy,
    #[serde(default)]
    pub item_groups: Vec<String>,
    #[serde(default)]
    pub requirement_groups: Vec<ApparatusMaterialRequirementGroup>,
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
    #[serde(default)]
    pub item_group_path: Vec<String>,
    #[serde(default)]
    pub apparatus: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawMaterialAssignmentDeleteInput {
    pub order_id: String,
    pub barcode: String,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RawMaterialStartRequirements {
    pub policy: RawMaterialStartPolicy,
    pub requires_material: bool,
    pub requirement_groups: Vec<ApparatusMaterialRequirementGroup>,
    pub assigned_barcodes: Vec<String>,
    pub staged_barcodes: Vec<String>,
    pub eligible_barcodes: Vec<String>,
    pub required_scan_count: usize,
    pub matched_scan_count: usize,
    pub assignments_satisfied: bool,
    pub scan_satisfied: bool,
}

pub struct MaterialScanProgressAction<'a> {
    pub apparatus: &'a str,
    pub order_id: &'a str,
    pub action: queue_state::ApparatusQueueAction,
    pub assigned_apparatus: &'a [String],
    pub actor: QueueActionActor,
    pub material_barcode: &'a str,
    pub state_material_barcodes: &'a [String],
    pub progress: QueueProgressInput,
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

    pub async fn raw_material_start_requirements(
        &self,
        apparatus: &str,
        order_id: &str,
        state_material_barcodes: &[String],
        material_barcodes: &str,
    ) -> Result<RawMaterialStartRequirements, ProductionMapError> {
        let assignments = self
            .raw_material_assignments_for_order_apparatus(order_id, apparatus)
            .await?;
        let rule = self.material_rule_for_apparatus(apparatus).await?;
        Ok(build_raw_material_start_requirements(
            rule.as_ref(),
            &assignments,
            state_material_barcodes,
            material_barcodes,
        ))
    }

    pub async fn unlink_raw_material_assignment(
        &self,
        input: RawMaterialAssignmentDeleteInput,
    ) -> Result<RawMaterialAssignment, ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        let order_id = input.order_id.trim().to_string();
        let barcode = normalize_barcode(&input.barcode);
        if order_id.is_empty() || barcode.is_empty() {
            return Err(ProductionMapError::RawMaterialInvalidInput);
        }
        let removed = self
            .store
            .delete_raw_material_assignment(&order_id, &barcode)
            .await?
            .ok_or(ProductionMapError::RawMaterialAssignmentNotFound)?;
        self.notify_live();
        Ok(removed)
    }

    pub async fn assign_raw_material_to_order(
        &self,
        input: RawMaterialAssignmentInput,
        actor: &QueueActionActor,
    ) -> Result<RawMaterialAssignment, ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        let assignment = self.prepare_raw_material_assignment(input, actor).await?;
        self.store
            .put_raw_material_assignment(assignment.clone())
            .await?;
        self.notify_live();
        Ok(assignment)
    }

    pub async fn receive_raw_material_for_active_order(
        &self,
        input: RawMaterialAssignmentInput,
        assigned_apparatus: &[String],
        actor: &QueueActionActor,
    ) -> Result<(RawMaterialAssignment, Vec<String>), ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        let normalized_barcode = normalize_barcode(&input.barcode);
        let requested_apparatus = input.apparatus.trim().to_string();
        let item_group_path =
            normalize_group_path(&input.item_group, input.item_group_path.clone());
        let existing = self
            .store
            .raw_material_assignments()
            .await?
            .into_iter()
            .find(|assignment| same_barcode(&assignment.barcode, &normalized_barcode));
        let assignment = match existing {
            Some(assignment)
                if assignment.order_id.trim() == input.order_id.trim()
                    && (requested_apparatus.is_empty()
                        || queue_state::apparatus_titles_match(
                            &assignment.apparatus,
                            &requested_apparatus,
                        )) =>
            {
                assignment
            }
            Some(_) => return Err(ProductionMapError::RawMaterialAlreadyAssigned),
            None => return Err(ProductionMapError::RawMaterialAssignmentNotFound),
        };
        if !queue_state::apparatus_matches_assigned(&assignment.apparatus, assigned_apparatus) {
            return Err(ProductionMapError::ApparatusNotAssigned);
        }
        let queue_states = self.store.apparatus_queue_states().await?;
        let is_active = queue_states.iter().any(|(apparatus, states)| {
            queue_state::apparatus_titles_match(apparatus, &assignment.apparatus)
                && states
                    .get(&assignment.order_id)
                    .and_then(|state| queue_state::ApparatusQueueOrderState::parse(state))
                    .is_some_and(queue_state::ApparatusQueueOrderState::is_active)
        });
        if !is_active {
            return Err(ProductionMapError::RawMaterialOrderNotActive);
        }
        if let Some(control) = self
            .store
            .order_control_states()
            .await?
            .get(&assignment.order_id)
        {
            match control.state {
                OrderControlState::Active => {}
                OrderControlState::FreezeRequested => {
                    return Err(ProductionMapError::OrderFreezeRequested);
                }
                OrderControlState::Frozen => return Err(ProductionMapError::OrderFrozen),
            }
        }
        if self
            .material_rule_for_apparatus(&assignment.apparatus)
            .await?
            .is_some_and(|rule| {
                !rule_matches(
                    &rule,
                    &assignment.apparatus,
                    &item_group_path,
                )
            })
        {
            return Err(ProductionMapError::RawMaterialGroupNotAllowed);
        }
        let warehouses = self
            .store
            .receive_raw_material_assignment(assignment.clone(), actor)
            .await?;
        self.notify_live();
        Ok((assignment, warehouses))
    }

    async fn prepare_raw_material_assignment(
        &self,
        input: RawMaterialAssignmentInput,
        actor: &QueueActionActor,
    ) -> Result<RawMaterialAssignment, ProductionMapError> {
        let order_id = input.order_id.trim().to_string();
        let barcode = normalize_barcode(&input.barcode);
        let item_code = input.item_code.trim().to_string();
        let item_group = input.item_group.trim().to_string();
        let item_group_path = normalize_group_path(&item_group, input.item_group_path);
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
        let apparatus_options = self
            .resolve_material_apparatus_options(&map, &item_group_path)
            .await?;
        let requested_apparatus = input.apparatus.trim();
        let apparatus = if requested_apparatus.is_empty() {
            match apparatus_options.len() {
                0 => return Err(ProductionMapError::RawMaterialGroupNotAllowed),
                1 => apparatus_options[0].clone(),
                _ => {
                    return Err(ProductionMapError::RawMaterialGroupAmbiguous(
                        apparatus_options,
                    ));
                }
            }
        } else {
            apparatus_options
                .iter()
                .find(|candidate| {
                    queue_state::apparatus_titles_match(candidate, requested_apparatus)
                })
                .cloned()
                .ok_or(ProductionMapError::RawMaterialGroupNotAllowed)?
        };
        for existing in self.store.raw_material_assignments().await? {
            if same_barcode(&existing.barcode, &barcode) {
                if existing.order_id.trim() == order_id
                    && queue_state::apparatus_titles_match(&existing.apparatus, &apparatus)
                {
                    return Err(ProductionMapError::RawMaterialAlreadyAssignedToOrder);
                }
                return Err(ProductionMapError::RawMaterialAlreadyAssigned);
            }
        }
        Ok(RawMaterialAssignment {
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
        })
    }

    pub async fn apply_apparatus_queue_action_with_material_scan(
        &self,
        apparatus: &str,
        order_id: &str,
        action: queue_state::ApparatusQueueAction,
        assigned_apparatus: &[String],
        actor: QueueActionActor,
        material_barcode: &str,
        state_material_barcodes: &[String],
    ) -> Result<std::collections::BTreeMap<String, String>, ProductionMapError> {
        if !queue_state::apparatus_matches_assigned(apparatus, assigned_apparatus) {
            return Err(ProductionMapError::ApparatusNotAssigned);
        }
        self.validate_material_scan(
            apparatus,
            order_id,
            action,
            material_barcode,
            state_material_barcodes,
        )
        .await?;
        self.apply_apparatus_queue_action(apparatus, order_id, action, assigned_apparatus, actor)
            .await
    }

    pub async fn apply_apparatus_queue_action_with_material_scan_and_progress(
        &self,
        request: MaterialScanProgressAction<'_>,
    ) -> Result<ApparatusQueueActionResult, ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        let prepared = self
            .prepare_apparatus_queue_action_with_material_scan_and_progress(request)
            .await?;
        self.commit_prepared_queue_action(prepared).await
    }

    pub(crate) async fn prepare_apparatus_queue_action_with_material_scan_and_progress(
        &self,
        request: MaterialScanProgressAction<'_>,
    ) -> Result<PreparedApparatusQueueAction, ProductionMapError> {
        let MaterialScanProgressAction {
            apparatus,
            order_id,
            action,
            assigned_apparatus,
            actor,
            material_barcode,
            state_material_barcodes,
            progress,
        } = request;
        if !queue_state::apparatus_matches_assigned(apparatus, assigned_apparatus) {
            return Err(ProductionMapError::ApparatusNotAssigned);
        }
        self.validate_material_scan(
            apparatus,
            order_id,
            action,
            material_barcode,
            state_material_barcodes,
        )
        .await?;
        self.prepare_apparatus_queue_action_with_progress(
            apparatus,
            order_id,
            action,
            assigned_apparatus,
            actor,
            progress,
        )
        .await
    }

    async fn resolve_material_apparatus_options(
        &self,
        map: &super::ProductionMapDefinition,
        item_group_path: &[String],
    ) -> Result<Vec<String>, ProductionMapError> {
        let rules = self.store.apparatus_material_rules().await?;
        let mut matches = BTreeSet::new();
        for stage in chain::linear_work_stages(map) {
            if rules
                .iter()
                .any(|rule| rule_matches(rule, &stage.station_title, item_group_path))
            {
                matches.insert(stage.station_title);
            }
        }
        Ok(matches.into_iter().collect())
    }

    async fn validate_material_scan(
        &self,
        apparatus: &str,
        order_id: &str,
        action: queue_state::ApparatusQueueAction,
        material_barcode: &str,
        state_material_barcodes: &[String],
    ) -> Result<(), ProductionMapError> {
        if !matches!(action, queue_state::ApparatusQueueAction::Start) {
            return Ok(());
        }
        let assignments = self
            .raw_material_assignments_for_order_apparatus(order_id, apparatus)
            .await?;
        let rule = self.material_rule_for_apparatus(apparatus).await?;
        let requirements = build_raw_material_start_requirements(
            rule.as_ref(),
            &assignments,
            state_material_barcodes,
            material_barcode,
        );
        if assignments.is_empty() {
            if !requirements.assignments_satisfied {
                return Err(ProductionMapError::RawMaterialAssignmentNotFound);
            }
            return Ok(());
        }
        if !requirements.assignments_satisfied {
            return Err(ProductionMapError::RawMaterialAssignmentNotFound);
        }
        let scanned = normalized_barcodes(material_barcode);
        if scanned.is_empty() {
            return Err(ProductionMapError::RawMaterialScanRequired);
        }
        let assigned = assignments
            .iter()
            .map(|assignment| normalize_barcode(&assignment.barcode))
            .collect::<BTreeSet<_>>();
        if !scanned.is_subset(&assigned) {
            return Err(ProductionMapError::RawMaterialMismatch);
        }
        match rule
            .as_ref()
            .map(|rule| rule.start_policy)
            .unwrap_or_default()
        {
            RawMaterialStartPolicy::StateAll => {
                let staged = state_material_barcodes
                    .iter()
                    .map(|barcode| normalize_barcode(barcode))
                    .filter(|barcode| assigned.contains(barcode))
                    .collect::<BTreeSet<_>>();
                if staged.is_empty() {
                    return Err(ProductionMapError::RawMaterialStateNotReady);
                }
                if !requirements.scan_satisfied {
                    return Err(ProductionMapError::RawMaterialScanIncomplete);
                }
            }
            RawMaterialStartPolicy::RequirementGroups => {
                if !requirements.scan_satisfied {
                    return Err(ProductionMapError::RawMaterialRequirementNotMet);
                }
            }
        }
        Ok(())
    }

    async fn raw_material_assignments_for_order_apparatus(
        &self,
        order_id: &str,
        apparatus: &str,
    ) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
        Ok(self
            .store
            .raw_material_assignments()
            .await?
            .into_iter()
            .filter(|assignment| {
                assignment.order_id.trim() == order_id.trim()
                    && queue_state::apparatus_titles_match(&assignment.apparatus, apparatus)
            })
            .collect())
    }

    async fn material_rule_for_apparatus(
        &self,
        apparatus: &str,
    ) -> Result<Option<ApparatusMaterialRule>, ProductionMapError> {
        Ok(self
            .store
            .apparatus_material_rules()
            .await?
            .into_iter()
            .find(|rule| queue_state::apparatus_titles_match(&rule.apparatus, apparatus)))
    }
}

fn build_raw_material_start_requirements(
    rule: Option<&ApparatusMaterialRule>,
    assignments: &[RawMaterialAssignment],
    state_material_barcodes: &[String],
    material_barcodes: &str,
) -> RawMaterialStartRequirements {
    let policy = rule.map(|rule| rule.start_policy).unwrap_or_default();
    let requires_material = rule.is_some_and(|rule| rule.requires_material);
    let assigned = assignments
        .iter()
        .map(|assignment| normalize_barcode(&assignment.barcode))
        .filter(|barcode| !barcode.is_empty())
        .collect::<BTreeSet<_>>();
    let staged = state_material_barcodes
        .iter()
        .map(|barcode| normalize_barcode(barcode))
        .filter(|barcode| assigned.contains(barcode))
        .collect::<BTreeSet<_>>();
    let scanned = normalized_barcodes(material_barcodes);
    let requirement_groups = rule.map(effective_requirement_groups).unwrap_or_default();
    let assignments_satisfied = if assignments.is_empty() {
        !requires_material
    } else {
        !requires_material
            || policy != RawMaterialStartPolicy::RequirementGroups
            || rule.is_some_and(|rule| material_requirements_met(rule, assignments))
    };
    let (eligible, required_scan_count, matched_scan_count, scan_satisfied) = match policy {
        RawMaterialStartPolicy::StateAll => {
            let matched = scanned.intersection(&staged).count();
            (
                staged.clone(),
                staged.len(),
                matched,
                assignments.is_empty() && !requires_material
                    || !assignments.is_empty()
                        && !scanned.is_empty()
                        && scanned.is_subset(&assigned)
                        && scanned == staged,
            )
        }
        RawMaterialStartPolicy::RequirementGroups => {
            let scanned_assignments = assignments
                .iter()
                .filter(|assignment| scanned.contains(&normalize_barcode(&assignment.barcode)))
                .cloned()
                .collect::<Vec<_>>();
            let required = rule.map(material_requirement_slot_count).unwrap_or_default();
            let matched = rule
                .map(|rule| material_requirement_match_count(rule, &scanned_assignments))
                .unwrap_or_default();
            (
                assigned.clone(),
                required,
                matched,
                assignments.is_empty() && !requires_material
                    || !assignments.is_empty()
                        && scanned.is_subset(&assigned)
                        && required > 0
                        && matched == required,
            )
        }
    };
    RawMaterialStartRequirements {
        policy,
        requires_material,
        requirement_groups,
        assigned_barcodes: assigned.into_iter().collect(),
        staged_barcodes: staged.into_iter().collect(),
        eligible_barcodes: eligible.into_iter().collect(),
        required_scan_count,
        matched_scan_count,
        assignments_satisfied,
        scan_satisfied,
    }
}
