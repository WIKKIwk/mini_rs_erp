//! Admin read windows. A cursor belongs to one query and one canonical revision.
//! These projections must never be used as the authority for queue mutations.
use super::*;
use crate::core::production_map::{
    ApparatusQueuePolicy, ProductionMapLiveSnapshot, ProductionMapSaved,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, Deserialize)]
pub struct OrderPageQuery {
    #[serde(default)]
    pub(super) view: String,
    #[serde(default)]
    pub(super) apparatus: String,
    #[serde(default)]
    pub(super) target: String,
    #[serde(default)]
    pub(super) search: String,
    #[serde(default)]
    pub(super) cursor: String,
    #[serde(default)]
    pub(super) if_version: String,
    pub(super) limit: Option<usize>,
}

#[derive(serde::Serialize, Deserialize)]
struct Cursor {
    version: String,
    offset: usize,
}

fn version(scope: &str, epoch: &str, revision: u64, pending: &str) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!("{scope}\0{epoch}\0{revision}\0{pending}"))
    )
}

fn offset(cursor: &str, version: &str) -> Result<usize, AdminError> {
    if cursor.is_empty() {
        return Ok(0);
    }
    if cursor.len() > 512 {
        return Err(bad_request("invalid_order_page_cursor"));
    }
    let decoded = URL_SAFE_NO_PAD
        .decode(cursor)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Cursor>(&bytes).ok())
        .ok_or_else(|| bad_request("invalid_order_page_cursor"))?;
    if decoded.version != version {
        return Err(conflict("order_page_changed"));
    }
    Ok(decoded.offset)
}

fn next_cursor(version: &str, end: usize, total: usize) -> Option<String> {
    (end < total).then(|| {
        URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&Cursor {
                version: version.into(),
                offset: end,
            })
            .unwrap(),
        )
    })
}

fn not_modified() -> Response {
    (
        StatusCode::NOT_MODIFIED,
        [(header::CACHE_CONTROL, "no-store")],
    )
        .into_response()
}

pub async fn production_map_order_page(
    State(state): State<AppState>,
    Query(query): Query<OrderPageQuery>,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    Ok(match read_page(&state, &principal, query).await? {
        Some(payload) => json_response(payload),
        None => not_modified(),
    })
}

/// The HTTP bootstrap and live windows use the same domain projection and
/// validation. A live window is never an authority for a queue mutation.
pub(super) async fn read_page(
    state: &AppState,
    principal: &Principal,
    mut query: OrderPageQuery,
) -> Result<Option<serde_json::Value>, AdminError> {
    query.search = query.search.trim().to_lowercase();
    query.apparatus = query.apparatus.trim().to_string();
    query.target = query.target.trim().to_string();
    let limit = query.limit.unwrap_or(20);
    if !(1..=50).contains(&limit)
        || query.search.chars().count() > 120
        || !matches!(
            query.view.as_str(),
            "sequence" | "orders" | "move" | "closed" | "audit"
        )
        || (query.view == "sequence" && !queue_state::is_canonical_apparatus_id(&query.apparatus))
        || (query.view == "move"
            && (!valid_move_apparatus(&query.apparatus)
                || !valid_move_apparatus(&query.target)
                || query.apparatus == query.target))
    {
        return Err(bad_request("invalid_order_page_scope"));
    }
    // No account may reuse another account's conditional response/cursor.
    let scope = serde_json::to_string(&(
        &principal.role,
        &principal.ref_,
        &query.view,
        &query.apparatus,
        &query.target,
        &query.search,
        limit,
    ))
    .map_err(|_| bad_request("invalid_order_page_scope"))?;

    // Pending intake has its own store, so include its content in the version
    // instead of assuming every intake write increments the queue revision.
    let epoch = state.production_maps.snapshot_epoch();
    let revision = state.production_maps.snapshot_revision();
    let pending = if query.view == "orders" {
        match state.pending_orders.as_deref() {
            Some(store) => store
                .list()
                .await
                .map_err(super::super::pending_orders::pending_error)?,
            None => Vec::new(),
        }
    } else {
        Vec::new()
    };
    let pending_version = if pending.is_empty() {
        String::new()
    } else {
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&pending).unwrap())
        )
    };
    // An intake insert can notify while list() is awaiting. Never stamp the
    // older list (or a conditional response) with the newer live revision.
    if state.production_maps.snapshot_revision() != revision {
        return Err(conflict("order_page_changed"));
    }
    let initial_version = version(&scope, &epoch, revision, &pending_version);
    if query.cursor.is_empty() && query.if_version == initial_version {
        return Ok(None);
    }
    offset(&query.cursor, &initial_version)?;

    if query.view == "audit" {
        let mut report = state
            .production_maps
            .audit_production_workflow()
            .await
            .map_err(production_map_error)?;
        if state.production_maps.snapshot_revision() != revision {
            return Err(conflict("order_page_changed"));
        }
        let total_violations = report.violations.len();
        report.violations.sort_by(|a, b| {
            (&a.order_id, &a.code, &a.subject, &a.detail).cmp(&(
                &b.order_id,
                &b.code,
                &b.subject,
                &b.detail,
            ))
        });
        report.violations.retain(|item| {
            query.search.is_empty()
                || [&item.order_id, &item.code, &item.subject, &item.detail]
                    .iter()
                    .any(|text| text.to_lowercase().contains(&query.search))
        });
        let total = report.violations.len();
        let start = offset(&query.cursor, &initial_version)?;
        if start > total {
            return Err(conflict("order_page_changed"));
        }
        let end = start.saturating_add(limit).min(total);
        report.violations = report.violations[start..end].to_vec();
        let mut report = serde_json::to_value(report).unwrap();
        report["total_violations"] = total_violations.into();
        return Ok(Some(serde_json::json!({
            "ok": true, "version": initial_version, "epoch": epoch, "rev": revision,
            "total": total, "offset": start,
            "next_cursor": next_cursor(&initial_version, end, total),
            "workflow_audit": report,
        })));
    }

    if query.view == "closed" {
        let orders = state
            .production_maps
            .closed_order_summaries()
            .await
            .map_err(production_map_error)?;
        if state.production_maps.snapshot_revision() != revision {
            return Err(conflict("order_page_changed"));
        }
        let orders = orders
            .into_iter()
            .filter(|order| {
                query.search.is_empty() || closed_search_text(order).contains(&query.search)
            })
            .collect::<Vec<_>>();
        let start = offset(&query.cursor, &initial_version)?;
        if start > orders.len() {
            return Err(conflict("order_page_changed"));
        }
        let end = start.saturating_add(limit).min(orders.len());
        let mut items = serde_json::to_value(&orders[start..end]).unwrap();
        for item in items.as_array_mut().unwrap() {
            // Log details are fetched only when the history row is expanded.
            item["history_version"] = format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&item["logs"]).unwrap())
            )
            .into();
            item["logs"] = serde_json::json!([]);
            item["progress_batches"] = serde_json::json!([]);
        }
        return Ok(Some(serde_json::json!({
            "ok": true, "version": initial_version, "epoch": epoch, "rev": revision,
            "total": orders.len(), "offset": start,
            "next_cursor": next_cursor(&initial_version, end, orders.len()),
            "closed_orders": items,
        })));
    }

    let apparatus = if query.apparatus == "unassigned" {
        &query.target
    } else {
        &query.apparatus
    };
    let mut sequence_positions = BTreeMap::new();
    let (ids, customers, snapshot, revision, existing_ids) = if query.view == "orders"
        || query.apparatus == "unassigned"
    {
        let (catalog, existing_ids) = state
            .production_maps
            .admin_order_catalog((query.view == "move").then_some(apparatus.as_str()))
            .await
            .map_err(production_map_error)?;
        let customers =
            production_map_definition_customers(&state, &catalog.iter().collect::<Vec<_>>()).await;
        let configurations = state
            .production_maps
            .admin_move_page_configurations()
            .await
            .map_err(production_map_error)?;
        let ids = catalog
            .iter()
            .filter(|map| {
                matches_search(map, &query.search, &customers)
                    && (query.view != "move"
                        || state.production_maps.admin_move_page_matches(
                            map,
                            &query.apparatus,
                            &query.target,
                            &configurations,
                        ))
            })
            .map(|map| map.id.clone())
            .collect::<Vec<_>>();
        (ids, customers, None, revision, existing_ids)
    } else {
        let (snapshot, revision) = state
            .production_maps
            .worker_snapshot_shared_with_revision(&[apparatus.clone()], &[])
            .await
            .map_err(production_map_error)?;
        let snapshot = super::super::training::merge_worker_training_snapshot_shared(
            &state, &principal, snapshot,
        )
        .await
        .map_err(super::super::training::training_workspace_error)?;
        let customers = production_map_order_customers(&state, &snapshot.maps).await;
        let by_id = snapshot
            .maps
            .iter()
            .map(|saved| (saved.map.id.as_str(), saved))
            .collect::<BTreeMap<_, _>>();
        let mut ids = if query.apparatus == "unassigned" {
            snapshot
                .maps
                .iter()
                .filter(|saved| is_order(&saved.map.id))
                .map(|saved| saved.map.id.clone())
                .collect::<Vec<_>>()
        } else {
            ordered_ids(&snapshot, apparatus)
        };
        let configurations = if query.view == "move" {
            Some(
                state
                    .production_maps
                    .admin_move_page_configurations()
                    .await
                    .map_err(production_map_error)?,
            )
        } else {
            None
        };
        // Count eligible queue rows before search or pagination. Only matching
        // IDs are retained here, and only selected page IDs go on the wire.
        let mut position = 0usize;
        ids.retain(|id| {
            by_id.get(id.as_str()).is_some_and(|saved| {
                if !is_order(id) {
                    return false;
                }
                position += 1;
                let matches = matches_search(&saved.map, &query.search, &customers)
                    && (query.view != "move"
                        || state.production_maps.admin_move_page_matches(
                            &saved.map,
                            &query.apparatus,
                            &query.target,
                            configurations.as_ref().unwrap(),
                        ));
                if matches && query.view == "sequence" {
                    sequence_positions.insert(id.clone(), position);
                }
                matches
            })
        });
        let existing_ids = by_id.keys().map(|id| id.to_string()).collect();
        (ids, customers, Some(snapshot), revision, existing_ids)
    };
    let page_version = version(&scope, &epoch, revision, &pending_version);
    let start = offset(&query.cursor, &page_version)?;
    let pending = pending
        .into_iter()
        .filter(|order| {
            !existing_ids.contains(&order.id)
                && (query.search.is_empty()
                    || [
                        &order.template.order_number,
                        &order.template.customer,
                        &order.template.product,
                        &order.manager_name,
                    ]
                    .iter()
                    .any(|value| value.to_lowercase().contains(&query.search)))
        })
        .collect::<Vec<_>>();
    let total = ids.len() + pending.len();
    if start > total {
        return Err(conflict("order_page_changed"));
    }
    let end = start.saturating_add(limit).min(total);
    let selected_pending = pending
        .iter()
        .skip(start)
        .take(end - start)
        .collect::<Vec<_>>();
    let selected = ids
        .iter()
        .skip(start.saturating_sub(pending.len()))
        .take(
            end.saturating_sub(pending.len())
                .saturating_sub(start.saturating_sub(pending.len())),
        )
        .cloned()
        .collect::<Vec<_>>();
    let snapshot = if let Some(snapshot) = snapshot {
        snapshot
    } else {
        let (snapshot, detail_revision) = state
            .production_maps
            .order_snapshot_shared_with_revision(&selected)
            .await
            .map_err(production_map_error)?;
        if detail_revision != revision {
            return Err(conflict("order_page_changed"));
        }
        snapshot
    };
    if state.production_maps.snapshot_revision() != revision {
        return Err(conflict("order_page_changed"));
    }
    let projected = project(&snapshot, &selected);
    let calculation_templates = page_calculation_templates(&state, &projected.maps).await;
    if state.production_maps.snapshot_revision() != revision {
        return Err(conflict("order_page_changed"));
    }
    let mut payload = serde_json::to_value(projected).unwrap();
    payload["ok"] = true.into();
    payload["epoch"] = epoch.into();
    payload["rev"] = revision.into();
    payload["scope"] = scope.into();
    payload["version"] = page_version.clone().into();
    payload["offset"] = start.into();
    payload["total"] = total.into();
    payload["order_ids"] = serde_json::json!(selected);
    payload["sequence_positions"] = serde_json::json!(
        sequence_positions
            .into_iter()
            .filter(|(id, _)| selected.contains(id))
            .collect::<BTreeMap<_, _>>()
    );
    payload["pending_orders"] = serde_json::json!(selected_pending);
    payload["calculation_templates"] = serde_json::json!(calculation_templates);
    payload["next_cursor"] = serde_json::json!(next_cursor(&page_version, end, total));
    payload["order_customers"] = serde_json::json!(
        customers
            .into_iter()
            .filter(|(id, _)| selected.contains(id))
            .collect::<BTreeMap<_, _>>()
    );
    Ok(Some(payload))
}

async fn page_calculation_templates(
    state: &AppState,
    maps: &[ProductionMapSaved],
) -> BTreeMap<String, CalculateOrderTemplate> {
    let legacy = maps
        .iter()
        .filter(|saved| {
            saved.map.base_length.is_none_or(|v| v <= 0.0)
                || saved.map.order_kg.is_none_or(|v| v <= 0.0)
        })
        .collect::<Vec<_>>();
    if legacy.is_empty() {
        return BTreeMap::new();
    }
    let Ok(templates) = state.calculate_orders.list_all().await else {
        return BTreeMap::new();
    };
    let mut selected = BTreeMap::new();
    for saved in legacy {
        let map = &saved.map;
        let keys = [
            &map.order_number,
            &map.code,
            map.id.strip_prefix("zakaz-").unwrap_or(""),
        ];
        let exact = templates
            .iter()
            .find(|t| t.source_map_id.trim() == map.id.trim())
            .or_else(|| {
                templates.iter().find(|t| {
                    keys.iter()
                        .filter(|k| !k.trim().is_empty())
                        .any(|k| k.trim() == t.order_number.trim() || k.trim() == t.code.trim())
                })
            });
        let fallback = || {
            let products = [map.product_code.to_lowercase(), map.title.to_lowercase()];
            let candidates = templates
                .iter()
                .filter(|t| {
                    (!t.product.is_empty() && products.contains(&t.product.to_lowercase())
                        || !t.item_code.is_empty()
                            && products.contains(&t.item_code.to_lowercase()))
                        && !map.width_mm.is_some_and(|width| {
                            width > 0.0 && t.width_mm > 0.0 && (width - t.width_mm).abs() > 0.5
                        })
                })
                .collect::<Vec<_>>();
            candidates
                .iter()
                .copied()
                .find(|t| !t.source_map_id.trim().is_empty())
                .or_else(|| candidates.last().copied())
        };
        if let Some(template) = exact.or_else(fallback) {
            selected.insert(map.id.clone(), template.clone());
        }
    }
    selected
}

pub async fn production_map_closed_order_detail(
    State(state): State<AppState>,
    Query(query): Query<super::super::pending_orders::PendingOrderQuery>,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
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

fn ordered_ids(snapshot: &ProductionMapLiveSnapshot, apparatus: &str) -> Vec<String> {
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

fn project(snapshot: &ProductionMapLiveSnapshot, ids: &[String]) -> ProductionMapLiveSnapshot {
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
