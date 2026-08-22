impl ProductionMapService {

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
        let apparatus_id = parse_apparatus_id(apparatus)?;
        let canonical = self.validated_material_apparatus(&apparatus_id).await?;
        let rule = live_material_rule(&canonical);
        let scanned = normalized_barcodes(material_barcode);
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
            material_barcode,
        );
        if assignments.is_empty() {
            if !scanned.is_empty() {
                return Err(ProductionMapError::RawMaterialMismatch);
            }
            if !super::apparatus::is_laminatsiya_apparatus(&canonical)
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
        if !super::apparatus::is_laminatsiya_apparatus(&canonical)
            || (progress.qr_payload.trim().is_empty()
                && progress.progress_batch_id.trim().is_empty())
        {
            return Ok(false);
        }
        let Some(order_map) = self.raw_map(order_id).await? else {
            return Ok(false);
        };
        Ok(self
            .previous_stage_start_progress_batch(order_id, &order_map, apparatus.as_str(), progress)
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
            .raw_material_assignments()
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
