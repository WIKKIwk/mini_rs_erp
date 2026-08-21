
impl ProductionMapService {
    pub async fn audit_production_workflow(
        &self,
    ) -> Result<ProductionWorkflowAuditReport, ProductionMapError> {
        let maps = self.store.maps().await?;
        let maps_by_id = maps
            .iter()
            .filter_map(|map| {
                let id = map.id.trim();
                (!id.is_empty()).then(|| (id.to_string(), map))
            })
            .collect::<BTreeMap<_, _>>();
        let known_orders = maps_by_id.keys().cloned().collect::<BTreeSet<_>>();
        let queue_states = self.store.apparatus_queue_states().await?;
        let sequences = self.store.apparatus_sequences().await?;
        let mut violations = Vec::new();
        let mut qr_owners = BTreeMap::<String, (String, Vec<(String, String)>)>::new();
        let mut active_sessions = BTreeMap::<(String, String), Vec<String>>::new();
        let mut active_queue_orders = BTreeMap::<String, Vec<String>>::new();

        audit_queue_states(
            &known_orders,
            &maps,
            &queue_states,
            &mut active_queue_orders,
            &mut violations,
        );
        audit_sequences(&known_orders, &maps, &sequences, &mut violations);

        for (order_id, apparatuses) in active_queue_orders {
            if apparatuses.len() > 1 {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "duplicate_active_queue_assignment",
                    &order_id,
                    &apparatuses.join(","),
                    "an order is active or paused on more than one apparatus",
                ));
            }
        }

        let sessions = self.store.order_run_sessions_for_audit().await?;
        let sessions_by_id = sessions
            .iter()
            .filter_map(|session| {
                let id = session.session_id.trim();
                (!id.is_empty()).then(|| (id.to_string(), session))
            })
            .collect::<BTreeMap<_, _>>();
        let mut checked_session_count = 0;
        for session in &sessions {
            checked_session_count += 1;
            audit_session(
                &known_orders,
                &maps_by_id,
                &queue_states,
                session,
                &mut active_sessions,
                &mut violations,
            );
        }

        let stored_batches = self.store.progress_batches_for_audit().await?;
        let mut batches_by_id = BTreeMap::<String, OrderProgressBatch>::new();
        for batch in stored_batches {
            let batch_id = batch.batch_id.trim().to_string();
            if batch_id.is_empty() {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "blank_progress_batch_id",
                    batch.order_id.trim(),
                    "",
                    "every progress batch must have a stable batch_id",
                ));
                continue;
            }
            if batches_by_id.insert(batch_id.clone(), batch).is_some() {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "duplicate_progress_batch_id",
                    "",
                    &batch_id,
                    "progress batch ids must be unique in the audit source",
                ));
            }
        }

        for batch in batches_by_id.values() {
            audit_progress_batch(
                &known_orders,
                &maps_by_id,
                &sessions_by_id,
                &batches_by_id,
                batch,
                &mut violations,
            );
            let qr = batch.qr_payload.trim();
            if !qr.is_empty() {
                qr_owners
                    .entry(qr.to_ascii_lowercase())
                    .or_insert_with(|| (qr.to_string(), Vec::new()))
                    .1
                    .push((
                        batch.order_id.trim().to_string(),
                        batch.batch_id.trim().to_string(),
                    ));
            }
        }

        audit_paused_session_progress(&sessions, &batches_by_id, &mut violations);

        for (qr_payload, owners) in qr_owners.values() {
            if owners.len() <= 1 {
                continue;
            }
            let batches = owners
                .iter()
                .map(|(_, batch_id)| batch_id.as_str())
                .collect::<Vec<_>>()
                .join(",");
            let order_id = owners
                .iter()
                .map(|(order_id, _)| order_id.as_str())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join(",");
            violations.push(ProductionWorkflowAuditViolation::new(
                "duplicate_qr_payload",
                &order_id,
                qr_payload,
                &format!("duplicate progress QR is used by batches: {batches}"),
            ));
        }

        for ((apparatus, order_id), sessions) in active_sessions {
            if sessions.len() <= 1 {
                continue;
            }
            violations.push(ProductionWorkflowAuditViolation::new(
                "duplicate_active_order_session",
                &order_id,
                &apparatus,
                &format!(
                    "more than one active or paused session exists: {}",
                    sessions.join(",")
                ),
            ));
        }

        audit_transfers(
            &known_orders,
            &maps_by_id,
            &queue_states,
            &self.store.apparatus_transfers_for_audit().await?,
            &mut violations,
        );
        let capacity_snapshot = self.apparatus_capacity_snapshot().await?;
        audit_capacity(
            &known_orders,
            &capacity_snapshot.profiles,
            &capacity_snapshot.downtimes,
            &capacity_snapshot.reservations,
            &mut violations,
        );

        Ok(ProductionWorkflowAuditReport {
            ok: violations.is_empty(),
            checked_order_count: known_orders.len(),
            checked_batch_count: batches_by_id.len(),
            checked_session_count,
            violations,
        })
    }
}

fn audit_queue_states(
    known_orders: &BTreeSet<String>,
    maps: &[ProductionMapDefinition],
    queue_states: &BTreeMap<String, BTreeMap<String, String>>,
    active_orders: &mut BTreeMap<String, Vec<String>>,
    violations: &mut Vec<ProductionWorkflowAuditViolation>,
) {
    for (apparatus, states) in queue_states {
        let apparatus = apparatus.trim();
        if apparatus.is_empty() {
            violations.push(ProductionWorkflowAuditViolation::new(
                "blank_queue_apparatus",
                "",
                "",
                "queue state groups must identify an apparatus",
            ));
        }
        let visible_order_ids = visible_order_ids_for_apparatus(maps, apparatus)
            .into_iter()
            .collect::<BTreeSet<_>>();
        for (order_id, raw_state) in states {
            let order_id = order_id.trim();
            if order_id.is_empty() {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "blank_queue_order",
                    "",
                    apparatus,
                    "queue states must not contain an empty order id",
                ));
                continue;
            }
            if !known_orders.contains(order_id) {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "unknown_order_queue_state",
                    order_id,
                    apparatus,
                    "queue state references an order that is not present in production maps",
                ));
            }
            let Some(state) = ApparatusQueueOrderState::parse(raw_state) else {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "invalid_queue_state",
                    order_id,
                    apparatus,
                    "queue state must be pending, in_progress, paused, frozen, or completed",
                ));
                continue;
            };
            if known_orders.contains(order_id) && !visible_order_ids.contains(order_id) {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "queue_order_apparatus_mismatch",
                    order_id,
                    apparatus,
                    "queue state is stored on an apparatus that is not a stage of the order",
                ));
            }
            if state.is_active() {
                active_orders
                    .entry(order_id.to_string())
                    .or_default()
                    .push(apparatus.to_string());
            }
        }
    }
}

fn audit_sequences(
    known_orders: &BTreeSet<String>,
    maps: &[ProductionMapDefinition],
    sequences: &BTreeMap<String, Vec<String>>,
    violations: &mut Vec<ProductionWorkflowAuditViolation>,
) {
    for (apparatus, sequence) in sequences {
        let visible_order_ids = visible_order_ids_for_apparatus(maps, apparatus)
            .into_iter()
            .collect::<BTreeSet<_>>();
        let mut seen = BTreeSet::new();
        for order_id in sequence {
            let order_id = order_id.trim();
            if order_id.is_empty() {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "blank_queue_sequence_order",
                    "",
                    apparatus,
                    "queue sequence must not contain an empty order id",
                ));
                continue;
            }
            if !seen.insert(order_id.to_string()) {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "duplicate_queue_sequence_order",
                    order_id,
                    apparatus,
                    "an order appears more than once in an apparatus sequence",
                ));
            }
            if !known_orders.contains(order_id) {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "unknown_order_queue_sequence",
                    order_id,
                    apparatus,
                    "queue sequence references an order that is not present in production maps",
                ));
            } else if !visible_order_ids.contains(order_id) {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "queue_sequence_apparatus_mismatch",
                    order_id,
                    apparatus,
                    "queue sequence contains an order that is not a stage of the order",
                ));
            }
        }
    }
}

fn audit_session(
    known_orders: &BTreeSet<String>,
    maps_by_id: &BTreeMap<String, &ProductionMapDefinition>,
    queue_states: &BTreeMap<String, BTreeMap<String, String>>,
    session: &OrderRunSession,
    active_sessions: &mut BTreeMap<(String, String), Vec<String>>,
    violations: &mut Vec<ProductionWorkflowAuditViolation>,
) {
    let order_id = session.order_id.trim();
    let session_id = session.session_id.trim();
    let apparatus = session.apparatus.trim();
    if session_id.is_empty() {
        violations.push(ProductionWorkflowAuditViolation::new(
            "blank_run_session_id",
            order_id,
            apparatus,
            "every run session must have a stable session_id",
        ));
    }
    if apparatus.is_empty() {
        violations.push(ProductionWorkflowAuditViolation::new(
            "blank_run_session_apparatus",
            order_id,
            session_id,
            "every run session must identify an apparatus",
        ));
    }
    if !known_orders.contains(order_id) {
        violations.push(ProductionWorkflowAuditViolation::new(
            "unknown_order_run_session",
            order_id,
            session_id,
            "run session references an order that is not present in production maps",
        ));
    }
    let is_requeued = session.status == OrderRunStatus::Paused
        && session
            .payload_json
            .get("requeued_at_tail")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
    if !is_requeued
        && matches!(
            session.status,
            OrderRunStatus::Active
                | OrderRunStatus::Paused
                | OrderRunStatus::Frozen
                | OrderRunStatus::RollDetached
        )
    {
        active_sessions
            .entry((apparatus.to_ascii_lowercase(), order_id.to_string()))
            .or_default()
            .push(session_id.to_string());
    }
    let Some(map) = maps_by_id.get(order_id) else {
        return;
    };
    if !is_requeued
        && !chain::map_has_work_stage_for_station(map, apparatus)
        && matches!(
            session.status,
            OrderRunStatus::Active
                | OrderRunStatus::Paused
                | OrderRunStatus::Frozen
                | OrderRunStatus::RollDetached
        )
    {
        violations.push(ProductionWorkflowAuditViolation::new(
            "run_session_apparatus_mismatch",
            order_id,
            apparatus,
            "active or paused run session is attached to an apparatus outside the order route",
        ));
    }
    let state = queue_state_for_apparatus_order(queue_states, apparatus, order_id);
    let expected = match session.status {
        OrderRunStatus::Active => Some(ApparatusQueueOrderState::InProgress),
        OrderRunStatus::Paused if is_requeued => None,
        OrderRunStatus::Paused | OrderRunStatus::RollDetached => {
            Some(ApparatusQueueOrderState::Paused)
        }
        OrderRunStatus::Frozen => Some(ApparatusQueueOrderState::Frozen),
        OrderRunStatus::Completed => None,
    };
    if let Some(expected) = expected {
        if state != Some(expected) {
            violations.push(ProductionWorkflowAuditViolation::new(
                "session_queue_state_mismatch",
                order_id,
                apparatus,
                &format!(
                    "{} session requires queue state {}",
                    session.status.as_str(),
                    expected.as_str()
                ),
            ));
        }
    } else if state.is_some_and(ApparatusQueueOrderState::is_active) {
        violations.push(ProductionWorkflowAuditViolation::new(
            "completed_session_active_queue",
            order_id,
            apparatus,
            "completed run session cannot leave an active queue state",
        ));
    }
}
