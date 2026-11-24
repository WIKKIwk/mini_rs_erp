impl ProductionMapService {
    pub async fn queue_action_controls_for_apparatus(
        &self,
        apparatus: &str,
    ) -> Result<
        BTreeMap<String, BTreeMap<String, ApparatusQueueOrderActionControl>>,
        ProductionMapError,
    > {
        if !queue_state::is_canonical_apparatus_id(apparatus) {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        self.queue_action_controls_in_scope(Some(apparatus)).await
    }

    async fn queue_action_controls_in_scope(
        &self,
        only_apparatus: Option<&str>,
    ) -> Result<
        BTreeMap<String, BTreeMap<String, ApparatusQueueOrderActionControl>>,
        ProductionMapError,
    > {
        let (maps, sequences, all_states, order_controls, canonical_apparatuses) = tokio::join!(
            self.store.maps(),
            self.store.apparatus_sequences(),
            self.store.apparatus_queue_states(),
            self.order_control_states(),
            self.active_canonical_apparatuses(),
        );
        let maps = maps?;
        let sequences = sequences?;
        let all_states = all_states?;
        let order_controls = order_controls?;
        let canonical_apparatuses = canonical_apparatuses?;
        self.queue_action_controls_for_snapshot_scope(
            &maps,
            &sequences,
            &all_states,
            &order_controls,
            &canonical_apparatuses,
            only_apparatus,
            None,
            None,
            None,
            None,
        )
        .await
    }
}

impl ProductionMapService {
    pub(in crate::core::production_map) async fn queue_action_controls_for_snapshot(
        &self,
        maps: &[ProductionMapDefinition],
        sequences: &BTreeMap<String, Vec<String>>,
        all_states: &ApparatusQueueStateMap,
        order_controls: &OrderControlMap,
        canonical_apparatuses: &[std::sync::Arc<
            crate::core::apparatus_standard::RuntimeApparatusConfiguration,
        >],
        active_print_preflight_holds: Vec<PrintPreflightHold>,
        projections: &[chain::ProjectionMap<'_>],
    ) -> Result<
        BTreeMap<String, BTreeMap<String, ApparatusQueueOrderActionControl>>,
        ProductionMapError,
    > {
        self.queue_action_controls_for_snapshot_scope(
            maps,
            sequences,
            all_states,
            order_controls,
            canonical_apparatuses,
            None,
            Some(active_print_preflight_holds),
            None,
            None,
            Some(projections),
        )
        .await
    }

    // Keep rebuild-local holds separate from the independently scoped inputs.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::core::production_map) async fn queue_action_controls_for_snapshot_orders(
        &self,
        maps: &[ProductionMapDefinition],
        sequences: &BTreeMap<String, Vec<String>>,
        all_states: &ApparatusQueueStateMap,
        order_controls: &OrderControlMap,
        canonical_apparatuses: &[std::sync::Arc<
            crate::core::apparatus_standard::RuntimeApparatusConfiguration,
        >],
        holds: Vec<PrintPreflightHold>,
        order_ids: &BTreeSet<String>,
        apparatus_ids: &BTreeSet<String>,
        projections: &[chain::ProjectionMap<'_>],
    ) -> Result<
        BTreeMap<String, BTreeMap<String, ApparatusQueueOrderActionControl>>,
        ProductionMapError,
    > {
        self.queue_action_controls_for_snapshot_scope(
            maps,
            sequences,
            all_states,
            order_controls,
            canonical_apparatuses,
            None,
            Some(holds),
            Some(order_ids),
            Some(apparatus_ids),
            Some(projections),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn queue_action_controls_for_snapshot_scope(
        &self,
        maps: &[ProductionMapDefinition],
        sequences: &BTreeMap<String, Vec<String>>,
        all_states: &ApparatusQueueStateMap,
        order_controls: &OrderControlMap,
        canonical_apparatuses: &[std::sync::Arc<
            crate::core::apparatus_standard::RuntimeApparatusConfiguration,
        >],
        only_apparatus: Option<&str>,
        snapshot_print_preflight_holds: Option<Vec<PrintPreflightHold>>,
        output_order_ids: Option<&BTreeSet<String>>,
        output_apparatus_ids: Option<&BTreeSet<String>>,
        projections: Option<&[chain::ProjectionMap<'_>]>,
    ) -> Result<
        BTreeMap<String, BTreeMap<String, ApparatusQueueOrderActionControl>>,
        ProductionMapError,
    > {
        let stage_controls = self.store.stage_order_control_states().await?;
        let qolip_start_settings = self.store.qolip_start_settings().await?;
        let owned_projections = projections.is_none().then(|| {
            maps.iter()
                .map(|map| self.projection_map(map))
                .collect::<Vec<_>>()
        });
        let projections = projections.unwrap_or_else(|| {
            owned_projections.as_deref().expect("standalone projections are initialized")
        });
        let scoped_order_ids = only_apparatus.map(|apparatus| {
            queue_order_ids_by_projection(projections)
                .remove(apparatus)
                .unwrap_or_default()
                .into_iter()
                .collect::<BTreeSet<_>>()
        });
        let order_ids = maps
            .iter()
            .map(|map| map.id.trim())
            .filter(|order_id| {
                !order_id.is_empty()
                    && scoped_order_ids
                        .as_ref()
                        .is_none_or(|orders| orders.contains(*order_id))
            })
            .map(str::to_string)
            .collect::<Vec<_>>();
        let opening_wip_query = OpeningWipQuery {
            order_id: String::new(),
            wip_status: None,
            limit: 100_000,
        };
        let (
            material_assignments,
            active_sessions_by_order,
            progress_batches_by_order,
            opening_wip_records,
            active_print_preflight_holds,
        ) = tokio::join!(
            async {
                if output_order_ids.is_some() || scoped_order_ids.is_some() {
                    self.store
                        .raw_material_assignments_for_orders(&order_ids)
                        .await
                } else {
                    self.store.raw_material_assignments().await
                }
            },
            self.store.order_run_sessions_for_orders(&order_ids),
            self.store.progress_batches_for_orders(&order_ids),
            async {
                if output_order_ids.is_some() || scoped_order_ids.is_some() {
                    self.store.opening_wip_records_for_orders(&order_ids).await
                } else {
                    self.store.opening_wip_records(opening_wip_query).await
                }
            },
            async {
                // Reuse only the holds loaded by this canonical snapshot rebuild.
                // Standalone/scoped controls retain a fresh, concurrent store read.
                match snapshot_print_preflight_holds {
                    Some(holds) => Ok(holds),
                    None => self.store.active_print_preflight_holds().await,
                }
            },
        );
        let material_assignments = material_assignments?;
        let active_sessions_by_order = active_sessions_by_order?;
        let progress_batches_by_order = progress_batches_by_order?;
        let opening_wip_records = opening_wip_records?;
        let projections_by_order_id = projections_by_order_id(projections);
        let now = super::super::progress::unix_seconds();
        let print_preflight_by_apparatus = active_print_preflight_holds?
            .into_iter()
            .filter(|hold| hold.is_live_at(now))
            .map(|hold| (hold.apparatus.trim().to_string(), hold))
            .collect::<HashMap<_, _>>();
        let mut opening_wip_by_order = HashMap::<String, Vec<OpeningWipRecord>>::new();
        for record in opening_wip_records {
            if scoped_order_ids
                .as_ref()
                .is_some_and(|orders| !orders.contains(record.intake.order_id.trim()))
            {
                continue;
            }
            opening_wip_by_order
                .entry(record.intake.order_id.trim().to_string())
                .or_default()
                .push(record);
        }
        let stage_work_by_order = projections
            .iter()
            .filter(|projection| {
                let map = projection.map();
                scoped_order_ids
                    .as_ref()
                    .is_none_or(|orders| orders.contains(map.id.trim()))
                    && output_order_ids.is_none_or(|orders| orders.contains(map.id.trim()))
            })
            .map(|projection| {
                let map = projection.map();
                let sessions = active_sessions_by_order
                    .get(&map.id)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let batches = progress_batches_by_order
                    .get(&map.id)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let opening = opening_wip_by_order
                    .get(&map.id)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                (
                    map.id.clone(),
                    super::super::stage_execution::stage_work_statuses_projected(
                        projection,
                        sessions,
                        &super::super::stage_execution::work_inputs_projected(
                            projection, batches, opening,
                        ),
                        all_states,
                        &[],
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut active_sessions_by_order_apparatus =
            HashMap::<(&str, &str), &OrderRunSession>::new();
        for sessions in active_sessions_by_order.values() {
            for session in sessions {
                if !session.status.is_open() {
                    continue;
                }
                if !queue_state::is_canonical_apparatus_id(&session.apparatus) {
                    continue;
                }
                active_sessions_by_order_apparatus
                    .entry((session.order_id.trim(), session.apparatus.trim()))
                    .or_insert(session);
            }
        }

        let mut material_assignments_by_order =
            HashMap::<String, Vec<RawMaterialAssignment>>::new();
        for assignment in material_assignments {
            if scoped_order_ids
                .as_ref()
                .is_some_and(|orders| !orders.contains(assignment.order_id.trim()))
            {
                continue;
            }
            material_assignments_by_order
                .entry(assignment.order_id.trim().to_string())
                .or_default()
                .push(assignment);
        }
        let mut queue_orders_by_apparatus = queue_order_ids_by_projection(projections);
        apparatus::filter_unselected_print_orders(
            maps,
            canonical_apparatuses,
            &mut queue_orders_by_apparatus,
        );
        let frozen_order_ids = order_controls
            .iter()
            .filter_map(|(order_id, control)| {
                (control.state == OrderControlState::Frozen).then_some(order_id.clone())
            })
            .collect::<BTreeSet<_>>();
        let canonical_by_id = canonical_apparatuses
            .iter()
            .map(|configuration| {
                (
                    configuration.runtime.apparatus_id.as_str(),
                    configuration.as_ref(),
                )
            })
            .collect::<HashMap<_, _>>();
        let known_keys = sequences
            .keys()
            .map(String::as_str)
            .chain(all_states.keys().map(String::as_str))
            .chain(
                canonical_apparatuses
                    .iter()
                    .map(|configuration| configuration.runtime.apparatus_id.as_str()),
            )
            .chain(queue_orders_by_apparatus.keys().map(String::as_str))
            .filter(|key| queue_state::is_canonical_apparatus_id(key))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let mut result = BTreeMap::new();

        for apparatus in &known_keys {
            let storage_key = apparatus.trim().to_string();
            if only_apparatus.is_some_and(|expected| expected != storage_key) {
                continue;
            }
            if output_apparatus_ids.is_some_and(|ids| !ids.contains(&storage_key)) {
                continue;
            }
            // Snapshot reads must stay fail-soft: one map referencing a deleted or
            // deactivated apparatus must not take the whole live stream down for
            // every operator. Write paths keep their strict validation.
            let Some(canonical) = canonical_by_id.get(storage_key.as_str()).copied() else {
                warn_skipped_snapshot_apparatus(&storage_key);
                continue;
            };
            let is_rezka = apparatus::is_rezka_apparatus(canonical);
            let is_laminatsiya = apparatus::uses_lamination_workflow(canonical);
            let apparatus_preflight = print_preflight_by_apparatus.get(&storage_key);
            let stored_sequence = sequences
                .get(&storage_key)
                .or_else(|| sequences.get(apparatus))
                .map(Vec::as_slice)
                .unwrap_or_default();
            let visible_order_ids = queue_orders_by_apparatus
                .get(&storage_key)
                .or_else(|| queue_orders_by_apparatus.get(apparatus))
                .map(Vec::as_slice)
                .unwrap_or_default();
            let sequence = queue_state::effective_apparatus_sequence_excluding(
                stored_sequence,
                visible_order_ids,
                &frozen_order_ids,
            );
            // Frozen orders leave the executable sequence, but their visible
            // detail rows still need an authoritative, actionless contract.
            let control_order_ids =
                queue_state::effective_apparatus_sequence(stored_sequence, visible_order_ids);
            let mut order_inputs = Vec::with_capacity(control_order_ids.len());
            for order_id in &control_order_ids {
                let Some(projection) = projections_by_order_id.get(order_id.trim()).copied() else {
                    continue;
                };
                let batches = progress_batches_by_order
                    .get(order_id.trim())
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let opening_wip_records = opening_wip_by_order
                    .get(order_id.trim())
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let active_session = active_sessions_by_order_apparatus
                    .get(&(order_id.trim(), storage_key.as_str()))
                    .copied();
                order_inputs.push(queue_control_order_input(
                    order_id.trim(),
                    projection,
                    &storage_key,
                    batches,
                    opening_wip_records,
                    active_session,
                ));
            }
            let mut effective_states = all_states
                .get(&storage_key)
                .or_else(|| all_states.get(apparatus))
                .map(parsed_queue_states)
                .unwrap_or_default();
            for input in &order_inputs {
                if effective_states.get(input.order_id)
                    != Some(&queue_state::ApparatusQueueOrderState::Completed)
                {
                    continue;
                }
                if input.waiting_reentry_stage_node_id.is_some()
                    || input.opening_wip_stage_node_id.is_some()
                {
                    effective_states.insert(
                        input.order_id.to_string(),
                        queue_state::ApparatusQueueOrderState::Pending,
                    );
                }
            }
            let policy = effective_apparatus_queue_policy(canonical);
            let active_order_id = effective_states
                .iter()
                .find_map(|(order_id, state)| state.is_active().then_some(order_id.as_str()));
            let is_bosma = pechat::is_pechat_apparatus(canonical);
            let actionable_order_id =
                queue_state::first_actionable_order_id(&sequence, &effective_states);
            let mut apparatus_controls = BTreeMap::new();

            let station = StationQueueSnapshot {
                apparatus,
                storage_key: &storage_key,
                canonical,
                is_rezka,
                is_laminatsiya,
                is_bosma,
                policy,
                stage_controls: &stage_controls,
                order_controls,
                effective_states: &effective_states,
                all_states,
                stage_work_by_order: &stage_work_by_order,
                active_sessions_by_order: &active_sessions_by_order,
                material_assignments_by_order: &material_assignments_by_order,
                active_order_id,
                actionable_order_id,
                apparatus_preflight,
                qolip_start_settings: &qolip_start_settings,
            };
            for input in &order_inputs {
                if output_order_ids.is_some_and(|orders| !orders.contains(input.order_id)) {
                    continue;
                }
                if let Some(control) = self.snapshot_order_action_control(&station, input).await? {
                    apparatus_controls.insert(input.order_id.to_string(), control);
                }
            }
            if !apparatus_controls.is_empty() {
                result.insert(storage_key, apparatus_controls);
            }
        }
        Ok(result)
    }
}
