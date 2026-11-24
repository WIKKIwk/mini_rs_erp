impl ProductionMapService {
    async fn build_production_snapshot_in_scope(
        &self,
        scope: Option<&WorkerSnapshotScope>,
        previous: Option<&ProductionMapLiveSnapshot>,
    ) -> Result<ProductionMapLiveSnapshot, ProductionMapError> {
        let (
            raw_maps,
            (stored_sequences, sequence_revisions),
            mut queue_states,
            order_controls,
            output_scope,
        ) = if let Some(scope) = scope {
            let inputs = self.worker_snapshot_inputs(scope).await?;
            (
                inputs.maps,
                inputs.sequences,
                inputs.queue_states,
                inputs.order_controls,
                Some((inputs.order_ids, inputs.apparatus_ids)),
            )
        } else {
            let (maps, sequences, states, controls) = tokio::try_join!(
                self.store.maps(),
                self.store.apparatus_sequences_with_revisions(),
                self.store.apparatus_queue_states(),
                self.store.order_control_states(),
            )?;
            (maps, sequences, states, controls, None)
        };
        let stage_order_controls = self.store.stage_order_control_states().await?;
        let canonical_apparatuses = self.snapshot_canonical_apparatuses().await;

        // One immutable topology cache serves every phase of this rebuild.
        // Queue visibility, sequence versions, controls and presentation must
        // not independently traverse the same dependency maps.
        let projections = raw_maps
            .iter()
            .map(|map| self.projection_map(map))
            .collect::<Vec<_>>();
        let mut projections_by_order = BTreeMap::new();
        for projection in &projections {
            projections_by_order.entry(projection.map().id.as_str()).or_insert(projection);
        }
        let unfiltered_visible = apparatus::visible_order_ids_by_projection(&projections);
        let mut visible_order_ids = unfiltered_visible.clone();
        apparatus::filter_unselected_print_orders(
            &raw_maps,
            &canonical_apparatuses,
            &mut visible_order_ids,
        );
        let frozen_order_ids = order_controls
            .iter()
            .filter_map(|(id, control)| {
                (control.state == OrderControlState::Frozen).then_some(id.clone())
            })
            .collect::<BTreeSet<_>>();
        let mut sequences = Self::effective_apparatus_sequences_for_visible(
            &unfiltered_visible,
            &stored_sequences,
            &frozen_order_ids,
        );
        let holds = self.store.active_print_preflight_holds().await?;
        let sequence_versions = {
            let mut sequence_versions = BTreeMap::new();
            for canonical in &canonical_apparatuses {
                let id = canonical.runtime.apparatus_id.as_str();
                if scope.is_some_and(|scope| {
                    if scope.only_orders {
                        !output_scope
                            .as_ref()
                            .is_some_and(|(_, apparatus)| apparatus.contains(id))
                    } else {
                        !scope.apparatus.iter().any(|assigned| assigned == id)
                    }
                }) {
                    continue;
                }
                let preflight = holds
                    .iter()
                    .filter(|h| h.apparatus == id && h.is_live_at(0))
                    .map(|h| h.order_id.clone())
                    .collect();
                let state = SequenceMoveState::from_projection(
                    canonical,
                    &projections,
                    stored_sequences
                        .get(id)
                        .map(Vec::as_slice)
                        .unwrap_or_default(),
                    queue_states.get(id).unwrap_or(&BTreeMap::new()),
                    &frozen_order_ids,
                    &preflight,
                );
                sequence_versions.insert(id.to_string(), state.version());
            }
            sequence_versions
        };
        apparatus::filter_unselected_print_orders(
            &raw_maps,
            &canonical_apparatuses,
            &mut sequences,
        );
        let queue_policies = canonical_apparatuses
            .iter()
            .map(|canonical| effective_apparatus_queue_policy_record(canonical))
            .collect::<Vec<_>>();
        let order_ids = raw_maps
            .iter()
            .filter(|map| {
                output_scope
                    .as_ref()
                    .is_none_or(|(ids, _)| ids.contains(&map.id))
            })
            .map(|map| map.id.trim().to_string())
            .filter(|order_id| !order_id.is_empty())
            .collect::<Vec<_>>();
        let (queue_action_controls, queue_logs_by_order, lifecycles) = tokio::try_join!(
            async {
                if let Some((ids, apparatus_ids)) = output_scope.as_ref() {
                    self.queue_action_controls_for_snapshot_orders(
                        &raw_maps,
                        &stored_sequences,
                        &queue_states,
                        &order_controls,
                        &canonical_apparatuses,
                        holds,
                        ids,
                        apparatus_ids,
                        &projections,
                    )
                    .await
                } else {
                    self.queue_action_controls_for_snapshot(
                        &raw_maps,
                        &stored_sequences,
                        &queue_states,
                        &order_controls,
                        &canonical_apparatuses,
                        holds,
                        &projections,
                    )
                    .await
                }
            },
            self.store.queue_snapshot_logs_for_orders(&order_ids),
            self.store.production_order_lifecycles(&order_ids),
        )?;

        let output_maps = output_scope.as_ref().map(|(ids, _)| {
            raw_maps
                .iter()
                .filter(|map| ids.contains(&map.id))
                .cloned()
                .collect::<Vec<_>>()
        });
        let mut stage_states = stage_states_for_snapshot(
            &projections,
            output_scope.as_ref().map(|(ids, _)| ids),
            &queue_action_controls,
            &queue_logs_by_order,
            &queue_states,
        );
        for stage in &stage_order_controls {
            if stage.state == OrderControlState::Frozen {
                stage_states.entry(stage.order_id.clone()).or_default()
                    .insert(stage.stage_node_id.clone(), "frozen".to_string());
            }
        }
        for (apparatus, controls) in &queue_action_controls {
            let states = queue_states.entry(apparatus.clone()).or_default();
            for (order_id, control) in controls {
                states.insert(order_id.clone(), control.state.as_str().to_string());
                if control.state == queue_state::ApparatusQueueOrderState::Frozen {
                    if let Some(sequence) = sequences.get_mut(apparatus) {
                        sequence.retain(|id| id != order_id);
                    }
                }
            }
        }
        let order_statuses =
            Self::order_status_details_from_lifecycles(&order_ids, &lifecycles, &order_controls)?;
        for (apparatus, orders) in &mut visible_order_ids {
            orders.retain(|order_id| {
                if order_statuses.get(order_id).is_some_and(|detail| {
                    matches!(
                        detail.lifecycle_status,
                        ProductionOrderLifecycleStatus::ProductionCompleted
                            | ProductionOrderLifecycleStatus::Closed
                            | ProductionOrderLifecycleStatus::Cancelled
                    )
                }) {
                    return false;
                }
                let Some(projection) = projections_by_order.get(order_id.as_str()) else {
                    return false;
                };
                let map = projection.map();
                let occurrences = projection.stages()
                    .into_iter()
                    .filter(|stage| stage.apparatus_id.as_deref() == Some(apparatus.as_str()))
                    .collect::<Vec<_>>();
                let own_control = queue_action_controls
                    .get(apparatus)
                    .and_then(|controls| controls.get(order_id));
                occurrences.is_empty()
                    || !occurrences.iter().all(|stage| {
                        stage_states
                            .get(order_id)
                            .and_then(|states| states.get(&stage.node_id))
                            .is_some_and(|state| state == "completed")
                            || own_control.is_some_and(|control| {
                                chain::stage_node_ids_match_for_map(
                                    map,
                                    &stage.node_id,
                                    &control.stage_node_id,
                                ) && control
                                    .stage_work
                                    .as_ref()
                                    .is_some_and(|work| work.local_completed)
                            })
                    })
            });
        }
        let mut frozen_orders_by_apparatus =
            Self::frozen_orders_by_apparatus(&order_controls, &queue_logs_by_order);
        for stage in &stage_order_controls {
            if stage.state == OrderControlState::Frozen {
                let records = BTreeMap::from([(stage.order_id.clone(), stage.clone())]);
                for (apparatus, frozen) in Self::frozen_orders_by_apparatus(&records, &queue_logs_by_order) {
                    let target = frozen_orders_by_apparatus.entry(apparatus).or_default();
                    for record in frozen {
                        if !target.iter().any(|c| c.order_id == record.order_id) { target.push(record); }
                    }
                }
            }
        }
        drop(projections_by_order);
        drop(projections);
        let maps = compile_saved_maps_reusing_programs(
            output_maps.unwrap_or(raw_maps),
            previous.map(|snapshot| snapshot.maps.as_slice()),
        );
        let mut snapshot = ProductionMapLiveSnapshot {
            sequence_versions,
            sequence_revisions,
            maps,
            sequences,
            visible_order_ids,
            sequence_positions_by_apparatus: BTreeMap::new(),
            queue_states,
            stage_states,
            queue_policies,
            queue_action_controls,
            order_statuses,
            order_controls,
            stage_order_controls,
            frozen_orders_by_apparatus,
        };
        if let Some((ids, apparatus_ids)) = output_scope {
            // Keep cross-machine route state for selected orders, and omit
            // unrelated queues before putting the derived snapshot in memory.
            for values in snapshot.queue_states.values_mut() {
                values.retain(|order_id, _| ids.contains(order_id));
            }
            snapshot.order_controls.retain(|id, _| ids.contains(id));
            snapshot.stage_order_controls.retain(|c| ids.contains(&c.order_id));
            for values in snapshot.frozen_orders_by_apparatus.values_mut() {
                values.retain(|order| ids.contains(&order.order_id));
            }
            if let Some(scope) = scope {
                let own = |id: &String| {
                    if scope.only_orders {
                        apparatus_ids.contains(id)
                    } else {
                        scope.apparatus.contains(id)
                    }
                };
                snapshot.sequences.retain(|id, _| own(id));
                snapshot.visible_order_ids.retain(|id, _| own(id));
                snapshot.sequence_versions.retain(|id, _| own(id));
                snapshot.sequence_revisions.retain(|id, _| own(id));
                snapshot.frozen_orders_by_apparatus.retain(|id, _| own(id));
            }
        }
        snapshot.sort_free_pick_visible_orders();
        Ok(snapshot)
    }

    fn frozen_orders_by_apparatus(
        order_controls: &BTreeMap<String, OrderControlRecord>,
        logs_by_order: &BTreeMap<String, Vec<ProductionOrderLogEntry>>,
    ) -> BTreeMap<String, Vec<FrozenOrderSnapshot>> {
        let mut result = BTreeMap::new();
        for (order_id, control) in order_controls.iter().filter(|(_, control)| {
            control.state == OrderControlState::Frozen && control.early_close.is_none()
        }) {
            let freeze_log = logs_by_order
                .get(order_id)
                .into_iter()
                .flat_map(|logs| logs.iter())
                .rev()
                .find(|log| {
                    log.action == queue_state::ApparatusQueueAction::Freeze
                        && log.to_state == queue_state::ApparatusQueueOrderState::Frozen
                        && (control.stage_node_id.is_empty() || log.stage_node_id == control.stage_node_id)
                });
            let apparatus = control
                .freeze_request
                .as_ref()
                .map(|request| request.target_apparatus.trim())
                .filter(|apparatus| !apparatus.is_empty())
                .or_else(|| freeze_log.map(|log| log.apparatus.trim()))
                .unwrap_or_default();
            if apparatus.is_empty() {
                continue;
            }
            let frozen_at_unix = control
                .frozen_at_unix
                .or_else(|| freeze_log.map(|log| log.created_at_unix))
                .unwrap_or_default();
            let frozen_by = if control.actor.display_name.trim().is_empty() {
                control.actor.ref_.trim().to_string()
            } else {
                control.actor.display_name.trim().to_string()
            };
            result
                .entry(apparatus.to_string())
                .or_insert_with(Vec::new)
                .push(FrozenOrderSnapshot {
                    order_id: order_id.clone(),
                    apparatus: apparatus.to_string(),
                    issue_note: freeze_log
                        .map(|log| log.issue_note.trim().to_string())
                        .unwrap_or_default(),
                    frozen_at_unix,
                    frozen_by,
                });
        }
        result
    }
}
