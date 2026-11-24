macro_rules! postgres_map_catalog {
    ($($following:tt)*) => {
        #[async_trait]
        impl ProductionMapStorePort for PostgresProductionMapStore {
            async fn set_order_print_side(
                &self, order_id: &str, assigned: &[String], standard: bool, previous: bool,
            ) -> Result<(), ProductionMapError> {
                let mut tx = self.pool.begin().await.map_err(|_| ProductionMapError::StoreFailed)?;
                lock_order_for_edit_tx(&mut tx, order_id).await?;
                let (map, calculation) = sqlx::query_as::<_, (serde_json::Value, Option<serde_json::Value>)>(
                    "SELECT m.map_json, o.calculation_json
             FROM mini_production_maps m LEFT JOIN mini_orders o ON o.id = m.id
             WHERE m.id = $1 FOR UPDATE OF m",
                )
                .bind(order_id)
                .fetch_optional(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?
                .ok_or(ProductionMapError::MapNotFound)?;
                let map: ProductionMapDefinition = serde_json::from_value(map)
                    .map_err(|_| ProductionMapError::StoreFailed)?;
                map.validate_print_side_update(assigned, standard, previous)?;
                sqlx::query("UPDATE mini_production_maps
            SET map_json = jsonb_set(map_json, '{standard_print_side}', to_jsonb($2::boolean)),
                standard_print_side_defaulted = false
            WHERE id = $1")
                    .bind(order_id).bind(standard).execute(&mut *tx).await
                    .map_err(|_| ProductionMapError::StoreFailed)?;
                if let Some(template) = calculation.and_then(|value| serde_json::from_value(value).ok()) {
                    super::postgres_mini_order::remember_print_side(&mut tx, &template, standard)
                        .await.map_err(|_| ProductionMapError::StoreFailed)?;
                }
                tx.commit().await.map_err(|_| ProductionMapError::StoreFailed)
            }

            async fn qolip_start_settings(&self) -> Result<crate::core::production_map::QolipStartSettings, ProductionMapError> {
                qolip_start_settings::load(&self.pool).await
            }
            async fn update_qolip_start_settings(&self, enabled: bool, actor: &QueueActionActor) -> Result<crate::core::production_map::QolipStartSettings, ProductionMapError> {
                qolip_start_settings::update(&self.pool, enabled, actor).await
            }
            async fn output_stats(
                &self,
                request: &crate::core::production_map::output_stats::OutputStatsRequest,
                scope: Option<&[String]>,
            ) -> Result<crate::core::production_map::output_stats::OutputStatsReport, crate::core::production_map::output_stats::OutputStatsError> {
                output_stats::load(&self.pool, request, scope).await
            }

            async fn lamination_settings(&self) -> Result<crate::core::production_map::LaminationSettings, ProductionMapError> {
                lamination_settings::load(&self.pool).await
            }
            async fn update_lamination_settings(
                &self,
                patch: crate::core::production_map::LaminationLabelSettingsPatch,
                actor: &QueueActionActor,
            ) -> Result<crate::core::production_map::LaminationSettings, ProductionMapError> {
                lamination_settings::update(&self.pool, patch, actor).await
            }
            async fn reclose_reactivated_work(&self, target: &crate::core::production_map::ReactivationRecloseTarget, actor: &QueueActionActor, assigned: &[String]) -> Result<(), ProductionMapError> {
                reactivation::reclose::execute(self, target, actor, assigned).await
            }
            async fn reactivation_was_applied(&self, target: &crate::core::production_map::ReactivationTarget, actor: &QueueActionActor) -> Result<bool, ProductionMapError> {
                reactivation::was_applied(self, target, actor).await
            }
            async fn preview_reactivation(&self, selector: &crate::core::production_map::ReactivationSelector, actor: &QueueActionActor, is_print: bool, is_lamination: bool) -> Result<crate::core::production_map::ReactivationPreview, ProductionMapError> {
                reactivation::preview(self, selector, actor, is_print, is_lamination).await
            }
            async fn reactivate_completed_work(&self, target: &crate::core::production_map::ReactivationTarget, actor: &QueueActionActor, assigned: &[String], checkouts: &[crate::core::qolip::QolipCheckout], is_print: bool, is_lamination: bool) -> Result<(), ProductionMapError> {
                reactivation::execute(self, target, actor, assigned, checkouts, is_print, is_lamination).await
            }
            fn supports_resume_pause(&self) -> bool { true }
            fn supports_temporary_pause(&self) -> bool { true }
            fn supports_cancel_start(&self) -> bool { true }
            async fn scanned_wip_inputs(&self, apparatus: &str, order: &str, stage: &str,
                actor: &QueueActionActor) -> Result<Vec<crate::core::production_map::ScannedWipInput>, ProductionMapError> {
                scanned_wip::load(self, apparatus, order, stage, actor).await
            }
            async fn return_scanned_wip(&self, target: &crate::core::production_map::ScannedWipReturn,
                actor: &QueueActionActor, assigned: &[String]) -> Result<(), ProductionMapError> {
                scanned_wip::execute(self, target, actor, assigned).await
            }
            async fn cancel_started_work(&self, apparatus: &str, order_id: &str, session_id: &str, token: &str,
                actor: &QueueActionActor, assigned: &[String]) -> Result<(), ProductionMapError> {
                cancel_start::execute(self, apparatus, order_id, session_id, token, actor, assigned).await
            }
            async fn temporarily_pause_work(&self, apparatus: &str, order_id: &str, session_id: &str, token: &str,
                actor: &QueueActionActor, assigned: &[String], policy: crate::core::production_map::ApparatusQueuePolicy)
                -> Result<(), ProductionMapError> {
                temporary_pause::execute(self, apparatus, order_id, session_id, token, actor, assigned, policy).await
            }

            async fn re_pause_resumed_work(
                &self, apparatus: &str, order_id: &str, resume_id: &str,
                actor: &QueueActionActor, assigned_apparatus: &[String],
            ) -> Result<(), ProductionMapError> {
                re_pause::re_pause(self, apparatus, order_id, resume_id, actor, assigned_apparatus).await
            }


            async fn layer_remainder_sources(&self, apparatus: &str, order_id: &str) -> Result<Vec<crate::core::production_map::LayerRemainderSource>, ProductionMapError> {
                PostgresProductionMapStore::layer_remainder_sources(self, apparatus, order_id).await
            }
            async fn layer_remainder_history(&self, apparatus: &str, order_id: &str) -> Result<Vec<crate::core::production_map::LayerRemainderRecord>, ProductionMapError> {
                PostgresProductionMapStore::layer_remainder_history(self, apparatus, order_id).await
            }
            async fn layer_remainder_by_id(&self, id: &str) -> Result<Option<crate::core::production_map::LayerRemainderRecord>, ProductionMapError> {
                PostgresProductionMapStore::layer_remainder_by_id(self, id).await
            }
            async fn record_layer_remainder(&self, input: crate::core::production_map::LayerRemainderInput, actor: QueueActionActor) -> Result<crate::core::production_map::LayerRemainderRecord, ProductionMapError> {
                PostgresProductionMapStore::record_layer_remainder(self, input, actor).await
            }
            async fn laminatsiya_astatka_commit_by_request_id(&self, actor_ref: &str, request_id: &str) -> Result<Option<(serde_json::Value,crate::core::production_map::LaminatsiyaAstatkaCommitResult)>,ProductionMapError> {
                PostgresProductionMapStore::laminatsiya_astatka_commit_by_request_id(self,actor_ref,request_id).await
            }
            async fn commit_laminatsiya_astatka(&self, request_id:String, request_json:serde_json::Value,
                report:Option<LaminatsiyaAstatkaReport>, expected_session:OrderRunSession, actor:QueueActionActor,
                queue_write:Option<QueueActionProgressWrite>,remainders:Vec<crate::core::production_map::LayerRemainderInput>,
            ) -> Result<crate::core::production_map::LaminatsiyaAstatkaCommitResult,ProductionMapError> {
                PostgresProductionMapStore::commit_laminatsiya_astatka(self,request_id,request_json,report,expected_session,actor,queue_write,remainders).await
            }
            async fn paddon_management_settings(&self) -> Result<PaddonManagementSettings, ProductionMapError> {
                paddon_management::load(&self.pool).await
            }
            async fn update_paddon_management_settings(&self, free_movement_enabled: Option<bool>, worker_visibility_enabled: Option<bool>, actor: &QueueActionActor) -> Result<PaddonManagementSettings, ProductionMapError> {
                paddon_management::update(&self.pool, free_movement_enabled, worker_visibility_enabled, actor).await
            }
            async fn move_apparatus_sequence(
                &self,
                canonical: &crate::core::apparatus_standard::RuntimeApparatusConfiguration,
                command: &crate::core::production_map::SequenceMove,
                actor: &QueueActionActor,
            ) -> Result<crate::core::production_map::SequenceMoveResult, ProductionMapError> {
                sequence_move::commit(&self.pool, canonical, command, actor).await
            }
            async fn rezka_astatka_commit_by_request_id(
                &self, actor: &QueueActionActor, request_id: &str,
            ) -> Result<Option<(serde_json::Value, RezkaAstatkaReport)>, ProductionMapError> {
                stage_execution::rezka_receipt(&self.pool, actor, request_id).await
            }
            async fn commit_rezka_astatka_report(
                &self, request_id: String, request_json: serde_json::Value,
                report: RezkaAstatkaReport, expected_session: OrderRunSession, actor: QueueActionActor,
            ) -> Result<RezkaAstatkaReport, ProductionMapError> {
                stage_execution::commit_rezka_report(&self.pool, request_id, request_json, report, expected_session, actor).await
            }
            async fn commit_stage_astatka_report(&self, report: crate::core::production_map::StageAstatkaReport,
                expected: Option<OrderRunSession>, actor: QueueActionActor) -> Result<(), ProductionMapError> {
                stage_execution::commit_report(&self.pool, report, expected, actor).await
            }
            async fn paddon_receipt(&self, code: &str) -> Result<Option<crate::core::production_map::PaddonReceipt>, ProductionMapError> {
                paddon_receipts::load(&self.pool, code).await
            }

            async fn warehouse_wip_snapshot(&self, qr: &str) -> Result<Option<crate::core::production_map::WarehouseWipSnapshot>, ProductionMapError> {
                warehouse_wip::load(&self.pool, qr).await
            }
            async fn receive_warehouse_wip(&self, write: crate::core::production_map::WarehouseWipReceiveWrite) -> Result<(), ProductionMapError> {
                warehouse_wip::commit(&self.pool, write).await
            }
            async fn receive_paddon(&self, write: crate::core::production_map::PaddonReceiveWrite) -> Result<crate::core::production_map::PaddonReceipt, ProductionMapError> {
                paddon_receipts::commit(&self.pool, write).await
            }
            async fn maps(&self) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
                PostgresProductionMapStore::maps(self).await
            }

            async fn map_by_id(&self, map_id: &str) -> Result<Option<ProductionMapDefinition>, ProductionMapError> {
                #[cfg(test)]
                if self.legacy_queue_reads {
                    return Ok(self.maps().await?.into_iter().find(|map| map.id.trim() == map_id.trim()));
                }
                catalog_helpers::load_map_by_id(&self.pool, map_id).await
            }

            async fn maps_for_apparatus(&self, apparatus: &str) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
                #[cfg(test)]
                if self.legacy_queue_reads { return self.maps().await; }
                catalog_helpers::load_maps_for_apparatus(&self.pool, apparatus).await
            }

            async fn maps_for_snapshot_scope(&self, apparatus: &[String], extra_order_ids: &[String]) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
                #[cfg(test)]
                if self.legacy_queue_reads { return self.maps().await; }
                catalog_helpers::load_maps_for_snapshot_scope(&self.pool, apparatus, extra_order_ids).await
            }

            async fn raw_material_assignments_for_orders(&self, order_ids: &[String]) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
                material_helpers::load_raw_material_assignments_for_orders(&self.pool, order_ids).await
            }

            async fn opening_wip_records_for_orders(&self, order_ids: &[String]) -> Result<Vec<OpeningWipRecord>, ProductionMapError> {
                opening_wip_helpers::load_opening_wip_records_for_orders(&self.pool, order_ids).await
            }

            async fn stage_order_control_states(&self) -> Result<Vec<OrderControlRecord>, ProductionMapError> {
                order_control_helpers::load_stage_controls(&self.pool).await
            }

            async fn stage_order_control_by_id(
                &self,
                order_id: &str,
                apparatus: &str,
                stage_node_id: &str,
            ) -> Result<Option<OrderControlRecord>, ProductionMapError> {
                let record = sqlx::query_scalar::<_, sqlx::types::Json<OrderControlRecord>>(
                    "SELECT record_json FROM mini_stage_order_controls
             WHERE order_id = $1 AND canonical_apparatus_id = $2 AND stage_node_id = $3",
                )
                .bind(order_id.trim())
                .bind(apparatus.trim())
                .bind(stage_node_id.trim())
                .fetch_optional(&self.pool)
                .await
                .map_err(|_| ProductionMapError::StoreFailed)?;
                Ok(record.map(|json| json.0))
            }

            async fn order_control_by_id(&self, order_id: &str) -> Result<Option<OrderControlRecord>, ProductionMapError> {
                #[cfg(test)]
                if self.legacy_queue_reads { return Ok(self.order_control_states().await?.remove(order_id.trim())); }
                order_control_helpers::load_order_control_by_id(&self.pool, order_id).await
            }

            async fn raw_material_assignments_for_order(&self, order_id: &str) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
                #[cfg(test)]
                if self.legacy_queue_reads {
                    return Ok(self.raw_material_assignments().await?.into_iter()
                        .filter(|assignment| assignment.order_id.trim() == order_id.trim()).collect());
                }
                material_helpers::load_raw_material_assignments_for_order(&self.pool, order_id).await
            }

            async fn active_order_run_sessions_for_apparatus(&self, apparatus: &str) -> Result<Vec<OrderRunSession>, ProductionMapError> {
                #[cfg(test)]
                if self.legacy_queue_reads {
                    return Ok(self.order_run_sessions_for_audit().await?.into_iter()
                        .filter(|session| session.status == crate::core::production_map::OrderRunStatus::Active && session.apparatus.trim() == apparatus.trim()).collect());
                }
                order_query_helpers::load_active_order_run_sessions_for_apparatus(&self.pool, apparatus).await
            }

            async fn maps_by_lifecycle_statuses(
                &self,
                statuses: &[ProductionOrderLifecycleStatus],
            ) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
                PostgresProductionMapStore::maps_by_lifecycle_statuses(self, statuses).await
            }

            async fn raw_material_assignment_maps(
                &self,
            ) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
                PostgresProductionMapStore::raw_material_assignment_maps(self).await
            }

            async fn production_order_lifecycles(
                &self,
                order_ids: &[String],
            ) -> Result<BTreeMap<String, ProductionOrderLifecycleRecord>, ProductionMapError> {
                PostgresProductionMapStore::production_order_lifecycles(self, order_ids).await
            }

            async fn next_order_number(&self) -> Result<String, ProductionMapError> {
                PostgresProductionMapStore::next_order_number(self).await
            }

            async fn put_map(&self, map: ProductionMapDefinition) -> Result<(), ProductionMapError> {
                PostgresProductionMapStore::put_map(self, map).await
            }

            async fn put_maps_batch(
                &self,
                maps: &[ProductionMapDefinition],
            ) -> Result<(), ProductionMapError> {
                PostgresProductionMapStore::put_maps_batch(self, maps).await
            }

            async fn put_maps_with_sequences(
                &self,
                maps: &[ProductionMapDefinition],
                expected_sequences: &BTreeMap<String, Vec<String>>,
                sequences: &BTreeMap<String, Vec<String>>,
            ) -> Result<(), ProductionMapError> {
                PostgresProductionMapStore::put_maps_with_sequences(
                    self,
                    maps,
                    expected_sequences,
                    sequences,
                )
                .await
            }

            async fn delete_map(&self, map_id: &str) -> Result<(), ProductionMapError> {
                PostgresProductionMapStore::delete_map(self, map_id).await
            }

            async fn order_control_states(
                &self,
            ) -> Result<BTreeMap<String, OrderControlRecord>, ProductionMapError> {
                PostgresProductionMapStore::order_control_states(self).await
            }

            async fn order_freeze_requests_for_audit(
                &self,
            ) -> Result<Vec<crate::core::production_map::OrderFreezeAuditRecord>, ProductionMapError> {
                PostgresProductionMapStore::order_freeze_requests_for_audit(self).await
            }

            async fn put_order_control_state(
                &self,
                record: OrderControlRecord,
            ) -> Result<(), ProductionMapError> {
                PostgresProductionMapStore::put_order_control_state(self, record).await
            }

            async fn apparatus_sequences(
                &self,
            ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
                PostgresProductionMapStore::apparatus_sequences(self).await
            }

            async fn apparatus_sequences_with_revisions(
                &self,
            ) -> Result<(BTreeMap<String, Vec<String>>, BTreeMap<String, i64>), ProductionMapError> {
                let rows = sqlx::query_as::<_, (String, serde_json::Value, i64)>(
                    "SELECT canonical_apparatus_id, order_ids, revision FROM mini_queue_sequences",
                )
                .fetch_all(&self.pool)
                .await
                .map_err(|_| ProductionMapError::StoreFailed)?;
                let mut sequences = BTreeMap::new();
                let mut revisions = BTreeMap::new();
                for (id, payload, revision) in rows {
                    if !crate::core::production_map::queue_state::is_canonical_apparatus_id(&id) {
                        continue;
                    }
                    let Ok(ids) = serde_json::from_value::<Vec<String>>(payload) else {
                        continue;
                    };
                    revisions.insert(id.clone(), revision);
                    sequences.insert(id, ids);
                }
                Ok((sequences, revisions))
            }

            async fn put_apparatus_sequence(
                &self,
                apparatus: &str,
                order_ids: Vec<String>,
            ) -> Result<(), ProductionMapError> {
                PostgresProductionMapStore::put_apparatus_sequence(self, apparatus, order_ids).await
            }

            async fn queue_events_replay(
                &self,
                apparatus: &str,
                from_rev: i64,
                to_rev: i64,
            ) -> Result<Vec<serde_json::Value>, ProductionMapError> {
                PostgresProductionMapStore::queue_events_replay(self, apparatus, from_rev, to_rev).await
            }

            async fn apparatus_downtimes(&self) -> Result<Vec<ApparatusDowntime>, ProductionMapError> {
                PostgresProductionMapStore::apparatus_downtimes(self).await
            }

            async fn put_apparatus_downtime(
                &self,
                downtime: ApparatusDowntime,
            ) -> Result<(), ProductionMapError> {
                PostgresProductionMapStore::put_apparatus_downtime(self, downtime).await
            }

            async fn apparatus_schedule_reservations(
                &self,
            ) -> Result<Vec<ApparatusScheduleReservation>, ProductionMapError> {
                PostgresProductionMapStore::apparatus_schedule_reservations(self).await
            }

            async fn apparatus_schedule_reservation_by_idempotency_key(
                &self,
                idempotency_key: &str,
            ) -> Result<Option<ApparatusScheduleReservation>, ProductionMapError> {
                PostgresProductionMapStore::apparatus_schedule_reservation_by_idempotency_key(self, idempotency_key).await
            }

            async fn put_apparatus_schedule_reservation(
                &self,
                reservation: ApparatusScheduleReservation,
                capacity_slots: u16,
                finite_capacity: bool,
            ) -> Result<ApparatusScheduleReservation, ProductionMapError> {
                PostgresProductionMapStore::put_apparatus_schedule_reservation(self, reservation, capacity_slots, finite_capacity).await
            }

            async fn cancel_apparatus_schedule_reservation(
                &self,
                input: ApparatusScheduleCancelRequest,
            ) -> Result<ApparatusScheduleReservation, ProductionMapError> {
                PostgresProductionMapStore::cancel_apparatus_schedule_reservation(self, input).await
            }

            async fn update_apparatus_schedule_reservation_status(
                &self,
                order_id: &str,
                apparatus_id: &ApparatusId,
                status: crate::core::production_map::ApparatusScheduleStatus,
                actor: &QueueActionActor,
            ) -> Result<(), ProductionMapError> {
                PostgresProductionMapStore::update_apparatus_schedule_reservation_status(self, order_id, apparatus_id, status, actor).await
            }

            async fn active_print_preflight_holds(
                &self,
            ) -> Result<Vec<crate::core::production_map::PrintPreflightHold>, ProductionMapError> {
                PostgresProductionMapStore::active_print_preflight_holds(self).await
            }

            async fn print_preflight_hold_by_id(
                &self,
                hold_id: &str,
            ) -> Result<Option<crate::core::production_map::PrintPreflightHold>, ProductionMapError> {
                PostgresProductionMapStore::print_preflight_hold_by_id(self, hold_id).await
            }

            async fn print_preflight_hold_by_idempotency_key(
                &self,
                idempotency_key: &str,
            ) -> Result<Option<crate::core::production_map::PrintPreflightHold>, ProductionMapError> {
                PostgresProductionMapStore::print_preflight_hold_by_idempotency_key(self, idempotency_key).await
            }

            async fn put_print_preflight_hold(
                &self,
                hold: crate::core::production_map::PrintPreflightHold,
            ) -> Result<(), ProductionMapError> {
                PostgresProductionMapStore::put_print_preflight_hold(self, hold).await
            }

            async fn update_print_preflight_hold(
                &self,
                hold: crate::core::production_map::PrintPreflightHold,
            ) -> Result<(), ProductionMapError> {
                PostgresProductionMapStore::update_print_preflight_hold(self, hold).await
            }

            async fn put_print_preflight_hold_with_event(
                &self,
                hold: crate::core::production_map::PrintPreflightHold,
            ) -> Result<Option<String>, ProductionMapError> {
                print_preflight::put_with_event(&self.pool, &hold).await.map(Some)
            }

            async fn update_print_preflight_hold_with_event(
                &self,
                hold: crate::core::production_map::PrintPreflightHold,
            ) -> Result<Option<String>, ProductionMapError> {
                print_preflight::update_with_event(&self.pool, &hold).await.map(Some)
            }

            async fn reopen_print_preflight_hold_with_event(
                &self,
                hold: crate::core::production_map::PrintPreflightHold,
            ) -> Result<Option<String>, ProductionMapError> {
                print_preflight::reopen_with_event(&self.pool, &hold).await.map(Some)
            }

            async fn consume_print_preflight_hold(
                &self,
                hold_id: &str,
                order_id: &str,
                apparatus: &str,
                actor: &QueueActionActor,
            ) -> Result<(), ProductionMapError> {
                PostgresProductionMapStore::consume_print_preflight_hold(
                    self, hold_id, order_id, apparatus, actor,
                )
                .await
            }

            async fn cancel_print_preflight_hold(
                &self,
                hold_id: &str,
                order_id: &str,
                apparatus: &str,
                actor: &QueueActionActor,
            ) -> Result<(), ProductionMapError> {
                PostgresProductionMapStore::cancel_print_preflight_hold(
                    self, hold_id, order_id, apparatus, actor,
                )
                .await
            }
            $($following)*
        }
    };
}
