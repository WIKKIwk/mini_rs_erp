/// Demand-loaded canonical detail for exactly one admin row. Cross-machine
/// controls remain complete for that order; unrelated orders never go on wire.
pub(super) async fn read_detail(
    state: &AppState,
    principal: &Principal,
    apparatus: &str,
    order_id: &str,
) -> Result<Response, AdminError> {
    for _ in 0..3 {
        let (snapshot, revision) = state
            .production_maps
            .order_snapshot_shared_with_revision(&[order_id.to_string()])
            .await
            .map_err(production_map_error)?;
        let snapshot = super::super::training::merge_worker_training_snapshot_shared(
            state, principal, snapshot,
        )
        .await
        .map_err(super::super::training::training_workspace_error)?;
        if state.production_maps.snapshot_revision() != revision {
            continue;
        }
        let projected = project(&snapshot, &[order_id.to_string()]);
        if projected.maps.len() != 1 {
            return Err(not_found("order_not_available"));
        }
        let customers = production_map_order_customers(state, &projected.maps).await;
        let templates = page_calculation_templates(state, &projected.maps).await;
        let creation = state
            .production_orders
            .order_creation_metadata(order_id)
            .await
            .map_err(|_| server_error("Buyurtma yaratilishi haqidagi ma’lumot yuklanmadi"))?;
        let map = &projected.maps[0].map;
        let qolip_in_rework = match state
            .qolip
            .require_qolips_available_for_production_map(&map)
            .await
        {
            Ok(()) => Some(false),
            Err(crate::core::qolip::QolipError::QolipInRework) => Some(true),
            Err(error) => {
                tracing::warn!(order_id, ?error, "failed to read order mold rework status");
                None
            }
        };
        if state.production_maps.snapshot_revision() != revision {
            continue;
        }
        let mut payload = serde_json::to_value(projected).unwrap();
        payload["maps"][0]["created_at_unix"] = serde_json::json!(creation.created_at_unix);
        payload["maps"][0]["created_by"] = serde_json::json!(creation.created_by.map(|actor| {
            serde_json::json!({"role": actor.role, "ref": actor.ref_, "display_name": actor.display_name})
        }));
        payload["maps"][0]["qolip_in_rework"] = serde_json::json!(qolip_in_rework);
        payload["ok"] = true.into();
        payload["projection"] = "order_detail_v1".into();
        payload["order_id"] = order_id.into();
        payload["apparatus"] = apparatus.into();
        payload["epoch"] = state.production_maps.snapshot_epoch().into();
        payload["rev"] = revision.into();
        payload["scope"] = serde_json::json!([
            principal.role,
            principal.ref_,
            "order_detail_v1",
            apparatus,
            order_id,
        ])
        .to_string()
        .into();
        payload["order_customers"] = serde_json::json!(customers);
        payload["calculation_templates"] = serde_json::json!(templates);
        return Ok(json_response(payload));
    }
    Err(conflict("order_page_changed"))
}

pub async fn production_map_closed_order_detail(
    State(state): State<AppState>,
    Query(query): Query<super::super::pending_orders::PendingOrderQuery>,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::ProductionMapRead,
        ],
    )
    .await?;
    let order = state
        .production_maps
        .closed_order_detail(&query.id)
        .await
        .map_err(production_map_error)?
        .ok_or_else(|| not_found("closed_order_not_found"))?;
    Ok(json_response(serde_json::json!(order)))
}

fn valid_move_apparatus(id: &str) -> bool {
    id == "unassigned" || queue_state::is_canonical_apparatus_id(id)
}

fn is_order(id: &str) -> bool {
    id.starts_with("zakaz-") || id.starts_with("training-zakaz-")
}

fn matches_search(
    map: &ProductionMapDefinition,
    query: &str,
    customers: &BTreeMap<String, String>,
) -> bool {
    query.is_empty()
        || [
            map.id.as_str(),
            &map.code,
            &map.order_number,
            &map.title,
            &map.product_code,
            &map.customer_name,
            customers.get(&map.id).map(String::as_str).unwrap_or(""),
        ]
        .into_iter()
        .chain(map.nodes.iter().map(|node| node.title.as_str()))
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
        .contains(query)
}

fn closed_search_text(
    order: &crate::core::production_map::FullyCompletedProductionOrder,
) -> String {
    let mut values = vec![
        order.order_id.as_str(),
        &order.order_number,
        &order.title,
        &order.product_code,
        &order.closed_by_role,
        &order.closed_by_ref,
        &order.closed_by_display_name,
    ];
    for log in &order.logs {
        values.extend([
            log.apparatus.as_str(),
            log.action.as_str(),
            log.from_state.as_str(),
            log.to_state.as_str(),
            &log.actor_role,
            &log.actor_ref,
            &log.actor_display_name,
        ]);
    }
    if order.early_close.is_some() {
        values.extend(["close_early", "cancelled"]);
    }
    values.join(" ").to_lowercase()
}

pub(crate) fn ordered_ids(snapshot: &ProductionMapLiveSnapshot, apparatus: &str) -> Vec<String> {
    let visible = snapshot
        .visible_order_ids
        .get(apparatus)
        .into_iter()
        .flatten()
        .filter(|id| {
            if snapshot
                .queue_states
                .get(apparatus)
                .and_then(|states| states.get(*id))
                .is_none_or(|state| state != "completed")
            {
                return true;
            }
            // Match the admin's occurrence-aware filter: an old completion
            // cannot hide a later unfinished use of the same physical machine.
            snapshot
                .maps
                .iter()
                .find(|saved| &saved.map.id == *id)
                .is_some_and(|saved| {
                    crate::core::production_map::chain::linear_work_stages(&saved.map)
                        .iter()
                        .filter(|stage| stage.apparatus_id.as_deref() == Some(apparatus))
                        .any(|stage| {
                            snapshot
                                .stage_states
                                .get(*id)
                                .and_then(|states| states.get(&stage.node_id))
                                .is_some_and(|state| state != "completed")
                        })
                })
        })
        .cloned()
        .collect::<Vec<_>>();
    if snapshot.queue_policies.iter().any(|policy| {
        policy.apparatus_id.as_str() == apparatus && policy.policy == ApparatusQueuePolicy::FreePick
    }) {
        visible
    } else {
        queue_state::effective_apparatus_sequence(
            snapshot
                .sequences
                .get(apparatus)
                .map(Vec::as_slice)
                .unwrap_or(&[]),
            &visible,
        )
    }
}

pub(super) fn project(
    snapshot: &ProductionMapLiveSnapshot,
    ids: &[String],
) -> ProductionMapLiveSnapshot {
    let selected = ids.iter().cloned().collect::<BTreeSet<_>>();
    fn keep<T: Clone>(
        values: &BTreeMap<String, T>,
        selected: &BTreeSet<String>,
    ) -> BTreeMap<String, T> {
        values
            .iter()
            .filter(|(id, _)| selected.contains(*id))
            .map(|(id, v)| (id.clone(), v.clone()))
            .collect()
    }
    ProductionMapLiveSnapshot {
        maps: ids
            .iter()
            .filter_map(|id| snapshot.maps.iter().find(|m| &m.map.id == id).cloned())
            .collect(),
        sequences: snapshot
            .sequences
            .iter()
            .map(|(id, values)| {
                (
                    id.clone(),
                    values
                        .iter()
                        .filter(|v| selected.contains(*v))
                        .cloned()
                        .collect(),
                )
            })
            .collect(),
        visible_order_ids: snapshot
            .visible_order_ids
            .iter()
            .map(|(id, values)| {
                (
                    id.clone(),
                    values
                        .iter()
                        .filter(|v| selected.contains(*v))
                        .cloned()
                        .collect(),
                )
            })
            .collect(),
        sequence_versions: snapshot.sequence_versions.clone(),
        sequence_revisions: snapshot.sequence_revisions.clone(),
        sequence_positions_by_apparatus: BTreeMap::new(),
        queue_states: snapshot
            .queue_states
            .iter()
            .map(|(id, v)| (id.clone(), keep(v, &selected)))
            .collect(),
        queue_action_controls: snapshot
            .queue_action_controls
            .iter()
            .map(|(id, v)| (id.clone(), keep(v, &selected)))
            .collect(),
        stage_states: keep(&snapshot.stage_states, &selected),
        order_statuses: keep(&snapshot.order_statuses, &selected),
        order_controls: keep(&snapshot.order_controls, &selected),
        stage_order_controls: snapshot
            .stage_order_controls
            .iter()
            .filter(|c| selected.contains(&c.order_id))
            .cloned()
            .collect(),
        queue_policies: snapshot.queue_policies.clone(),
        frozen_orders_by_apparatus: snapshot
            .frozen_orders_by_apparatus
            .iter()
            .map(|(id, values)| {
                (
                    id.clone(),
                    values
                        .iter()
                        .filter(|v| selected.contains(&v.order_id))
                        .cloned()
                        .collect(),
                )
            })
            .collect(),
    }
}
