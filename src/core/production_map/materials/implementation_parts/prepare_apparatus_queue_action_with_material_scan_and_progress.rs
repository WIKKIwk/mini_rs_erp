impl ProductionMapService {
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
            material_barcodes,
            state_material_barcodes,
            progress,
            qolip_validation,
        } = request;
        let apparatus_id = parse_apparatus_id(apparatus)?;
        if !assigned_apparatus_contains(&apparatus_id, assigned_apparatus) {
            return Err(ProductionMapError::ApparatusNotAssigned);
        }
        self.enforce_qolip_start_boundary(
            apparatus_id.as_str(),
            order_id,
            action,
            qolip_validation.as_ref(),
        )
        .await?;
        let (reacquire_materials, reuse_materials) = self.resume_material_requirements(
            apparatus_id.as_str(), order_id, action,
        ).await?;
        if reacquire_materials && material_barcodes.is_empty() {
            return Err(ProductionMapError::RawMaterialScanRequired);
        }
        let skip_material_scan = matches!(action, queue_state::ApparatusQueueAction::Start)
            && self
                .laminatsiya_material_scan_can_be_skipped_for_wip(
                    &apparatus_id,
                    order_id,
                    &progress,
                )
                .await?;
        if !skip_material_scan {
            self.validate_material_scan(
                apparatus,
                order_id,
                if reacquire_materials { queue_state::ApparatusQueueAction::Start } else { action },
                material_barcodes,
                state_material_barcodes,
            )
            .await?;
        }
        let mut prepared = self
            .prepare_apparatus_queue_action_with_progress(
                apparatus_id.as_str(),
                order_id,
                action,
                assigned_apparatus,
                actor,
                progress,
            )
            .await?;
        prepared.material_scan_skipped = skip_material_scan || reuse_materials;
        if reuse_materials {
            prepared.event.payload_json["print_transfer_materials_reused"] = serde_json::json!(true);
            if let Some(session) = &mut prepared.session {
                session.payload_json["print_transfer_materials_required"] = serde_json::json!(false);
            }
        }
        if reacquire_materials {
            let transferred = prepared.session.as_ref().is_some_and(|session| session.is_print_transfer_continuation());
            prepared.event.payload_json[if transferred { "print_transfer_material_reacquisition" } else { "lamination_raw_reacquisition" }] = serde_json::json!(true);
            if let Some(session) = &mut prepared.session {
                session.payload_json["print_transfer_materials_required"] = serde_json::json!(false);
            }
        }
        if (action == queue_state::ApparatusQueueAction::Start || reacquire_materials) && !skip_material_scan {
            // Capture only the validated scans for this occurrence. Assignment
            // lists are plans; unscanned materials must never become use history.
            let scans = material_barcodes
                .iter()
                .map(|barcode| normalize_barcode(barcode))
                .collect::<BTreeSet<_>>();
            if scans.is_empty() {
                prepared.attach_start_materials(Vec::new());
            } else {
                let assignments = match self
                    .raw_material_assignments_for_order_apparatus(order_id, &apparatus_id).await
                {
                    Ok(assignments) => assignments,
                    Err(error) if reacquire_materials => return Err(error),
                    Err(_) => return Ok(prepared),
                };
                let materials = assignments
                    .into_iter()
                    .filter(|assignment| scans.contains(&normalize_barcode(&assignment.barcode)))
                    .map(|assignment| ProductionQrRawMaterial {
                        barcode: assignment.barcode,
                        item_code: assignment.item_code,
                        item_name: assignment.item_name,
                        stock_id: None,
                        source_qty: None,
                        uom: None,
                    })
                    .collect::<Vec<_>>();
                // Preserve existing Start acceptance semantics. If metadata cannot
                // be proven, leave history unavailable rather than changing validation.
                if materials.len() == scans.len() {
                    prepared.attach_start_materials(materials);
                } else if reacquire_materials {
                    return Err(ProductionMapError::RawMaterialMismatch);
                }
            }
        }
        Ok(prepared)
    }

    pub async fn requires_material_reacquisition_for_action(
        &self,
        apparatus: &str,
        order_id: &str,
        action: queue_state::ApparatusQueueAction,
    ) -> Result<bool, ProductionMapError> {
        Ok(self.resume_material_requirements(apparatus, order_id, action).await?.0)
    }

    async fn resume_material_requirements(
        &self,
        apparatus: &str,
        order_id: &str,
        action: queue_state::ApparatusQueueAction,
    ) -> Result<(bool, bool), ProductionMapError> {
        if action != queue_state::ApparatusQueueAction::Resume {
            return Ok((false, false));
        }
        let canonical = self.resolve_canonical_apparatus_text(apparatus).await?;
        let Some(session) = self.store.active_order_run_session(
            canonical.runtime.apparatus_id.as_str(), order_id,
        ).await? else {
            return Ok((false, false));
        };
        if matches!(session.status, OrderRunStatus::Paused | OrderRunStatus::RollDetached)
            && super::apparatus::uses_lamination_workflow(&canonical)
            && session.payload_json["lamination_astatka_raw_released"].as_bool() == Some(true)
        {
            return Ok((true, false));
        }
        if session.needs_print_transfer_material_check()
            && let Some(rule) = live_material_rule(&canonical).filter(|rule| rule.requires_material)
        {
            let assignments = self.store.raw_material_assignments_for_order(order_id).await?;
            let reuse = self.print_transfer_can_reuse_materials(&session, &rule, &assignments).await?;
            return Ok((!reuse, reuse));
        }
        Ok((false, false))
    }

    pub(super) async fn print_transfer_can_reuse_materials(
        &self,
        session: &OrderRunSession,
        rule: &ApparatusMaterialRule,
        assignments: &[RawMaterialAssignment],
    ) -> Result<bool, ProductionMapError> {
        let recorded = recorded_material_barcodes(session);
        if recorded.is_empty() {
            return Ok(false);
        }
        let scanned = normalized_barcodes_list(&recorded);
        let assignments = assignments.iter().filter(|assignment| {
            assignment.apparatus_id.as_str() == session.apparatus
                && scanned.contains(&normalize_barcode(&assignment.barcode))
        }).collect::<Vec<_>>();
        let requirements = build_raw_material_start_requirements_refs(
            Some(rule), &assignments, &recorded, &scanned,
        );
        if !requirements.scan_satisfied {
            return Ok(false);
        }
        let confirmed = self.store.in_use_materials_at_apparatus(
            &session.order_id, &session.apparatus,
        ).await?;
        Ok(normalized_barcodes_list(&confirmed) == scanned)
    }

    pub(crate) async fn in_use_print_transfer_materials(
        &self,
        order_id: &str,
        apparatus: &str,
    ) -> Result<Vec<String>, ProductionMapError> {
        if let Some(session) = self.store.active_order_run_session(apparatus, order_id).await?
            && session.needs_print_transfer_material_check()
        {
            let recorded = normalized_barcodes_list(&recorded_material_barcodes(&session));
            if !recorded.is_empty() {
                let mut confirmed = self.store.in_use_materials_at_apparatus(order_id, apparatus).await?;
                confirmed.retain(|barcode| recorded.contains(&normalize_barcode(barcode)));
                return Ok(confirmed);
            }
        }
        Ok(Vec::new())
    }

    async fn resolve_material_apparatus_options(
        &self,
        map: &super::ProductionMapDefinition,
        item_group_path: &[String],
    ) -> Result<Vec<String>, ProductionMapError> {
        let mut matches = BTreeSet::new();
        for stage in chain::linear_work_stages(map) {
            let Some(stage_apparatus_id) = stage.apparatus_id.as_deref() else {
                continue;
            };
            // The shared chain retains every alternative for WIP topology.
            // New material assignments must follow this occurrence's selection.
            let Some(node) = map.nodes.iter().find(|node| node.id == stage.node_id) else {
                continue;
            };
            let group = node.alternative_group_id.trim();
            if !group.is_empty()
                && map.nodes.iter().any(|candidate| {
                    candidate.kind == super::ProductionMapNodeKind::Apparatus
                        && candidate.alternative_group_id.trim() == group
                        && !candidate.alternative_assigned_apparatus_id.trim().is_empty()
                        && candidate.alternative_assigned_apparatus_id.trim() != stage_apparatus_id
                })
            {
                continue;
            }
            let stage_apparatus_id = parse_apparatus_id(stage_apparatus_id)?;
            if self
                .material_rule_for_apparatus(&stage_apparatus_id)
                .await?
                .is_some_and(|rule| rule_matches(&rule, &stage_apparatus_id, item_group_path))
            {
                matches.insert(stage_apparatus_id.to_string());
            }
        }
        Ok(matches.into_iter().collect())
    }
}

fn recorded_material_barcodes(session: &OrderRunSession) -> Vec<String> {
    ProductionQrSessionResources::recorded(session)
        .map(|resources| resources.raw_materials.into_iter().map(|row| row.barcode).collect())
        .unwrap_or_default()
}

impl ProductionMapService {

    async fn validate_material_scan(
        &self,
        apparatus: &str,
        order_id: &str,
        action: queue_state::ApparatusQueueAction,
        material_barcodes: &[String],
        state_material_barcodes: &[String],
    ) -> Result<(), ProductionMapError> {
        if !matches!(action, queue_state::ApparatusQueueAction::Start) {
            return Ok(());
        }
        let apparatus_id = parse_apparatus_id(apparatus)?;
        let canonical = self.validated_material_apparatus(&apparatus_id).await?;
        let rule = live_material_rule(&canonical);
        let scanned = normalized_barcodes_list(material_barcodes);
        if rule.is_none() {
            if !scanned.is_empty() {
                return Err(ProductionMapError::RawMaterialMismatch);
            }
            return Ok(());
        }
        let assignments = self
            .raw_material_assignments_for_order_apparatus(order_id, &apparatus_id)
            .await?;
        let requirements = build_raw_material_start_requirements(
            rule.as_ref(),
            &assignments,
            state_material_barcodes,
            &scanned,
        );
        if assignments.is_empty() {
            if !scanned.is_empty() {
                return Err(ProductionMapError::RawMaterialMismatch);
            }
            if !super::apparatus::uses_lamination_workflow(&canonical)
                && !requirements.assignments_satisfied
            {
                return Err(ProductionMapError::RawMaterialAssignmentNotFound);
            }
            return Ok(());
        }
        if !requirements.assignments_satisfied {
            return Err(ProductionMapError::RawMaterialAssignmentNotFound);
        }
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

    async fn laminatsiya_material_scan_can_be_skipped_for_wip(
        &self,
        apparatus: &ApparatusId,
        order_id: &str,
        progress: &QueueProgressInput,
    ) -> Result<bool, ProductionMapError> {
        let canonical = self.validated_material_apparatus(apparatus).await?;
        if !super::apparatus::uses_lamination_workflow(&canonical)
            || (progress.qr_payload.trim().is_empty()
                && progress.progress_batch_id.trim().is_empty())
        {
            return Ok(false);
        }
        let Some(order_map) = self.raw_map(order_id).await? else {
            return Ok(false);
        };
        if self
            .previous_stage_start_progress_batch(order_id, &order_map, apparatus.as_str(), progress)
            .await
            .ok()
            .flatten()
            .is_some()
        {
            return Ok(true);
        }
        Ok(self
            .opening_wip_start_batch(order_id, &order_map, apparatus.as_str(), progress)
            .await
            .ok()
            .flatten()
            .is_some())
    }

    async fn raw_material_assignments_for_order_apparatus(
        &self,
        order_id: &str,
        apparatus: &ApparatusId,
    ) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
        Ok(self
            .store
            .raw_material_assignments_for_order(order_id)
            .await?
            .into_iter()
            .filter(|assignment| {
                assignment.order_id.trim() == order_id.trim()
                    && assignment.apparatus_id.as_str() == apparatus.as_str()
            })
            .collect())
    }

    async fn material_rule_for_apparatus(
        &self,
        apparatus: &ApparatusId,
    ) -> Result<Option<ApparatusMaterialRule>, ProductionMapError> {
        let canonical = self.validated_material_apparatus(apparatus).await?;
        Ok(live_material_rule(&canonical))
    }

    async fn validated_material_apparatus(
        &self,
        apparatus: &ApparatusId,
    ) -> Result<std::sync::Arc<RuntimeApparatusConfiguration>, ProductionMapError> {
        let canonical = self.resolve_canonical_apparatus(apparatus).await?;
        if canonical.runtime.apparatus_id != *apparatus || !canonical.has_coherent_source() {
            return Err(ProductionMapError::StoreFailed);
        }
        Ok(canonical)
    }
}


fn material_rule_record(canonical: &RuntimeApparatusConfiguration) -> ApparatusMaterialRule {
    let (requires_material, start_policy, item_groups, requirement_groups) =
        match &canonical.material.policy {
            MaterialExecutionPolicy::NotRequired { item_group_ids } => (
                false,
                RawMaterialStartPolicy::StateAll,
                item_group_ids.clone(),
                Vec::new(),
            ),
            MaterialExecutionPolicy::AllRequired { item_group_ids } => (
                true,
                RawMaterialStartPolicy::StateAll,
                item_group_ids.clone(),
                Vec::new(),
            ),
            MaterialExecutionPolicy::RequirementSets { sets } => (
                true,
                RawMaterialStartPolicy::RequirementGroups,
                Vec::new(),
                sets.iter()
                    .map(|set| ApparatusMaterialRequirementGroup {
                        name: set.requirement_id.clone(),
                        item_groups: set.item_group_ids.clone(),
                        min_required_count: set.minimum_required_count,
                    })
                    .collect(),
            ),
        };
    ApparatusMaterialRule {
        apparatus_id: canonical.runtime.apparatus_id.clone(),
        apparatus: canonical.runtime.display.display_name.clone(),
        requires_material,
        start_policy,
        item_groups,
        requirement_groups,
    }
}

pub(crate) fn live_material_rule(
    canonical: &RuntimeApparatusConfiguration,
) -> Option<ApparatusMaterialRule> {
    match &canonical.material.policy {
        MaterialExecutionPolicy::NotRequired { item_group_ids } if item_group_ids.is_empty() => {
            None
        }
        _ => Some(material_rule_record(canonical)),
    }
}

pub(super) fn build_raw_material_start_requirements(
    rule: Option<&ApparatusMaterialRule>,
    assignments: &[RawMaterialAssignment],
    state_material_barcodes: &[String],
    scanned: &BTreeSet<String>,
) -> RawMaterialStartRequirements {
    let assignment_refs = assignments.iter().collect::<Vec<_>>();
    build_raw_material_start_requirements_refs(
        rule,
        &assignment_refs,
        state_material_barcodes,
        scanned,
    )
}
