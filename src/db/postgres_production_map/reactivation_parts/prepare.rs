async fn prepare(
    tx: &mut Transaction<'_, Postgres>,
    selector: &ReactivationSelector,
    actor: &QueueActionActor,
    is_print: bool,
    is_lamination: bool,
) -> Result<Result<Prepared, &'static str>, ProductionMapError> {
    transaction_locks::lock_order_and_apparatuses_tx(
        tx,
        &selector.order_id,
        &[&selector.apparatus],
    )
    .await?;
    let apparatus_config:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(a) FROM mini_apparatus a WHERE id=$1 AND lifecycle_state='active' FOR SHARE")
        .bind(&selector.apparatus).fetch_optional(&mut **tx).await.map_err(|_|ProductionMapError::StoreFailed)?;
    let Some(apparatus_config) = apparatus_config else {
        deny!("stale_target");
    };
    let Some((map_json, lifecycle)) = sqlx::query_as::<_, (Value, String)>(
        "SELECT map_json,lifecycle_status FROM mini_production_maps WHERE id=$1 FOR UPDATE",
    )
    .bind(&selector.order_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    else {
        deny!("stale_target");
    };
    if matches!(lifecycle.as_str(), "closed" | "cancelled") {
        deny!("order_closed");
    }
    let map: ProductionMapDefinition =
        serde_json::from_value(map_json.clone()).map_err(|_| ProductionMapError::StoreFailed)?;
    let control: Option<(String, Option<Value>)> = sqlx::query_as(
        "SELECT state,early_close FROM mini_order_control_states WHERE order_id=$1 FOR UPDATE",
    )
    .bind(&selector.order_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if control.is_some_and(|(state, early)| state != "active" || early.is_some()) {
        deny!("order_closed");
    }
    let busy: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_queue_states WHERE canonical_apparatus_id=$1 AND state IN ('in_progress','print_preflight')) OR EXISTS(SELECT 1 FROM mini_order_run_sessions WHERE canonical_apparatus_id=$1 AND status='active') OR EXISTS(SELECT 1 FROM mini_print_preflight_holds WHERE canonical_apparatus_id=$1 AND status IN ('held','running','passed'))")
        .bind(&selector.apparatus).fetch_one(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    if busy {
        deny!("apparatus_busy");
    }
    let sessions = stage_execution::sessions_tx(tx, &selector.order_id).await?;
    if sessions
        .iter()
        .any(|s| s.apparatus == selector.apparatus && s.status.is_open())
    {
        deny!("apparatus_busy");
    }
    let mut selected_output = None;
    let selected = if is_print {
        if !selector.batch_id.is_empty() {
            deny!("stale_target");
        }
        let candidates = sessions
            .iter()
            .filter(|s| {
                s.apparatus == selector.apparatus
                    && s.status == OrderRunStatus::Completed
                    && s.payload_json.get("_reactivation_reclosed_without_work").and_then(Value::as_bool) != Some(true)
                    && s.worker_role == "aparatchi"
                    && !s.worker_ref.trim().is_empty()
            })
            .collect::<Vec<_>>();
        if candidates
            .iter()
            .map(|s| s.stage_node_id.as_str())
            .collect::<BTreeSet<_>>()
            .len()
            != 1
        {
            deny!("unsupported_history");
        }
        candidates
            .into_iter()
            .max_by_key(|s| (s.started_at_unix, &s.session_id))
            .cloned()
    } else {
        if selector.batch_id.is_empty() {
            deny!("stale_target");
        }
        let Some(batch) =
            order_query_helpers::load_progress_batch(&mut **tx, &selector.batch_id).await?
        else {
            deny!("stale_target");
        };
        if batch.order_id != selector.order_id || batch.apparatus != selector.apparatus {
            deny!("stale_target");
        }
        if batch.is_layer_remainder() {
            deny!("unsupported_history");
        }
        selected_output = Some(json!({
            "batch_id":batch.batch_id,"session_id":batch.session_id,"order_id":batch.order_id,
            "apparatus":batch.apparatus,"qr_payload":batch.qr_payload,"produced_qty":batch.produced_qty,
            "stage_node_id":batch.payload_json.get("stage_node_id"),
            "finished_goods_meter":batch.finished_goods_meter,"finished_goods_kg":batch.finished_goods_kg,
            "bobina_kg":batch.bobina_kg,"total_waste":batch.total_waste,
        }));
        sessions
            .iter()
            .find(|s| s.session_id == batch.session_id)
            .cloned()
    };
    let Some(session) = selected else {
        deny!("stale_target");
    };
    let allowed_worker = session.worker_role == "aparatchi"
        || (!is_print && !is_lamination && session.worker_role == "rezka_master");
    if session.status != OrderRunStatus::Completed
        || !allowed_worker
        || session.worker_ref.trim().is_empty()
        || session.stage_node_id.is_empty()
    {
        deny!("unsupported_history");
    }
    if sessions.iter().any(|s| {
        s.payload_json
            .get("_reactivation_of_session_id")
            .and_then(Value::as_str)
            == Some(session.session_id.as_str())
            && s.payload_json.get("_reactivation_reclosed_without_work").and_then(Value::as_bool) != Some(true)
    }) {
        deny!("stale_target");
    }
    if chain::work_stage_for_station(&map, &selector.apparatus, &session.stage_node_id)
        .is_none_or(|stage| stage.node_id != session.stage_node_id)
    {
        deny!("stale_target");
    }
    let states: std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>> = {
        let rows = sqlx::query_as::<_, (String, String, String)>(
            "SELECT canonical_apparatus_id,order_id,state FROM mini_queue_states WHERE order_id=$1",
        )
        .bind(&selector.order_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        let mut result = std::collections::BTreeMap::new();
        for (apparatus, order, state) in rows {
            result
                .entry(apparatus)
                .or_insert_with(std::collections::BTreeMap::new)
                .insert(order, state);
        }
        result
    };
    let events = sqlx::query_as::<_,(String,String,String)>("SELECT stage_node_id,action,to_state FROM mini_queue_action_events WHERE order_id=$1 AND stage_node_id<>'' ORDER BY id")
        .bind(&selector.order_id).fetch_all(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?.into_iter().filter_map(|(stage_node_id,action,state)| Some(crate::core::production_map::ProductionStageLifecycleEvent { stage_node_id, action: crate::core::production_map::queue_state::ApparatusQueueAction::parse(&action)?, to_state: crate::core::production_map::queue_state::ApparatusQueueOrderState::parse(&state)? })).collect::<Vec<_>>();
    let stage_inputs = stage_execution::inputs_tx(tx, &selector.order_id).await?;
    let stage_work = crate::core::production_map::stage_execution::stage_work_statuses(
        &map,
        &sessions,
        &stage_inputs,
        &states,
        &events,
    );
    if stage_work
        .iter()
        .any(|work| chain::is_final_work_stage_node(&map, &work.stage_node_id) && work.completed)
    {
        deny!("final_stage_closed");
    }
    // Reactivation must anchor an actual Complete, never an issue/forced closure.
    let report_id = session
        .payload_json
        .get("_completion_queue_event_id")
        .and_then(Value::as_str)
        .or_else(|| {
            session
                .payload_json
                .get("stage_work_report")
                .and_then(|p| p.get("report_id"))
                .and_then(Value::as_str)
        })
        .unwrap_or_default();
    // A later astatka report has its own report ID. Prefer a real queue receipt;
    // otherwise require a single completion at the source session boundary.
    let exact_anchor:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_queue_action_events WHERE event_id=$1 AND order_id=$2 AND canonical_apparatus_id=$3 AND stage_node_id=$4 AND action='complete')")
        .bind(report_id).bind(&selector.order_id).bind(&selector.apparatus).bind(&session.stage_node_id).fetch_one(&mut **tx).await.map_err(|_|ProductionMapError::StoreFailed)?;
    let report_id = if exact_anchor { report_id } else { "" };
    let mut closings: Vec<(Value,String)> = sqlx::query_as("SELECT payload_json,policy FROM mini_queue_action_events WHERE order_id=$1 AND canonical_apparatus_id=$2 AND stage_node_id=$3 AND action='complete' AND actor_role=$4 AND actor_ref=$5 AND (($6<>'' AND event_id=$6) OR ($6='' AND extract(epoch FROM created_at)>=$7 AND extract(epoch FROM created_at)<$8)) ORDER BY id DESC LIMIT 2")
        .bind(&selector.order_id).bind(&selector.apparatus).bind(&session.stage_node_id).bind(&session.worker_role).bind(&session.worker_ref).bind(report_id).bind(session.updated_at_unix as f64).bind((session.updated_at_unix+1) as f64).fetch_all(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    if closings.len() != 1 {
        deny!("unsupported_history");
    }
    let Some((closing, policy)) = closings.pop() else {
        deny!("unsupported_history");
    };
    if closing.get("completed_with_issue").and_then(Value::as_bool) == Some(true)
        || closing.get("completion_request").and_then(Value::as_bool) == Some(true)
    {
        deny!("unsupported_history");
    }
    let policy = crate::core::production_map::ApparatusQueuePolicy::parse(&policy)
        .ok_or(ProductionMapError::StoreFailed)?;
    let mut output_ids = if is_print {
        Vec::new()
    } else {
        sqlx::query_scalar::<_,String>("SELECT batch_id FROM mini_progress_batches WHERE session_id=$1 AND action='complete' ORDER BY batch_id")
            .bind(&session.session_id).fetch_all(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?
    };
    if !is_print {
        let reports: Option<Value> = sqlx::query_scalar("SELECT payload_json FROM mini_order_progress_events WHERE session_id=$1 AND action='complete' ORDER BY id DESC LIMIT 1")
            .bind(&session.session_id).fetch_optional(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        if let Some(report) = reports {
            if let Some(ids) = report
                .get("rezka_previously_recorded_batches")
                .and_then(Value::as_array)
            {
                output_ids.extend(ids.iter().filter_map(Value::as_str).map(str::to_string));
            }
        }
    }
    output_ids.sort();
    output_ids.dedup();
    if !is_print && !output_ids.contains(&selector.batch_id) {
        deny!("unsupported_history");
    }
    // Lamination continues its INPUT, while its previous outputs remain owned
    // by downstream consumers. The selected output only identifies the exact
    // completed occurrence; cutting retains its coupled output reservations.
    if is_lamination {
        output_ids.clear();
    }
    // Lock packages before their rolls, matching receipt/movement lock order.
    let paddons = sqlx::query_as::<_,(String,bool)>("SELECT p.id,(p.locked_at IS NOT NULL OR p.receipt_json IS NOT NULL) FROM mini_paddons p WHERE EXISTS(SELECT 1 FROM mini_paddon_items i WHERE i.paddon_id=p.id AND i.progress_batch_id=ANY($1) AND i.removed_at IS NULL) ORDER BY p.id FOR UPDATE")
        .bind(&output_ids).fetch_all(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    if paddons.iter().any(|(_, locked)| *locked) {
        deny!("source_not_free");
    }
    sqlx::query("SELECT batch_id FROM mini_progress_batches WHERE batch_id=ANY($1) ORDER BY batch_id FOR UPDATE")
        .bind(&output_ids).fetch_all(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    let mut outputs = Vec::new();
    for id in &output_ids {
        let Some(batch) = order_query_helpers::load_progress_batch(&mut **tx, id).await? else {
            deny!("source_not_free");
        };
        if batch.session_id != session.session_id
            || batch.wip_status != OrderProgressBatchWipStatus::Waiting
            || !batch.used_by_session_id.is_empty()
            || !batch.processed_by_session_id.is_empty()
            || !batch.used_by_apparatus.is_empty()
            || !batch.processed_by_apparatus.is_empty()
            || batch.is_layer_remainder()
            || !field(&batch.payload_json, "finished_goods_stock_id").is_empty()
            || !field(&batch.payload_json, "received_warehouse").is_empty()
        {
            deny!("source_not_free");
        }
        let external: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_paddon_receipt_lines WHERE progress_batch_id=$1) OR EXISTS(SELECT 1 FROM mini_layer_remainder_operations WHERE source_kind='progress_batch' AND source_id=$1) OR EXISTS(SELECT 1 FROM mini_paddon_items i JOIN mini_paddons p ON p.id=i.paddon_id WHERE i.progress_batch_id=$1 AND i.removed_at IS NULL AND (NOT(p.id=ANY($2)) OR p.locked_at IS NOT NULL OR p.receipt_json IS NOT NULL))")
            .bind(id).bind(paddons.iter().map(|(id,_)| id.clone()).collect::<Vec<_>>()).fetch_one(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        if external {
            deny!("source_not_free");
        }
        let previously_used:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_order_run_sessions WHERE session_id<>$2 AND (payload_json->>'input_progress_batch_id'=$1 OR EXISTS(SELECT 1 FROM jsonb_array_elements(CASE WHEN jsonb_typeof(payload_json->'input_lineage')='array' THEN payload_json->'input_lineage' ELSE '[]'::jsonb END) link WHERE link->>'input_batch_id'=$1)))")
            .bind(id).bind(&session.session_id).fetch_one(&mut **tx).await.map_err(|_|ProductionMapError::StoreFailed)?;
        if previously_used {
            deny!("source_not_free");
        }
        outputs.push(batch);
    }
    let input_id = field(&session.payload_json, "input_progress_batch_id");
    let mut input_released = is_lamination && input_id.is_empty();
    let remaining_rolls = if is_lamination {
        sqlx::query_as::<_, (Option<f64>, Option<f64>)>("SELECT lamination_print_leftover_rolls::float8,lamination_film_leftover_rolls::float8 FROM mini_order_progress_events WHERE session_id=$1 AND action='complete' ORDER BY id DESC LIMIT 1")
            .bind(&session.session_id).fetch_optional(&mut **tx).await
            .map_err(|_| ProductionMapError::StoreFailed)?.unwrap_or_default()
    } else { (None, None) };
    // A processed owner proves provenance, not physical stock. Legacy history
    // without positive leftover evidence must scan a new/remainder input.
    let input_exhausted = is_lamination && remaining_rolls.0.is_none_or(|qty| qty <= 0.0);
    let source_kind = field(&session.payload_json, "input_wip_source_kind");
    let mut opening_input = if source_kind == "opening_wip" {
        let row:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(b) FROM mini_opening_wip_batches b WHERE batch_id=$1 AND order_id=$2 FOR UPDATE")
            .bind(input_id).bind(&selector.order_id).fetch_optional(&mut **tx).await.map_err(|_|ProductionMapError::StoreFailed)?;
        let Some(mut row) = row else {
            deny!("source_not_free");
        };
        if field(&row, "wip_status") != "processed"
            || field(&row, "processed_by_session_id") != session.session_id
            || field(&row, "processed_by_apparatus") != selector.apparatus
        {
            deny!("source_not_free");
        }
        let intake: Option<Value> = sqlx::query_scalar(
            "SELECT to_jsonb(i) || jsonb_build_object(
                'resume_apparatus', COALESCE(resume_apparatus, ''),
                'actor', jsonb_build_object('role', actor_role, 'ref_', actor_ref, 'display_name', actor_display_name),
                'created_at_unix', extract(epoch FROM created_at)::bigint,
                'updated_at_unix', extract(epoch FROM updated_at)::bigint)
             FROM mini_opening_wip_intakes i WHERE intake_id=$1 FOR SHARE",
        )
        .bind(field(&row, "intake_id"))
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        let Some(intake_json) = intake else {
            deny!("source_not_free");
        };
        let intake: crate::core::production_map::OpeningWipIntake =
            serde_json::from_value(intake_json.clone())
                .map_err(|_| ProductionMapError::StoreFailed)?;
        if intake.status != crate::core::production_map::OpeningWipIntakeStatus::Confirmed
            || intake.order_id != selector.order_id
        {
            deny!("source_not_free");
        }
        if crate::core::production_map::ProductionMapService::opening_wip_target_stage(
            &map,
            &intake,
            &selector.apparatus,
            &session.stage_node_id,
        )
        .is_none_or(|stage| stage.node_id != session.stage_node_id)
        {
            deny!("stale_target");
        }
        row["_reactivation_intake_snapshot"] = intake_json;
        Some(row)
    } else {
        None
    };
    let links = order_run_input_links_from_payload(&session.payload_json)
        .map_err(|_| ProductionMapError::ProgressInputInvalid)?;
    if links.len() > 1
        && session
            .payload_json
            .get("_completion_reactivation_payload")
            .is_none()
    {
        deny!("unsupported_history");
    }
    let mut input = if opening_input.is_some() {
        None
    } else if input_id.is_empty() {
        if !is_print && !is_lamination {
            deny!("unsupported_history");
        }
        None
    } else {
        sqlx::query("SELECT batch_id FROM mini_progress_batches WHERE batch_id=$1 FOR UPDATE")
            .bind(input_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let Some(batch) = order_query_helpers::load_progress_batch(&mut **tx, input_id).await?
        else {
            deny!("source_not_free");
        };
        if batch.order_id != selector.order_id
            || batch.wip_status != OrderProgressBatchWipStatus::Processed
            || batch.processed_by_session_id != session.session_id
            || batch.processed_by_apparatus != selector.apparatus
        {
            deny!("source_not_free");
        }
        let returned: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_layer_remainder_operations WHERE source_kind='progress_batch' AND source_id=$1)").bind(input_id).fetch_one(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        if returned && !is_lamination {
            deny!("source_not_free");
        }
        if crate::core::production_map::wip_route::resolve_wip_input_route(&map, &batch).is_ok_and(
            |route| {
                route.stage_node_id == session.stage_node_id
                    && route.consumer_apparatus_ids.contains(&selector.apparatus)
            },
        ) == false
        {
            deny!("stale_target");
        }
        if is_lamination && (returned || input_exhausted) {
            input_released = true;
            None
        } else {
            Some(batch)
        }
    };
    if input_exhausted {
        input_released = true;
        input = None;
        opening_input = None;
    }
    let (raw, raw_released) = match reactivation_raw_materials(
        tx, selector, &session, &closing, is_lamination, remaining_rolls.1,
    ).await? {
        Ok(materials) => materials,
        Err(reason) => return Ok(Err(reason)),
    };
    if is_print {
        sqlx::query(
            "SELECT pg_advisory_xact_lock_shared(hashtextextended('qolip:product-composition',0))",
        )
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        let mut active = session.clone();
        active.status = OrderRunStatus::Active;
        for outcome in [
            qolip_session_helpers::reject_qolip_in_use_tx(tx, &active).await,
            qolip_session_helpers::validate_qolip_set_tx(tx, &active).await,
        ] {
            if let Err(error) = outcome {
                if error == ProductionMapError::StoreFailed {
                    return Err(error);
                }
                deny!("resource_unavailable");
            }
        }
        let codes = session
            .payload_json
            .get("qolip_codes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str);
        for code in codes {
            let held_elsewhere:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_qolip_checkouts WHERE lower(qolip_code)=lower($1) AND status='open' AND lower(issued_to_ref)<>lower($2))")
                .bind(code).bind(&actor.ref_).fetch_one(&mut **tx).await.map_err(|_|ProductionMapError::StoreFailed)?;
            if held_elsewhere {
                deny!("resource_unavailable");
            }
        }
    }
    let queue_state = states
        .get(&selector.apparatus)
        .and_then(|s| s.get(&selector.order_id))
        .cloned()
        .unwrap_or("pending".into());
    if !matches!(queue_state.as_str(), "completed" | "pending") {
        deny!("apparatus_busy");
    }
    let continuations = sessions.iter().filter(|s| field(&s.payload_json, "_reactivation_of_session_id") == session.session_id).map(|s| &s.session_id).collect::<BTreeSet<_>>();
    let token=format!("{:x}",Sha256::digest(serde_json::to_vec(&json!({"apparatus_config":apparatus_config,"map":map_json,"session":session,"selected_output":selected_output,"outputs":outputs,"input":input,"opening_input":opening_input,"raw":raw,"input_released":input_released,"raw_released":raw_released,"actor_role":actor.role,"actor_ref":actor.ref_,"queue":queue_state,"continuations":continuations})).map_err(|_| ProductionMapError::StoreFailed)?));
    let target = ReactivationTarget {
        apparatus: selector.apparatus.clone(),
        order_id: selector.order_id.clone(),
        batch_id: selector.batch_id.clone(),
        session_id: session.session_id.clone(),
        stage_node_id: session.stage_node_id.clone(),
        token,
    };
    Ok(Ok(Prepared {
        target,
        session,
        outputs,
        input,
        opening_input,
        policy,
        raw,
        queue_state,
        latest_started: sessions
            .iter()
            .filter(|s| s.apparatus == selector.apparatus)
            .map(|s| s.started_at_unix)
            .max()
            .unwrap_or(0),
        latest_session_id: sessions
            .iter()
            .filter(|s| s.apparatus == selector.apparatus)
            .max_by_key(|s| (s.started_at_unix, &s.session_id))
            .map(|s| s.session_id.clone())
            .unwrap_or_default(),
        input_released,
        raw_released,
    }))
}
