impl PostgresProductionMapStore {
    async fn maps(&self) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
        load_maps(&self.pool).await
    }

    async fn maps_by_lifecycle_statuses(
        &self,
        statuses: &[ProductionOrderLifecycleStatus],
    ) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
        load_maps_by_lifecycle_statuses(&self.pool, statuses).await
    }

    async fn raw_material_assignment_maps(
        &self,
    ) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
        load_raw_material_assignment_maps(&self.pool).await
    }

    async fn production_order_lifecycles(
        &self,
        order_ids: &[String],
    ) -> Result<BTreeMap<String, ProductionOrderLifecycleRecord>, ProductionMapError> {
        load_production_order_lifecycles(&self.pool, order_ids).await
    }

    async fn next_order_number(&self) -> Result<String, ProductionMapError> {
        sqlx::query_scalar::<_, Option<String>>(
            "SELECT CASE
                WHEN is_called AND last_value >= 9999 THEN NULL
                ELSE lpad(nextval('mini_production_order_number_seq')::text, 4, '0')
             END
             FROM mini_production_order_number_seq",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?
        .ok_or(ProductionMapError::OrderNumberExhausted)
    }

    async fn put_map(&self, map: ProductionMapDefinition) -> Result<(), ProductionMapError> {
        reject_order_number_immutable(&self.pool, &map).await?;
        reject_duplicate_order_number(&self.pool, &map).await?;
        put_map_inner(&self.pool, &map).await
    }

    async fn put_maps_batch(
        &self,
        maps: &[ProductionMapDefinition],
    ) -> Result<(), ProductionMapError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        for map in maps {
            reject_order_number_immutable_tx(&mut tx, map).await?;
            reject_duplicate_order_number_tx(&mut tx, map).await?;
            put_map_inner_tx(&mut tx, map).await?;
        }
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)
    }

    async fn put_maps_with_sequences(
        &self,
        maps: &[ProductionMapDefinition],
        expected_sequences: &BTreeMap<String, Vec<String>>,
        sequences: &BTreeMap<String, Vec<String>>,
    ) -> Result<(), ProductionMapError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let order_ids = maps.iter().map(|map| map.id.as_str()).collect::<Vec<_>>();
        let apparatus_ids = sequences.keys().map(String::as_str).collect::<Vec<_>>();
        lock_orders_and_apparatuses_tx(&mut tx, &order_ids, &apparatus_ids).await?;
        for (apparatus, expected) in expected_sequences {
            let payload = sqlx::query_scalar::<_, serde_json::Value>(
                "SELECT order_ids FROM mini_queue_sequences WHERE canonical_apparatus_id = $1",
            )
            .bind(apparatus)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
            let current = payload
                .map(serde_json::from_value::<Vec<String>>)
                .transpose()
                .map_err(|_| ProductionMapError::StoreFailed)?
                .unwrap_or_default();
            if current != *expected {
                return Err(ProductionMapError::QueueReorderConflict);
            }
        }
        for map in maps {
            reject_order_number_immutable_tx(&mut tx, map).await?;
            reject_duplicate_order_number_tx(&mut tx, map).await?;
            put_map_inner_tx(&mut tx, map).await?;
        }
        for (apparatus, order_ids) in sequences {
            catalog_helpers::save_apparatus_sequence_tx(&mut tx, apparatus, order_ids).await?;
        }
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)
    }

    async fn delete_map(&self, map_id: &str) -> Result<(), ProductionMapError> {
        delete_map_by_id(&self.pool, map_id).await
    }

    async fn order_control_states(
        &self,
    ) -> Result<BTreeMap<String, OrderControlRecord>, ProductionMapError> {
        load_order_control_states(&self.pool).await
    }

    async fn order_freeze_requests_for_audit(
        &self,
    ) -> Result<Vec<crate::core::production_map::OrderFreezeAuditRecord>, ProductionMapError> {
        load_order_freeze_requests_for_audit(&self.pool).await
    }

    async fn put_order_control_state(
        &self,
        record: OrderControlRecord,
    ) -> Result<(), ProductionMapError> {
        save_order_control_state(&self.pool, &record).await
    }

    async fn apparatus_sequences(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
        load_apparatus_sequences(&self.pool).await
    }

    async fn put_apparatus_sequence(
        &self,
        apparatus: &str,
        order_ids: Vec<String>,
    ) -> Result<(), ProductionMapError> {
        save_apparatus_sequence(&self.pool, apparatus, order_ids).await
    }

    pub(crate) async fn queue_events_replay(
        &self,
        apparatus: &str,
        from_rev: i64,
        to_rev: i64,
    ) -> Result<Vec<serde_json::Value>, ProductionMapError> {
        let events = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT json_build_object(
                'id', id,
                'canonical_apparatus_id', canonical_apparatus_id,
                'revision', revision,
                'base_revision', base_revision,
                'base_version', ops->0->>'base_version',
                'version', ops->0->>'version',
                'event_type', event_type,
                'ops', ops,
                'created_at', created_at
             )
             FROM mini_queue_events
             WHERE canonical_apparatus_id = $1 AND revision >= $2 AND revision <= $3
             ORDER BY revision ASC LIMIT 200",
        )
        .bind(apparatus)
        .bind(from_rev)
        .bind(to_rev)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        Ok(events)
    }

    async fn apparatus_downtimes(&self) -> Result<Vec<ApparatusDowntime>, ProductionMapError> {
        load_apparatus_downtimes(&self.pool).await
    }

    async fn put_apparatus_downtime(
        &self,
        downtime: ApparatusDowntime,
    ) -> Result<(), ProductionMapError> {
        put_apparatus_downtime(&self.pool, downtime).await
    }

    async fn apparatus_schedule_reservations(
        &self,
    ) -> Result<Vec<ApparatusScheduleReservation>, ProductionMapError> {
        load_apparatus_schedule_reservations(&self.pool).await
    }

    async fn apparatus_schedule_reservation_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<ApparatusScheduleReservation>, ProductionMapError> {
        load_apparatus_schedule_reservation_by_idempotency_key(&self.pool, idempotency_key).await
    }

    async fn put_apparatus_schedule_reservation(
        &self,
        reservation: ApparatusScheduleReservation,
        capacity_slots: u16,
        finite_capacity: bool,
    ) -> Result<ApparatusScheduleReservation, ProductionMapError> {
        put_apparatus_schedule_reservation(&self.pool, reservation, capacity_slots, finite_capacity)
            .await
    }

    async fn cancel_apparatus_schedule_reservation(
        &self,
        input: ApparatusScheduleCancelRequest,
    ) -> Result<ApparatusScheduleReservation, ProductionMapError> {
        cancel_apparatus_schedule_reservation(&self.pool, input).await
    }

    async fn update_apparatus_schedule_reservation_status(
        &self,
        order_id: &str,
        apparatus_id: &ApparatusId,
        status: crate::core::production_map::ApparatusScheduleStatus,
        actor: &QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        update_apparatus_schedule_reservation_status_tx(
            &mut tx,
            order_id,
            apparatus_id,
            status,
            actor,
        )
        .await?;
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)
    }

    async fn active_print_preflight_holds(
        &self,
    ) -> Result<Vec<crate::core::production_map::PrintPreflightHold>, ProductionMapError> {
        load_active_print_preflight_holds(&self.pool).await
    }

    async fn print_preflight_hold_by_id(
        &self,
        hold_id: &str,
    ) -> Result<Option<crate::core::production_map::PrintPreflightHold>, ProductionMapError> {
        load_print_preflight_hold_by_id(&self.pool, hold_id).await
    }

    async fn print_preflight_hold_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<crate::core::production_map::PrintPreflightHold>, ProductionMapError> {
        load_print_preflight_hold_by_idempotency_key(&self.pool, idempotency_key).await
    }

    async fn put_print_preflight_hold(
        &self,
        hold: crate::core::production_map::PrintPreflightHold,
    ) -> Result<(), ProductionMapError> {
        put_print_preflight_hold(&self.pool, &hold).await
    }

    async fn update_print_preflight_hold(
        &self,
        hold: crate::core::production_map::PrintPreflightHold,
    ) -> Result<(), ProductionMapError> {
        update_print_preflight_hold(&self.pool, &hold).await
    }

    async fn consume_print_preflight_hold(
        &self,
        hold_id: &str,
        order_id: &str,
        apparatus: &str,
        actor: &QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        consume_print_preflight_hold_tx(&mut tx, hold_id, order_id, apparatus, actor).await?;
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)
    }

    async fn cancel_print_preflight_hold(
        &self,
        hold_id: &str,
        order_id: &str,
        apparatus: &str,
        actor: &QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        cancel_print_preflight_hold_tx(&mut tx, hold_id, order_id, apparatus, actor).await?;
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)
    }

    async fn apparatus_queue_states(
        &self,
    ) -> Result<BTreeMap<String, BTreeMap<String, String>>, ProductionMapError> {
        load_apparatus_queue_states(&self.pool).await
    }

    async fn put_apparatus_queue_states(
        &self,
        apparatus: &str,
        states: BTreeMap<String, String>,
    ) -> Result<(), ProductionMapError> {
        let apparatus = apparatus.trim();
        let order_ids = states.keys().cloned().collect::<Vec<_>>();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        put_queue_states_tx(&mut tx, apparatus, states).await?;
        let actor = QueueActionActor {
            role: "system".to_string(),
            ref_: "queue-state-sync".to_string(),
            display_name: "Queue state sync".to_string(),
        };
        for order_id in order_ids {
            refresh_production_order_lifecycle_tx(
                &mut tx,
                &order_id,
                &actor,
                "",
                "queue_state_sync",
            )
            .await?;
        }
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)
    }

    async fn put_apparatus_queue_states_with_event(
        &self,
        apparatus: &str,
        _states: BTreeMap<String, String>,
        event: ApparatusQueueActionEvent,
    ) -> Result<(), ProductionMapError> {
        let apparatus = apparatus.trim();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        lock_order_and_apparatuses_tx(&mut tx, &event.order_id, &[apparatus, &event.apparatus])
            .await?;
        if queue_action_event_replay_tx(&mut tx, &event).await? {
            tx.commit()
                .await
                .map_err(|_| ProductionMapError::StoreFailed)?;
            return Ok(());
        }
        validate_queue_action_event_transition_tx(&mut tx, &event).await?;
        put_queue_action_state_tx(&mut tx, &event).await?;
        insert_queue_action_event_tx(&mut tx, &event).await?;
        refresh_production_order_lifecycle_tx(
            &mut tx,
            &event.order_id,
            &event.actor,
            &event.event_id,
            "queue_action",
        )
        .await?;
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)
    }

    async fn append_apparatus_queue_action_event(
        &self,
        event: ApparatusQueueActionEvent,
    ) -> Result<(), ProductionMapError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        lock_order_and_apparatuses_tx(&mut tx, &event.order_id, &[&event.apparatus]).await?;
        if queue_action_event_replay_tx(&mut tx, &event).await? {
            tx.commit()
                .await
                .map_err(|_| ProductionMapError::StoreFailed)?;
            return Ok(());
        }
        insert_queue_action_event_tx(&mut tx, &event).await?;
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)
    }

    async fn completed_queue_orders_for_actor(
        &self,
        actor_ref: &str,
        limit: usize,
    ) -> Result<Vec<CompletedQueueOrder>, ProductionMapError> {
        load_completed_queue_orders_for_actor(&self.pool, actor_ref, limit).await
    }

    async fn completed_queue_orders_for_apparatuses(
        &self,
        apparatuses: &[String],
        limit: usize,
    ) -> Result<Vec<CompletedQueueOrder>, ProductionMapError> {
        load_completed_queue_orders_for_apparatuses(&self.pool, apparatuses, limit).await
    }

    async fn completion_requests(
        &self,
        limit: usize,
    ) -> Result<Vec<CompletionRequestNotification>, ProductionMapError> {
        load_completion_requests(&self.pool, limit).await
    }

    async fn completion_request_by_event_id(
        &self,
        event_id: &str,
    ) -> Result<Option<CompletionRequestNotification>, ProductionMapError> {
        load_completion_request_by_event_id(&self.pool, event_id).await
    }

    async fn completion_request_decisions_for_actor(
        &self,
        actor_ref: &str,
        limit: usize,
    ) -> Result<Vec<CompletionRequestDecisionNotification>, ProductionMapError> {
        load_completion_request_decisions_for_actor(&self.pool, actor_ref, limit).await
    }

    async fn resolve_completion_request_decision(
        &self,
        request_event_id: &str,
        decision: CompletionRequestDecision,
        actor: &QueueActionActor,
        notification: &CompletionRequestDecisionNotification,
        state_resolution: Option<CompletionRequestStateResolution>,
    ) -> Result<QueueActionProgressWriteResult, ProductionMapError> {
        resolve_completion_request(
            &self.pool,
            request_event_id,
            decision,
            actor,
            notification,
            state_resolution,
        )
        .await
    }
}
