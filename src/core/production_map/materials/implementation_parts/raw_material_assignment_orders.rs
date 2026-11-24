impl ProductionMapService {
    pub async fn raw_material_assignment_orders(
        &self,
    ) -> Result<Vec<ProductionMapSaved>, ProductionMapError> {
        let order_controls = self.order_control_states().await?;
        let maps = self.store.raw_material_assignment_maps().await?;
        Ok(compile_saved_maps(maps.into_iter().filter(|map| {
            let order_id = map.id.trim();
            order_id.starts_with("zakaz-")
                && order_controls
                    .get(order_id)
                    .is_none_or(|control| control.state == OrderControlState::Active)
        })))
    }

    pub async fn apparatus_material_rules(
        &self,
    ) -> Result<Vec<ApparatusMaterialRule>, ProductionMapError> {
        let mut apparatus_ids = BTreeSet::new();
        for map in self.store.maps().await? {
            for stage in chain::linear_work_stages(&map) {
                let Some(apparatus) = stage.apparatus_id.as_deref() else {
                    continue;
                };
                let apparatus_id =
                    parse_apparatus_id(apparatus).map_err(|_| ProductionMapError::StoreFailed)?;
                apparatus_ids.insert(apparatus_id);
            }
        }

        let mut rules = Vec::new();
        for apparatus_id in apparatus_ids {
            let canonical = self.validated_material_apparatus(&apparatus_id).await?;
            if let Some(rule) = live_material_rule(&canonical) {
                rules.push(rule);
            }
        }
        Ok(rules)
    }

    pub async fn raw_material_assignments(
        &self,
    ) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
        self.store.raw_material_assignments().await
    }

    pub async fn raw_material_assignment_by_barcode(
        &self,
        barcode: &str,
    ) -> Result<Option<RawMaterialAssignment>, ProductionMapError> {
        self.store
            .raw_material_assignment_by_barcode(barcode.trim())
            .await
    }

    pub async fn raw_material_assignments_for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
        self.store.raw_material_assignments_for_order(order_id.trim()).await
    }

    pub async fn raw_material_intake_is_available(
        &self,
        order_id: &str,
        apparatus: &str,
    ) -> Result<bool, ProductionMapError> {
        let apparatus_id = parse_apparatus_id(apparatus)?;
        self.validated_material_apparatus(&apparatus_id).await?;
        let queue_states = self.store.apparatus_queue_states().await?;
        let intake_allowed = queue_states.iter().any(|(candidate, states)| {
            canonical_apparatuses_match(candidate, &apparatus_id)
                && states
                    .get(order_id.trim())
                    .and_then(|state| queue_state::ApparatusQueueOrderState::parse(state))
                    .is_some_and(queue_state::ApparatusQueueOrderState::allows_material_intake)
        });
        if !intake_allowed {
            return Ok(false);
        }
        Ok(self.effective_order_control_for_apparatus(order_id.trim(), apparatus_id.as_str()).await?.state == OrderControlState::Active)
    }

    pub async fn raw_material_matches_apparatus_rule(
        &self,
        apparatus: &str,
        item_group: &str,
        item_group_path: Vec<String>,
    ) -> Result<bool, ProductionMapError> {
        let apparatus_id = parse_apparatus_id(apparatus)?;
        let path = normalize_group_path(item_group, item_group_path);
        Ok(self
            .material_rule_for_apparatus(&apparatus_id)
            .await?
            .is_none_or(|rule| rule_matches(&rule, &apparatus_id, &path)))
    }

    pub async fn raw_material_start_requirements(
        &self,
        apparatus: &str,
        order_id: &str,
        state_material_barcodes: &[String],
        material_barcodes: &str,
    ) -> Result<RawMaterialStartRequirements, ProductionMapError> {
        self.raw_material_start_requirements_for_stock(
            apparatus, order_id, state_material_barcodes, material_barcodes, None,
        ).await
    }

    pub async fn raw_material_start_requirements_for_stock(
        &self,
        apparatus: &str,
        order_id: &str,
        state_material_barcodes: &[String],
        material_barcodes: &str,
        usable_barcodes: Option<&[String]>,
    ) -> Result<RawMaterialStartRequirements, ProductionMapError> {
        let apparatus_id = parse_apparatus_id(apparatus)?;
        let mut assignments = self
            .raw_material_assignments_for_order_apparatus(order_id, &apparatus_id)
            .await?;
        let rule = self.material_rule_for_apparatus(&apparatus_id).await?;
        let has_commitment = rule.is_some() && !assignments.is_empty();
        let committed_barcodes = assignments.iter().map(|assignment| normalize_barcode(&assignment.barcode)).collect::<BTreeSet<_>>().into_iter().collect();
        if let Some(usable) = usable_barcodes {
            assignments.retain(|assignment| usable.iter().any(|barcode|
                barcode.trim().eq_ignore_ascii_case(assignment.barcode.trim())));
        }
        let assignments_for_policy: &[RawMaterialAssignment] =
            if rule.is_some() { &assignments } else { &[] };
        let material_barcodes = parse_material_barcodes(std::iter::once(material_barcodes));
        let mut requirements = build_raw_material_start_requirements(
            rule.as_ref(),
            assignments_for_policy,
            state_material_barcodes,
            &normalized_barcodes_list(&material_barcodes),
        );
        // Pending preparation stock is still attached, but is never a scan
        // candidate and must not turn an optional-material policy into a bypass.
        if has_commitment && assignments.is_empty() {
            requirements.material_scan_required = true;
            requirements.assignments_satisfied = false;
            requirements.scan_satisfied = false;
        }
        if has_commitment { requirements.assigned_barcodes = committed_barcodes; }
        Ok(requirements)
    }

    pub async fn unlink_raw_material_assignment(
        &self,
        input: RawMaterialAssignmentDeleteInput,
    ) -> Result<RawMaterialAssignment, ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        self.unlink_raw_material_assignment_under_queue_guard(input)
            .await
    }

    pub(crate) async fn unlink_raw_material_assignment_under_queue_guard(
        &self,
        input: RawMaterialAssignmentDeleteInput,
    ) -> Result<RawMaterialAssignment, ProductionMapError> {
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

    pub async fn raw_material_assignment_apparatus_options(
        &self,
        order_id: &str,
        item_group_path: &[String],
    ) -> Result<Vec<String>, ProductionMapError> {
        let order_id = order_id.trim();
        if order_id.is_empty() || item_group_path.is_empty() {
            return Err(ProductionMapError::RawMaterialInvalidInput);
        }
        let Some(map) = self.raw_map(order_id).await? else {
            return Err(ProductionMapError::MapNotFound);
        };
        self.raw_material_assignment_apparatus_options_for_map(&map, item_group_path)
            .await
    }

    /// Resolve a candidate from the same definition already loaded for this
    /// request, rather than performing a point read for every candidate order.
    pub async fn raw_material_assignment_apparatus_options_for_map(
        &self,
        map: &super::ProductionMapDefinition,
        item_group_path: &[String],
    ) -> Result<Vec<String>, ProductionMapError> {
        if map.id.trim().is_empty() || item_group_path.is_empty() {
            return Err(ProductionMapError::RawMaterialInvalidInput);
        }
        self.resolve_material_apparatus_options(map, item_group_path)
            .await
    }

    pub async fn receive_raw_material_for_active_order(
        &self,
        input: RawMaterialAssignmentInput,
        assigned_apparatus: &[String],
        actor: &QueueActionActor,
    ) -> Result<(RawMaterialAssignment, Vec<String>), ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        let normalized_barcode = normalize_barcode(&input.barcode);
        let requested_apparatus = parse_optional_apparatus_id(&input.apparatus)?;
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
                    && requested_apparatus
                        .as_ref()
                        .is_none_or(|id| id == &assignment.apparatus_id) =>
            {
                assignment
            }
            Some(_) => return Err(ProductionMapError::RawMaterialAlreadyAssigned),
            None => return Err(ProductionMapError::RawMaterialAssignmentNotFound),
        };
        if !assigned_apparatus_contains(&assignment.apparatus_id, assigned_apparatus) {
            return Err(ProductionMapError::ApparatusNotAssigned);
        }
        let queue_states = self.store.apparatus_queue_states().await?;
        let intake_allowed = queue_states.iter().any(|(apparatus, states)| {
            canonical_apparatuses_match(apparatus, &assignment.apparatus_id)
                && states
                    .get(&assignment.order_id)
                    .and_then(|state| queue_state::ApparatusQueueOrderState::parse(state))
                    .is_some_and(queue_state::ApparatusQueueOrderState::allows_material_intake)
        });
        if !intake_allowed {
            return Err(ProductionMapError::RawMaterialOrderNotActive);
        }
        {
            let control = self.effective_order_control_for_apparatus(&assignment.order_id, assignment.apparatus_id.as_str()).await?;
            match control.state {
                OrderControlState::Active => {}
                OrderControlState::FreezeRequested => {
                    return Err(ProductionMapError::OrderFreezeRequested);
                }
                OrderControlState::Frozen => return Err(ProductionMapError::OrderFrozen),
            }
        }
        let material_rule = self
            .material_rule_for_apparatus(&assignment.apparatus_id)
            .await?;
        if material_rule
            .as_ref()
            .is_none_or(|rule| !rule_matches(rule, &assignment.apparatus_id, &item_group_path))
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

    pub(crate) async fn prepare_raw_material_assignment(
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
        let apparatus_options = self
            .raw_material_assignment_apparatus_options(&order_id, &item_group_path)
            .await?;
        let requested_apparatus = parse_optional_apparatus_id(&input.apparatus)?;
        let apparatus_id = if let Some(requested_apparatus) = requested_apparatus {
            apparatus_options
                .iter()
                .find_map(|candidate| {
                    parse_apparatus_id(candidate)
                        .ok()
                        .filter(|candidate_id| candidate_id == &requested_apparatus)
                })
                .ok_or(ProductionMapError::RawMaterialGroupNotAllowed)?
        } else {
            match apparatus_options.len() {
                0 => return Err(ProductionMapError::RawMaterialGroupNotAllowed),
                1 => parse_apparatus_id(&apparatus_options[0])?,
                _ => {
                    return Err(ProductionMapError::RawMaterialGroupAmbiguous(
                        apparatus_options,
                    ));
                }
            }
        };
        for existing in self.store.raw_material_assignments().await? {
            if same_barcode(&existing.barcode, &barcode) {
                if existing.order_id.trim() == order_id && existing.apparatus_id == apparatus_id {
                    return Err(ProductionMapError::RawMaterialAlreadyAssignedToOrder);
                }
                return Err(ProductionMapError::RawMaterialAlreadyAssigned);
            }
        }
        Ok(RawMaterialAssignment {
            order_id,
            apparatus: apparatus_id.to_string(),
            apparatus_id,
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

    #[allow(clippy::too_many_arguments)]
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
        let apparatus_id = parse_apparatus_id(apparatus)?;
        if !assigned_apparatus_contains(&apparatus_id, assigned_apparatus) {
            return Err(ProductionMapError::ApparatusNotAssigned);
        }
        let material_barcodes = parse_material_barcodes(std::iter::once(material_barcode));
        self.validate_material_scan(
            apparatus,
            order_id,
            action,
            &material_barcodes,
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
        let _guard = self.queue_progress_action_guard(
            request.apparatus, request.action, &request.progress,
            request.material_barcodes,
            request.qolip_validation.as_ref().map(|validation| validation.qolip_codes.as_slice()).unwrap_or_default(),
        ).await?;
        let material_barcodes = parse_material_barcodes(
            request.material_barcodes.iter().map(String::as_str),
        );
        let prepared = self
            .prepare_apparatus_queue_action_with_material_scan_and_progress(MaterialScanProgressAction {
                material_barcodes: &material_barcodes,
                ..request
            })
            .await?;
        self.commit_prepared_queue_action(prepared).await
    }
}
