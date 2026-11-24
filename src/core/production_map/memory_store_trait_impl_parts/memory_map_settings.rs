macro_rules! memory_map_settings {
    ($($following:tt)*) => {
        #[async_trait]
        #[cfg(any(test, feature = "verification"))]
        impl ProductionMapStorePort for MemoryProductionMapStore {
            async fn set_order_print_side(
                &self, order_id: &str, assigned: &[String], standard: bool, previous: bool,
            ) -> Result<(), ProductionMapError> {
                let mut maps = self.maps.write().await;
                let map = maps.get_mut(order_id).ok_or(ProductionMapError::MapNotFound)?;
                map.validate_print_side_update(assigned, standard, previous)?;
                map.standard_print_side = Some(standard);
                Ok(())
            }

            fn supports_temporary_pause(&self) -> bool { true }
            async fn temporarily_pause_work(&self, apparatus: &str, order_id: &str, session_id: &str, token: &str,
                actor: &QueueActionActor, assigned: &[String], policy: ApparatusQueuePolicy) -> Result<(), ProductionMapError> {
                if !super::temporary_pause::valid_actor(apparatus, order_id, session_id, token, actor, assigned) {
                    return Err(ProductionMapError::QueueActionNotAllowed);
                }
                let receipt = self.queue_events.read().await.iter().find(|event| event.event_id == super::temporary_pause::receipt_id(token)).cloned();
                if let Some(event) = receipt {
                    return temporary_pause::matching_receipt(&event, apparatus, order_id, session_id, actor);
                }
                let current = self.active_order_run_session(apparatus, order_id).await?.ok_or(ProductionMapError::QueueActionNotAllowed)?;
                let states = self.apparatus_queue_states().await?.get(apparatus).cloned().unwrap_or_default();
                let write = super::temporary_pause::suspension_write(&current, session_id, token, actor, assigned, states, policy)?;
                self.put_apparatus_queue_states_with_event_and_progress(&write).await?;
                Ok(())
            }
            async fn qolip_start_settings(&self) -> Result<QolipStartSettings, ProductionMapError> {
                Ok(self.qolip_start_settings.read().await.clone())
            }
            async fn update_qolip_start_settings(&self, enabled: bool, _actor: &QueueActionActor) -> Result<QolipStartSettings, ProductionMapError> {
                let mut settings = self.qolip_start_settings.write().await;
                if settings.automatic_confirmation_enabled != enabled {
                    settings.automatic_confirmation_enabled = enabled;
                    settings.revision += 1;
                }
                Ok(settings.clone())
            }
            async fn lamination_settings(&self) -> Result<LaminationSettings, ProductionMapError> {
                Ok(self.lamination_settings.read().await.clone())
            }
            async fn update_lamination_settings(
                &self,
                patch: super::LaminationLabelSettingsPatch,
                _actor: &QueueActionActor,
            ) -> Result<LaminationSettings, ProductionMapError> {
                let mut settings = self.lamination_settings.write().await;
                settings.apply(patch);
                Ok(settings.clone())
            }
            async fn paddon_management_settings(&self) -> Result<PaddonManagementSettings, ProductionMapError> {
                Ok(self.paddon_management_settings.read().await.clone())
            }
            async fn update_paddon_management_settings(&self, free_movement_enabled: Option<bool>, worker_visibility_enabled: Option<bool>, _actor: &QueueActionActor) -> Result<PaddonManagementSettings, ProductionMapError> {
                let mut settings = self.paddon_management_settings.write().await;
                if let Some(enabled) = free_movement_enabled { settings.free_movement_enabled = enabled; }
                if let Some(enabled) = worker_visibility_enabled { settings.worker_visibility_enabled = enabled; }
                Ok(settings.clone())
            }
            async fn paddons_for_creator(&self, limit: usize, selectable_only: bool, creator_ref: Option<&str>) -> Result<Vec<PaddonSummary>, ProductionMapError> {
                let mut paddons: Vec<_> = self.paddons.read().await.values()
                    .filter(|p| creator_ref.is_none_or(|creator| p.created_by_ref.trim() == creator)
                        && (!selectable_only || p.locked_at_unix.is_none())).cloned().collect();
                paddons.sort_by(|a, b| b.updated_at_unix.cmp(&a.updated_at_unix).then_with(|| a.code.cmp(&b.code)));
                paddons.truncate(limit);
                Ok(paddons)
            }
            async fn paddons_for_apparatus(&self, limit: usize, selectable_only: bool, creator_ref: Option<&str>, apparatus: Option<&str>) -> Result<Vec<PaddonSummary>, ProductionMapError> {
                let mut paddons: Vec<_> = self.paddons.read().await.values()
                    .filter(|p| creator_ref.is_none_or(|creator| p.created_by_ref.trim() == creator)
                        && apparatus.is_none_or(|id| p.apparatus_id == id)
                        && (!selectable_only || p.locked_at_unix.is_none())).cloned().collect();
                paddons.sort_by(|a, b| b.updated_at_unix.cmp(&a.updated_at_unix).then_with(|| a.code.cmp(&b.code)));
                paddons.truncate(limit);
                Ok(paddons)
            }
            async fn move_apparatus_sequence(
                &self,
                canonical: &crate::core::apparatus_standard::RuntimeApparatusConfiguration,
                command: &SequenceMove,
                actor: &QueueActionActor,
            ) -> Result<SequenceMoveResult, ProductionMapError> {
                command.validate()?;
                let mut receipts = self.sequence_move_receipts.lock().await;
                let key = (command.apparatus.clone(), actor.role.clone(), actor.ref_.clone(), command.idempotency_key.clone());
                if let Some((previous, result)) = receipts.get(&key) {
                    return if previous == command { Ok(result.clone()) }
                        else { Err(ProductionMapError::QueueReorderIdempotencyConflict) };
                }
                let maps = self.maps().await?;
                let states = self.apparatus_queue_states().await?;
                let frozen = self.order_control_states().await?.into_iter()
                    .filter_map(|(id,c)| (c.state == OrderControlState::Frozen).then_some(id)).collect();
                let holds = self.active_print_preflight_holds().await?.into_iter()
                    .filter(|h| h.apparatus == command.apparatus && h.is_live_at(0)).map(|h| h.order_id).collect();
                let mut sequences = self.sequences.write().await;
                let state = SequenceMoveState::from_data(canonical, &maps,
                    sequences.get(&command.apparatus).map(Vec::as_slice).unwrap_or_default(),
                    states.get(&command.apparatus).unwrap_or(&BTreeMap::new()), &frozen, &holds);
                let mut result = state.apply(command)?;
                if result.event.is_some() {
                    sequences.insert(command.apparatus.clone(), result.order_ids.clone());
                    result.revision = Some(1);
                }
                receipts.insert(key, (command.clone(), result.clone()));
                Ok(result)
            }
            async fn active_rezka_paddon(&self, apparatus: &str, actor: &QueueActionActor) -> Result<Option<String>, ProductionMapError> {
                let code = self.active_paddons.read().await.get(&(actor.role.clone(), actor.ref_.clone(), apparatus.to_string())).cloned();
                if let Some(code) = code.as_deref() {
                    if !self.paddons.read().await.get(code).is_some_and(|p| p.apparatus_id == apparatus && p.locked_at_unix.is_none()) { return Ok(None); }
                }
                if let Some(code) = code.as_deref().filter(|_| actor.role == "aparatchi") {
                    let owned = self.paddons.read().await.get(code).is_some_and(|p| p.created_by_ref.trim() == actor.ref_.trim());
                    if !owned && !self.paddon_management_settings.read().await.worker_visibility_enabled { return Ok(None); }
                }
                Ok(code)
            }
            async fn set_active_rezka_paddon(&self, apparatus: &str, actor: &QueueActionActor, code: Option<&str>) -> Result<(), ProductionMapError> {
                if let Some(code) = code {
                    let paddons = self.paddons.read().await;
                    let paddon = paddons.get(code).ok_or(ProductionMapError::PaddonNotFound)?;
                    if paddon.locked_at_unix.is_some() { return Err(ProductionMapError::PaddonLocked); }
                    if paddon.apparatus_id != apparatus { return Err(ProductionMapError::PaddonInvalidInput); }
                }
                let key = (actor.role.clone(), actor.ref_.clone(), apparatus.to_string());
                let mut selections = self.active_paddons.write().await;
                if let Some(code) = code { selections.insert(key, code.to_string()); } else { selections.remove(&key); }
                Ok(())
            }
            async fn create_paddon(&self, input: PaddonCreateInput) -> Result<PaddonSummary, ProductionMapError> {
                let mut paddons = self.paddons.write().await;
                let code = format!("{:05}", paddons.len() + 1);
                let paddon = PaddonSummary {
                    id: code.clone(), code: code.clone(), apparatus_id: input.apparatus_id, location: input.location, note: input.note,
                    created_by_ref: input.actor_ref, created_by_display_name: input.actor_display_name,
                    created_at_unix: 0, updated_at_unix: 0, item_count: 0, contains_peremotka: false,
                    total_gross_kg: Some(0.0), total_net_kg: Some(0.0), locked_at_unix: None,
                };
                paddons.insert(code, paddon.clone());
                Ok(paddon)
            }
            async fn confirm_paddon_print(&self, code: &str, actor: &QueueActionActor) -> Result<PaddonPrintConfirmation, ProductionMapError> {
                let mut paddons = self.paddons.write().await;
                let paddon = paddons.get_mut(code).ok_or(ProductionMapError::PaddonNotFound)?;
                let newly_locked = paddon.locked_at_unix.is_none();
                if newly_locked {
                    self.paddon_lock_owners.write().await.insert(code.to_string(), actor.ref_.trim().to_string());
                }
                paddon.locked_at_unix.get_or_insert(super::progress::unix_seconds());
                let mut active = self.active_paddons.write().await;
                let apparatuses = active.iter().filter(|((role, ref_, _), selected)|
                    role == &actor.role && ref_ == &actor.ref_ && selected.as_str() == code)
                    .map(|((_, _, apparatus), _)| apparatus.clone()).collect();
                active.retain(|_, selected| selected != code);
                Ok(PaddonPrintConfirmation { paddon: paddon.clone(), newly_locked, apparatuses })
            }
            async fn can_unlock_paddon(&self, code: &str, actor: &QueueActionActor) -> Result<bool, ProductionMapError> {
                let paddons = self.paddons.read().await;
                let paddon = paddons.get(code).ok_or(ProductionMapError::PaddonNotFound)?;
                if paddon.locked_at_unix.is_none() { return Ok(false); }
                let owners = self.paddon_lock_owners.read().await;
                let enabled = self.paddon_management_settings.read().await.free_movement_enabled;
                Ok(paddon_unlock_actor_allowed(owners.get(code).map(String::as_str).unwrap_or_default(), enabled, actor))
            }
            async fn unlock_paddon(&self, code: &str, actor: &QueueActionActor) -> Result<PaddonSummary, ProductionMapError> {
                let mut successors = self.paddon_successors.lock().await;
                let mut paddons = self.paddons.write().await;
                let paddon = paddons.get_mut(code).ok_or(ProductionMapError::PaddonNotFound)?;
                if paddon.locked_at_unix.is_some() {
                    let mut owners = self.paddon_lock_owners.write().await;
                    let enabled = self.paddon_management_settings.read().await.free_movement_enabled;
                    if !paddon_unlock_actor_allowed(owners.get(code).map(String::as_str).unwrap_or_default(), enabled, actor) {
                        return Err(ProductionMapError::PaddonUnlockForbidden);
                    }
                    paddon.locked_at_unix = None;
                    paddon.updated_at_unix = super::progress::unix_seconds();
                    owners.remove(code);
                    successors.retain(|(source, _, _, _), _| source != code);
                }
                Ok(paddon.clone())
            }
            async fn paddon_snapshot(&self, code: &str) -> Result<Option<PaddonSnapshot>, ProductionMapError> {
                let Some(paddon) = self.paddons.read().await.get(code).cloned() else { return Ok(None); };
                let enabled = self.paddon_management_settings.read().await.free_movement_enabled;
                Ok(Some(PaddonSnapshot {
                    can_manage_items: paddon.locked_at_unix.is_none(),
                    paddon, items: Vec::new(), available_items: Vec::new(), free_movement_enabled: enabled,
                }))
            }
            async fn paddon_scan_snapshot(&self, code: &str) -> Result<Option<PaddonSnapshot>, ProductionMapError> {
                self.paddon_snapshot(code).await
            }
            async fn paddon_summary(&self, code: &str) -> Result<Option<PaddonSummary>, ProductionMapError> {
                Ok(self.paddons.read().await.get(code).cloned())
            }
            async fn create_active_paddon_successor(&self, code: &str, apparatus: &str, actor: &QueueActionActor) -> Result<PaddonSummary, ProductionMapError> {
                let mut successors = self.paddon_successors.lock().await;
                let key = (code.to_string(), actor.role.clone(), actor.ref_.clone(), apparatus.to_string());
                if let Some(next) = successors.get(&key) {
                    self.set_active_rezka_paddon(apparatus, actor, Some(next)).await?;
                    return self.paddons.read().await.get(next).cloned().ok_or(ProductionMapError::PaddonNotFound);
                }
                let source = self.paddons.read().await.get(code).cloned().ok_or(ProductionMapError::PaddonNotFound)?;
                if source.locked_at_unix.is_none() || source.apparatus_id != apparatus {
                    return Err(ProductionMapError::PaddonInvalidInput);
                }
                let paddon = self.create_paddon(PaddonCreateInput { apparatus_id: apparatus.to_string(), location: apparatus.to_string(), note: String::new(),
                    actor_ref: actor.ref_.clone(), actor_display_name: actor.display_name.clone() }).await?;
                self.set_active_rezka_paddon(apparatus, actor, Some(&paddon.code)).await?;
                successors.insert(key, paddon.code.clone());
                Ok(paddon)
            }
            async fn rezka_astatka_commit_by_request_id(
                &self, actor: &QueueActionActor, request_id: &str,
            ) -> Result<Option<(serde_json::Value, RezkaAstatkaReport)>, ProductionMapError> {
                Ok(self.rezka_astatka_receipts.lock().await
                    .get(&(actor.role.clone(), actor.ref_.clone(), request_id.to_string())).cloned())
            }
            async fn commit_rezka_astatka_report(
                &self, request_id: String, request_json: serde_json::Value,
                report: RezkaAstatkaReport, expected_session: OrderRunSession, actor: QueueActionActor,
            ) -> Result<RezkaAstatkaReport, ProductionMapError> {
                let mut receipts = self.rezka_astatka_receipts.lock().await;
                let key = (actor.role.clone(), actor.ref_.clone(), request_id);
                if let Some((previous, report)) = receipts.get(&key) {
                    return if previous == &request_json { Ok(report.clone()) }
                        else { Err(ProductionMapError::ProgressInputInvalid) };
                }
                self.commit_stage_astatka_report(StageAstatkaReport::Cut(report.clone()), Some(expected_session), actor).await?;
                receipts.insert(key, (request_json, report.clone()));
                Ok(report)
            }
            async fn commit_stage_astatka_report(&self, report: StageAstatkaReport,
                expected: Option<OrderRunSession>, actor: QueueActionActor) -> Result<(), ProductionMapError> {
                let (order_id, apparatus, report_id) = report.identity();
                let (order_id, apparatus, report_id) = (order_id.to_string(), apparatus.to_string(), report_id.to_string());
                let mut sessions = self.order_run_sessions.write().await;
                let current = sessions.values().filter(|s| s.order_id == order_id && s.apparatus == apparatus)
                    .max_by(|a, b| (a.started_at_unix, &a.session_id).cmp(&(b.started_at_unix, &b.session_id)));
                if current != expected.as_ref() { return Err(ProductionMapError::QueueActionNotAllowed); }
                match report {
                    StageAstatkaReport::Bosma(r) => { self.bosma_astatka_reports.write().await.push(r); }
                    StageAstatkaReport::Laminate(r) => self.put_laminatsiya_astatka_report(r).await?,
                    StageAstatkaReport::Cut(r) => self.put_rezka_astatka_report(r).await?,
                }
                if let Some(mut session) = expected.filter(|s| s.status == OrderRunStatus::Completed) {
                    let sequence = sessions.values().filter(|s| s.order_id == order_id)
                        .filter_map(super::stage_execution::work_report).map(|r| r.sequence).max().unwrap_or(0) + 1;
                    super::stage_execution::stamp_work_report(&mut session, &report_id, sequence, &actor, super::stage_execution::report_now());
                    sessions.insert(session.session_id.clone(), session);
                }
                drop(sessions);
                queue::refresh_production_order_lifecycles(self, &[order_id]).await
            }
            $($following)*
        }
    };
}
