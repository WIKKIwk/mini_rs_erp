macro_rules! production_map_catalog_methods {
    ($($following:tt)*) => {
        #[async_trait]
        pub trait ProductionMapStorePort: Send + Sync {
            async fn set_order_print_side(
                &self,
                _order_id: &str,
                _assigned: &[String],
                _standard: bool,
                _previous: bool,
            ) -> StoreResult<()> {
                Err(ProductionMapError::StoreFailed)
            }

            async fn qolip_start_settings(&self) -> StoreResult<super::QolipStartSettings> {
                Ok(super::QolipStartSettings::default())
            }
            async fn update_qolip_start_settings(&self, _enabled: bool, _actor: &QueueActionActor) -> StoreResult<super::QolipStartSettings> {
                Err(ProductionMapError::StoreFailed)
            }
            /// An independent, read-only reporting snapshot; unsupported stores fail closed.
            async fn output_stats(
                &self,
                _request: &super::output_stats::OutputStatsRequest,
                _scope: Option<&[String]>,
            ) -> Result<super::output_stats::OutputStatsReport, super::output_stats::OutputStatsError> {
                Err(super::output_stats::OutputStatsError::StoreFailed)
            }

            async fn lamination_settings(&self) -> StoreResult<LaminationSettings> {
                Ok(LaminationSettings::default())
            }
            async fn update_lamination_settings(
                &self,
                _patch: super::LaminationLabelSettingsPatch,
                _actor: &QueueActionActor,
            ) -> StoreResult<LaminationSettings> {
                Err(ProductionMapError::StoreFailed)
            }
            async fn rezka_astatka_commit_by_request_id(
                &self, _actor: &QueueActionActor, _request_id: &str,
            ) -> StoreResult<Option<(serde_json::Value, RezkaAstatkaReport)>> {
                Err(ProductionMapError::StoreFailed)
            }
            async fn commit_rezka_astatka_report(
                &self, _request_id: String, _request_json: serde_json::Value,
                _report: RezkaAstatkaReport, _expected_session: OrderRunSession, _actor: QueueActionActor,
            ) -> StoreResult<RezkaAstatkaReport> {
                Err(ProductionMapError::StoreFailed)
            }
            async fn laminatsiya_astatka_commit_by_request_id(
                &self, _actor_ref: &str, _request_id: &str,
            ) -> StoreResult<Option<(serde_json::Value, super::LaminatsiyaAstatkaCommitResult)>> {
                Err(ProductionMapError::StoreFailed)
            }
            #[allow(clippy::too_many_arguments)]
            async fn commit_laminatsiya_astatka(
                &self, _request_id: String, _request_json: serde_json::Value,
                _report: Option<LaminatsiyaAstatkaReport>, _expected_session: OrderRunSession,
                _actor: QueueActionActor, _queue_write: Option<QueueActionProgressWrite>,
                _remainders: Vec<super::LayerRemainderInput>,
            ) -> StoreResult<super::LaminatsiyaAstatkaCommitResult> {
                Err(ProductionMapError::StoreFailed)
            }
            async fn layer_remainder_sources(&self, _apparatus: &str, _order_id: &str) -> StoreResult<Vec<super::LayerRemainderSource>> { Err(ProductionMapError::StoreFailed) }
            async fn layer_remainder_history(&self, _apparatus: &str, _order_id: &str) -> StoreResult<Vec<super::LayerRemainderRecord>> { Err(ProductionMapError::StoreFailed) }
            async fn layer_remainder_by_id(&self, _id: &str) -> StoreResult<Option<super::LayerRemainderRecord>> { Err(ProductionMapError::StoreFailed) }
            async fn record_layer_remainder(&self, _input: super::LayerRemainderInput, _actor: QueueActionActor) -> StoreResult<super::LayerRemainderRecord> { Err(ProductionMapError::StoreFailed) }
            async fn commit_stage_astatka_report(
                &self, _report: super::StageAstatkaReport,
                _expected_session: Option<OrderRunSession>, _actor: QueueActionActor,
            ) -> StoreResult<()> { Err(ProductionMapError::StoreFailed) }
            // Maps and apparatus sequence persistence.
            async fn maps(&self) -> StoreResult<Vec<ProductionMapDefinition>>;
            /// Fresh point read; persistent stores override the compatibility fallback.
            async fn map_by_id(&self, map_id: &str) -> StoreResult<Option<ProductionMapDefinition>> {
                Ok(self.maps().await?.into_iter().find(|map| map.id.trim() == map_id.trim()))
            }
            /// A superset of maps containing this station, in the same order as maps().
            /// Callers still apply the authoritative chain/alternative-stage rules.
            async fn maps_for_apparatus(&self, _apparatus: &str) -> StoreResult<Vec<ProductionMapDefinition>> {
                self.maps().await
            }
            /// Ordered candidate maps for a worker and its explicit history/context.
            /// Persistent stores filter before decoding definitions. Chain rules remain
            /// authoritative in the service; the fallback is deliberately a superset.
            async fn maps_for_snapshot_scope(
                &self, _apparatus: &[String], _extra_order_ids: &[String],
            ) -> StoreResult<Vec<ProductionMapDefinition>> {
                self.maps().await
            }
            async fn raw_material_assignment_maps(&self) -> StoreResult<Vec<ProductionMapDefinition>> {
                Ok(self.maps_by_lifecycle_statuses(&[
                    ProductionOrderLifecycleStatus::Released,
                    ProductionOrderLifecycleStatus::InProgress,
                ]).await?.into_iter()
                    .filter(|map| map.id.trim().starts_with("zakaz-"))
                    .collect())
            }
            async fn maps_by_lifecycle_statuses(
                &self,
                statuses: &[ProductionOrderLifecycleStatus],
            ) -> StoreResult<Vec<ProductionMapDefinition>> {
                if statuses.is_empty() {
                    return Ok(Vec::new());
                }
                let maps = self.maps().await?;
                let order_ids = maps
                    .iter()
                    .map(|map| map.id.trim().to_string())
                    .filter(|order_id| !order_id.is_empty())
                    .collect::<Vec<_>>();
                let lifecycles = self.production_order_lifecycles(&order_ids).await?;
                Ok(maps
                    .into_iter()
                    .filter(|map| {
                        lifecycles
                            .get(map.id.trim())
                            .is_some_and(|record| statuses.contains(&record.status))
                    })
                    .collect())
            }
            async fn production_order_lifecycles(
                &self,
                order_ids: &[String],
            ) -> StoreResult<BTreeMap<String, ProductionOrderLifecycleRecord>> {
                let requested = order_ids
                    .iter()
                    .map(|order_id| order_id.trim())
                    .filter(|order_id| !order_id.is_empty())
                    .collect::<BTreeSet<_>>();
                let queue_states = self.apparatus_queue_states().await?;
                let mut records = BTreeMap::new();
                for map in self.maps().await? {
                    let order_id = map.id.trim();
                    if !requested.is_empty() && !requested.contains(order_id) {
                        continue;
                    }
                    let Some(status) = super::progress::derive_production_order_lifecycle(
                        &map,
                        &queue_states,
                    ) else {
                        continue;
                    };
                    let mut record = ProductionOrderLifecycleRecord::released(order_id);
                    record.transition_to(status, 0);
                    records.insert(order_id.to_string(), record);
                }
                Ok(records)
            }
            async fn put_map(&self, map: ProductionMapDefinition) -> StoreResult<()>;
            async fn put_maps_batch(&self, maps: &[ProductionMapDefinition]) -> StoreResult<()>;
            /// Persist a pending apparatus move and its queue positions together.
            /// Reject stale queue snapshots instead of overwriting a concurrent reorder.
            async fn put_maps_with_sequences(
                &self,
                _maps: &[ProductionMapDefinition],
                _expected_sequences: &ApparatusSequenceMap,
                _sequences: &ApparatusSequenceMap,
            ) -> StoreResult<()> {
                Err(ProductionMapError::StoreFailed)
            }
            async fn next_order_number(&self) -> StoreResult<String> {
                let max_order_number = self
                    .maps()
                    .await?
                    .iter()
                    .filter_map(|map| {
                        let value = map.order_number.trim();
                        (value.len() <= 4
                            && !value.is_empty()
                            && value.chars().all(|ch| ch.is_ascii_digit()))
                        .then(|| value.parse::<u32>().ok())
                        .flatten()
                    })
                    .max()
                    .unwrap_or_default();
                let next_order_number = max_order_number
                    .checked_add(1)
                    .filter(|value| *value <= 9999)
                    .ok_or(ProductionMapError::OrderNumberExhausted)?;
                Ok(format!("{next_order_number:04}"))
            }
            async fn delete_map(&self, map_id: &str) -> StoreResult<()>;
            async fn order_control_states(&self) -> StoreResult<OrderControlMap> {
                Ok(BTreeMap::new())
            }
            async fn order_control_by_id(&self, order_id: &str) -> StoreResult<Option<OrderControlRecord>> {
                Ok(self.order_control_states().await?.remove(order_id.trim()))
            }
            /// Stage holds are independent from the whole-order control overlay.
            async fn stage_order_control_states(&self) -> StoreResult<Vec<OrderControlRecord>> {
                Ok(Vec::new())
            }
            async fn stage_order_control_by_id(
                &self, order_id: &str, apparatus: &str, stage_node_id: &str,
            ) -> StoreResult<Option<OrderControlRecord>> {
                let mut matches = self.stage_order_control_states().await?.into_iter().filter(|record| {
                    record.order_id.trim() == order_id.trim()
                        && record.stage_node_id.trim() == stage_node_id.trim()
                        && record.freeze_request.as_ref().is_some_and(|request|
                            super::queue_state::apparatus_ids_match(&request.target_apparatus, apparatus))
                });
                let record = matches.next();
                if matches.next().is_some() { return Err(ProductionMapError::OrderFreezeTargetAmbiguous); }
                Ok(record)
            }
            async fn order_freeze_requests_for_audit(&self) -> StoreResult<Vec<OrderFreezeAuditRecord>> {
                Ok(Vec::new())
            }
            async fn put_order_control_state(&self, _record: OrderControlRecord) -> StoreResult<()> {
                Ok(())
            }
            async fn apparatus_sequences(&self) -> StoreResult<ApparatusSequenceMap>;
            /// Sequence and durable cursor must describe the same database read.
            /// Stores without durable cursors use snapshot-only recovery.
            async fn apparatus_sequences_with_revisions(
                &self,
            ) -> StoreResult<(ApparatusSequenceMap, BTreeMap<String, i64>)> {
                Ok((self.apparatus_sequences().await?, BTreeMap::new()))
            }
            async fn move_apparatus_sequence(
                &self,
                _canonical: &crate::core::apparatus_standard::RuntimeApparatusConfiguration,
                _command: &super::SequenceMove,
                _actor: &QueueActionActor,
            ) -> StoreResult<super::SequenceMoveResult> {
                // Never emulate an atomic command with a non-atomic read/write pair.
                Err(ProductionMapError::StoreFailed)
            }
            async fn put_apparatus_sequence(
                &self,
                apparatus: &str,
                order_ids: Vec<String>,
            ) -> StoreResult<()>;
            async fn queue_events_replay(
                &self,
                _apparatus: &str,
                _from_rev: i64,
                _to_rev: i64,
            ) -> StoreResult<Vec<serde_json::Value>> {
                Ok(Vec::new())
            }

            // Finite capacity, working calendars, downtime, and reservations.
            async fn apparatus_downtimes(&self) -> StoreResult<Vec<ApparatusDowntime>> {
                Ok(Vec::new())
            }
            async fn put_apparatus_downtime(&self, _downtime: ApparatusDowntime) -> StoreResult<()> {
                Err(ProductionMapError::StoreFailed)
            }
            async fn apparatus_schedule_reservations(
                &self,
            ) -> StoreResult<Vec<ApparatusScheduleReservation>> {
                Ok(Vec::new())
            }
            async fn apparatus_schedule_reservation_by_idempotency_key(
                &self,
                _idempotency_key: &str,
            ) -> StoreResult<Option<ApparatusScheduleReservation>> {
                Ok(None)
            }
            async fn put_apparatus_schedule_reservation(
                &self,
                _reservation: ApparatusScheduleReservation,
                _capacity_slots: u16,
                _finite_capacity: bool,
            ) -> StoreResult<ApparatusScheduleReservation> {
                Err(ProductionMapError::StoreFailed)
            }
            async fn cancel_apparatus_schedule_reservation(
                &self,
                _input: ApparatusScheduleCancelRequest,
            ) -> StoreResult<ApparatusScheduleReservation> {
                Err(ProductionMapError::ScheduleReservationNotFound)
            }
            async fn update_apparatus_schedule_reservation_status(
                &self,
                _order_id: &str,
                _apparatus_id: &ApparatusId,
                _status: ApparatusScheduleStatus,
                _actor: &QueueActionActor,
            ) -> StoreResult<()> {
                Ok(())
            }

            // Queue state, policy, log, and completion-request persistence.
            async fn active_print_preflight_holds(&self) -> StoreResult<Vec<PrintPreflightHold>> {
                Ok(Vec::new())
            }
            async fn print_preflight_hold_by_id(
                &self,
                _hold_id: &str,
            ) -> StoreResult<Option<PrintPreflightHold>> {
                Ok(None)
            }
            async fn print_preflight_hold_by_idempotency_key(
                &self,
                _idempotency_key: &str,
            ) -> StoreResult<Option<PrintPreflightHold>> {
                Ok(None)
            }
            async fn put_print_preflight_hold(
                &self,
                _hold: PrintPreflightHold,
            ) -> StoreResult<()> {
                Err(ProductionMapError::StoreFailed)
            }
            async fn update_print_preflight_hold(
                &self,
                _hold: PrintPreflightHold,
            ) -> StoreResult<()> {
                Err(ProductionMapError::StoreFailed)
            }
            /// Return the exact committed notification ID when persistence also
            /// publishes to another process. Memory/legacy stores need no deduplication.
            async fn put_print_preflight_hold_with_event(
                &self,
                hold: PrintPreflightHold,
            ) -> StoreResult<Option<String>> {
                self.put_print_preflight_hold(hold).await?;
                Ok(None)
            }
            async fn update_print_preflight_hold_with_event(
                &self,
                hold: PrintPreflightHold,
            ) -> StoreResult<Option<String>> {
                self.update_print_preflight_hold(hold).await?;
                Ok(None)
            }
            /// Atomically reopen only an unconsumed, passed trial on an active order.
            async fn reopen_print_preflight_hold_with_event(
                &self,
                _hold: PrintPreflightHold,
            ) -> StoreResult<Option<String>> {
                Err(ProductionMapError::PrintPreflightActionNotAllowed)
            }
            async fn consume_print_preflight_hold(
                &self,
                _hold_id: &str,
                _order_id: &str,
                _apparatus: &str,
                _actor: &QueueActionActor,
            ) -> StoreResult<()> {
                Err(ProductionMapError::PrintPreflightNotReady)
            }
            async fn cancel_print_preflight_hold(
                &self,
                _hold_id: &str,
                _order_id: &str,
                _apparatus: &str,
                _actor: &QueueActionActor,
            ) -> StoreResult<()> {
                Err(ProductionMapError::PrintPreflightNotReady)
            }
            /// Only stores with atomic checkpoint validation expose output-free re-pause.
            fn supports_resume_pause(&self) -> bool { false }
            fn supports_temporary_pause(&self) -> bool { false }
            fn supports_cancel_start(&self) -> bool { false }
            async fn scanned_wip_inputs(
                &self, _apparatus: &str, _order: &str, _stage: &str, _actor: &QueueActionActor,
            ) -> StoreResult<Vec<super::ScannedWipInput>> { Ok(Vec::new()) }
            async fn return_scanned_wip(
                &self, _target: &super::ScannedWipReturn, _actor: &QueueActionActor, _assigned: &[String],
            ) -> StoreResult<()> { Err(ProductionMapError::QueueActionNotAllowed) }
            async fn cancel_started_work(
                &self, _apparatus: &str, _order_id: &str, _session_id: &str, _token: &str,
                _actor: &QueueActionActor, _assigned: &[String],
            ) -> StoreResult<()> {
                Err(ProductionMapError::QueueActionNotAllowed)
            }
            async fn temporarily_pause_work(
                &self, _apparatus: &str, _order_id: &str, _session_id: &str, _token: &str,
                _actor: &QueueActionActor, _assigned: &[String], _policy: ApparatusQueuePolicy,
            ) -> StoreResult<()> {
                Err(ProductionMapError::QueueActionNotAllowed)
            }
            async fn re_pause_resumed_work(
                &self, _apparatus: &str, _order_id: &str, _resume_id: &str,
                _actor: &QueueActionActor, _assigned_apparatus: &[String],
            ) -> StoreResult<()> {
                Err(ProductionMapError::QueueActionNotAllowed)
            }
            async fn reclose_reactivated_work(&self, _target: &super::ReactivationRecloseTarget, _actor: &QueueActionActor, _assigned: &[String]) -> StoreResult<()> {
                Err(ProductionMapError::QueueActionNotAllowed)
            }
            async fn reactivation_was_applied(&self, _target: &super::ReactivationTarget, _actor: &QueueActionActor) -> StoreResult<bool> {
                Ok(false)
            }
            async fn preview_reactivation(&self, _selector: &super::ReactivationSelector, _actor: &QueueActionActor, _is_print: bool, _is_lamination: bool) -> StoreResult<super::ReactivationPreview> {
                Ok(super::ReactivationPreview::denied("unsupported_history"))
            }
            async fn reactivate_completed_work(&self, _target: &super::ReactivationTarget, _actor: &QueueActionActor, _assigned: &[String], _checkouts: &[QolipCheckout], _is_print: bool, _is_lamination: bool) -> StoreResult<()> {
                Err(ProductionMapError::QueueActionNotAllowed)
            }
            $($following)*
        }
    };
}
