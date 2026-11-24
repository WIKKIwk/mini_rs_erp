macro_rules! memory_map_scheduling {
    ($($following:tt)*) => {
        memory_map_settings! {
            async fn maps(&self) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
                MemoryProductionMapStore::maps(self).await
            }

            async fn production_order_lifecycles(
                &self,
                order_ids: &[String],
            ) -> Result<BTreeMap<String, ProductionOrderLifecycleRecord>, ProductionMapError> {
                MemoryProductionMapStore::production_order_lifecycles(self, order_ids).await
            }

            async fn put_map(&self, map: ProductionMapDefinition) -> Result<(), ProductionMapError> {
                MemoryProductionMapStore::put_map(self, map).await
            }

            async fn put_maps_batch(
                &self,
                maps: &[ProductionMapDefinition],
            ) -> Result<(), ProductionMapError> {
                MemoryProductionMapStore::put_maps_batch(self, maps).await
            }

            async fn put_maps_with_sequences(
                &self,
                maps: &[ProductionMapDefinition],
                expected_sequences: &BTreeMap<String, Vec<String>>,
                sequences: &BTreeMap<String, Vec<String>>,
            ) -> Result<(), ProductionMapError> {
                maps::put_maps_with_sequences(self, maps, expected_sequences, sequences).await
            }

            async fn delete_map(&self, map_id: &str) -> Result<(), ProductionMapError> {
                MemoryProductionMapStore::delete_map(self, map_id).await
            }

            async fn order_control_states(
                &self,
            ) -> Result<BTreeMap<String, OrderControlRecord>, ProductionMapError> {
                MemoryProductionMapStore::order_control_states(self).await
            }

            async fn stage_order_control_states(&self) -> Result<Vec<OrderControlRecord>, ProductionMapError> {
                Ok(self.stage_order_controls.read().await.values().cloned().collect())
            }

            async fn stage_order_control_by_id(
                &self,
                order_id: &str,
                apparatus: &str,
                stage_node_id: &str,
            ) -> Result<Option<OrderControlRecord>, ProductionMapError> {
                let key = (
                    order_id.trim().to_owned(),
                    apparatus.trim().to_owned(),
                    stage_node_id.trim().to_owned(),
                );
                Ok(self.stage_order_controls.read().await.get(&key).cloned())
            }

            async fn order_freeze_requests_for_audit(
                &self,
            ) -> Result<Vec<OrderFreezeAuditRecord>, ProductionMapError> {
                MemoryProductionMapStore::order_freeze_requests_for_audit(self).await
            }

            async fn put_order_control_state(
                &self,
                record: OrderControlRecord,
            ) -> Result<(), ProductionMapError> {
                MemoryProductionMapStore::put_order_control_state(self, record).await
            }

            async fn apparatus_sequences(
                &self,
            ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
                MemoryProductionMapStore::apparatus_sequences(self).await
            }

            async fn put_apparatus_sequence(
                &self,
                apparatus: &str,
                order_ids: Vec<String>,
            ) -> Result<(), ProductionMapError> {
                MemoryProductionMapStore::put_apparatus_sequence(self, apparatus, order_ids).await
            }

            async fn apparatus_downtimes(&self) -> Result<Vec<ApparatusDowntime>, ProductionMapError> {
                MemoryProductionMapStore::apparatus_downtimes(self).await
            }

            async fn put_apparatus_downtime(
                &self,
                downtime: ApparatusDowntime,
            ) -> Result<(), ProductionMapError> {
                MemoryProductionMapStore::put_apparatus_downtime(self, downtime).await
            }

            async fn apparatus_schedule_reservations(
                &self,
            ) -> Result<Vec<ApparatusScheduleReservation>, ProductionMapError> {
                MemoryProductionMapStore::apparatus_schedule_reservations(self).await
            }

            async fn apparatus_schedule_reservation_by_idempotency_key(
                &self,
                idempotency_key: &str,
            ) -> Result<Option<ApparatusScheduleReservation>, ProductionMapError> {
                MemoryProductionMapStore::apparatus_schedule_reservation_by_idempotency_key(
                    self,
                    idempotency_key,
                )
                .await
            }

            async fn put_apparatus_schedule_reservation(
                &self,
                reservation: ApparatusScheduleReservation,
                capacity_slots: u16,
                finite_capacity: bool,
            ) -> Result<ApparatusScheduleReservation, ProductionMapError> {
                MemoryProductionMapStore::put_apparatus_schedule_reservation(
                    self,
                    reservation,
                    capacity_slots,
                    finite_capacity,
                )
                .await
            }

            async fn cancel_apparatus_schedule_reservation(
                &self,
                input: ApparatusScheduleCancelRequest,
            ) -> Result<ApparatusScheduleReservation, ProductionMapError> {
                MemoryProductionMapStore::cancel_apparatus_schedule_reservation(self, input).await
            }

            async fn update_apparatus_schedule_reservation_status(
                &self,
                order_id: &str,
                apparatus_id: &ApparatusId,
                status: ApparatusScheduleStatus,
                actor: &QueueActionActor,
            ) -> Result<(), ProductionMapError> {
                MemoryProductionMapStore::update_apparatus_schedule_reservation_status(
                    self,
                    order_id,
                    apparatus_id,
                    status,
                    actor,
                )
                .await
            }

            async fn active_print_preflight_holds(
                &self,
            ) -> Result<Vec<PrintPreflightHold>, ProductionMapError> {
                let now = super::progress::unix_seconds();
                Ok(self
                    .print_preflight_holds
                    .read()
                    .await
                    .values()
                    .filter(|hold| hold.is_live_at(now))
                    .cloned()
                    .collect())
            }

            async fn print_preflight_hold_by_id(
                &self,
                hold_id: &str,
            ) -> Result<Option<PrintPreflightHold>, ProductionMapError> {
                Ok(self.print_preflight_holds.read().await.get(hold_id.trim()).cloned())
            }

            async fn print_preflight_hold_by_idempotency_key(
                &self,
                idempotency_key: &str,
            ) -> Result<Option<PrintPreflightHold>, ProductionMapError> {
                Ok(self
                    .print_preflight_holds
                    .read()
                    .await
                    .values()
                    .find(|hold| hold.idempotency_key.trim() == idempotency_key.trim())
                    .cloned())
            }

            async fn put_print_preflight_hold(
                &self,
                hold: PrintPreflightHold,
            ) -> Result<(), ProductionMapError> {
                let mut holds = self.print_preflight_holds.write().await;
                if holds.contains_key(hold.hold_id.trim()) {
                    return Err(ProductionMapError::PrintPreflightActive);
                }
                let order_id = hold.order_id.clone();
                self.queue_states
                    .write()
                    .await
                    .entry(hold.apparatus.clone())
                    .or_default()
                    .insert(order_id.clone(), "print_preflight".to_string());
                holds.insert(hold.hold_id.trim().to_string(), hold);
                drop(holds);
                queue::refresh_production_order_lifecycles(self, &[order_id]).await
            }

            async fn update_print_preflight_hold(
                &self,
                hold: PrintPreflightHold,
            ) -> Result<(), ProductionMapError> {
                let mut holds = self.print_preflight_holds.write().await;
                let current = holds.get(hold.hold_id.trim())
                    .ok_or(ProductionMapError::PrintPreflightNotFound)?;
                if !current.status.reserves_apparatus()
                    || current.revision.checked_add(1) != Some(hold.revision)
                    || current.order_id != hold.order_id
                    || current.apparatus != hold.apparatus
                    || current.stage_node_id != hold.stage_node_id
                {
                    return Err(ProductionMapError::PrintPreflightNotReady);
                }
                let order_id = hold.order_id.clone();
                let mut queue_states = self.queue_states.write().await;
                let states = queue_states.entry(hold.apparatus.clone()).or_default();
                if hold.status.reserves_apparatus() {
                    states.insert(order_id.clone(), "print_preflight".to_string());
                } else if let Some(previous) = &hold.previous_queue_state {
                    states.insert(order_id.clone(), previous.clone());
                } else {
                    states.remove(&order_id);
                }
                holds.insert(hold.hold_id.trim().to_string(), hold);
                drop(queue_states);
                drop(holds);
                queue::refresh_production_order_lifecycles(self, &[order_id]).await
            }

            async fn reopen_print_preflight_hold_with_event(
                &self,
                hold: PrintPreflightHold,
            ) -> Result<Option<String>, ProductionMapError> {
                let mut holds = self.print_preflight_holds.write().await;
                let current = holds
                    .get_mut(hold.hold_id.trim())
                    .ok_or(ProductionMapError::PrintPreflightNotFound)?;
                let states = self.queue_states.read().await;
                let controls = self.order_controls.read().await;
                if hold.status != PrintPreflightStatus::Running
                    || current.status != PrintPreflightStatus::Passed
                    || current.revision.checked_add(1) != Some(hold.revision)
                    || current.order_id != hold.order_id
                    || current.apparatus != hold.apparatus
                    || current.stage_node_id != hold.stage_node_id
                    || states.get(&hold.apparatus).and_then(|s| s.get(&hold.order_id))
                        .map(String::as_str) != Some("print_preflight")
                    || controls.get(&hold.order_id)
                        .is_some_and(|c| c.state != OrderControlState::Active)
                {
                    return Err(ProductionMapError::PrintPreflightNotReady);
                }
                *current = hold;
                Ok(None)
            }

            async fn consume_print_preflight_hold(
                &self,
                hold_id: &str,
                order_id: &str,
                apparatus: &str,
                actor: &QueueActionActor,
            ) -> Result<(), ProductionMapError> {
                let now = super::progress::unix_seconds();
                let mut holds = self.print_preflight_holds.write().await;
                let hold = holds
                    .get_mut(hold_id.trim())
                    .ok_or(ProductionMapError::PrintPreflightNotFound)?;
                if hold.order_id.trim() != order_id.trim()
                    || !queue_state::apparatus_ids_match(&hold.apparatus, apparatus)
                    || hold.status != PrintPreflightStatus::Passed
                    || !hold.is_live_at(now)
                {
                    return Err(ProductionMapError::PrintPreflightNotReady);
                }
                hold.revision = hold.revision.checked_add(1)
                    .ok_or(ProductionMapError::PrintPreflightNotReady)?;
                hold.status = PrintPreflightStatus::Consumed;
                hold.actor = actor.clone();
                hold.updated_at_unix = now;
                Ok(())
            }

            async fn cancel_print_preflight_hold(
                &self,
                hold_id: &str,
                order_id: &str,
                apparatus: &str,
                actor: &QueueActionActor,
            ) -> Result<(), ProductionMapError> {
                let now = super::progress::unix_seconds();
                let mut holds = self.print_preflight_holds.write().await;
                let hold = holds
                    .get_mut(hold_id.trim())
                    .ok_or(ProductionMapError::PrintPreflightNotFound)?;
                if hold.order_id.trim() != order_id.trim()
                    || !queue_state::apparatus_ids_match(&hold.apparatus, apparatus)
                    || !matches!(
                        hold.status,
                        PrintPreflightStatus::Held
                            | PrintPreflightStatus::Running
                            | PrintPreflightStatus::Passed
                    )
                {
                    return Err(ProductionMapError::PrintPreflightNotReady);
                }
                hold.revision = hold.revision.checked_add(1)
                    .ok_or(ProductionMapError::PrintPreflightNotReady)?;
                hold.status = PrintPreflightStatus::Cancelled;
                hold.actor = actor.clone();
                hold.updated_at_unix = now;
                Ok(())
            }
            $($following)*
        }
    };
}
