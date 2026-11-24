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
        || !valid_list_projection(&query.view, &query.projection)
        || (!query.compression.is_empty()
            && (query.compression != "identity"
                || !matches!(query.view.as_str(), "sequence" | "orders")))
        || (query.view == "move"
            && (!valid_move_apparatus(&query.apparatus)
                || !valid_move_apparatus(&query.target)
                || query.apparatus == query.target))
    {
        return Err(bad_request("invalid_order_page_scope"));
    }
    // No account may reuse another account's conditional response/cursor.
    let mut scope_value = serde_json::to_value(&(
        &principal.role,
        &principal.ref_,
        &query.view,
        &query.apparatus,
        &query.target,
        &query.search,
        limit,
    ))
    .map_err(|_| bad_request("invalid_order_page_scope"))?;
    // Preserve legacy scopes byte-for-byte; a new projection cannot reuse a
    // full-page cursor, conditional response, live baseline or disk cache.
    if !query.projection.is_empty() {
        scope_value
            .as_array_mut()
            .unwrap()
            .push(query.projection.clone().into());
    }
    let scope = scope_value.to_string();

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
    let (ids, mut customers, snapshot, revision, existing_ids) = if query.view == "orders"
        || query.apparatus == "unassigned"
    {
        let (catalog, existing_ids) = state
            .production_maps
            .admin_order_catalog((query.view == "move").then_some(apparatus.as_str()))
            .await
            .map_err(production_map_error)?;
        let customers = if query.projection == ORDERS_LIST_PROJECTION && query.search.is_empty() {
            BTreeMap::new()
        } else {
            production_map_definition_customers(&state, &catalog.iter().collect::<Vec<_>>()).await
        };
        let configurations = if query.view == "move" {
            state
                .production_maps
                .admin_move_page_configurations()
                .await
                .map_err(production_map_error)?
        } else {
            Vec::new()
        };
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
        // An unfiltered lightweight page needs customer fallbacks only for its
        // selected rows. One old order outside the page must not force an
        // archive lookup before the first twenty current rows are selected.
        let customers = if query.projection == SEQUENCE_LIST_PROJECTION && query.search.is_empty() {
            BTreeMap::new()
        } else {
            production_map_order_customers(&state, &snapshot.maps).await
        };
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
    let mut payload = if query.projection == SEQUENCE_LIST_PROJECTION {
        if !snapshot.sequence_versions.contains_key(apparatus)
            || !snapshot
                .queue_policies
                .iter()
                .any(|policy| policy.apparatus_id.as_str() == apparatus)
        {
            return Err(bad_request("invalid_order_page_scope"));
        }
        if query.search.is_empty() {
            customers = production_map_definition_customers(
                state,
                &snapshot
                    .maps
                    .iter()
                    .filter(|saved| selected.contains(&saved.map.id))
                    .map(|saved| &saved.map)
                    .collect::<Vec<_>>(),
            )
            .await;
        }
        list::project(&snapshot, &selected, apparatus, &query.search, &customers)
    } else if query.projection == ORDERS_LIST_PROJECTION {
        if query.search.is_empty() {
            customers = production_map_order_customers(state, &snapshot.maps).await;
        }
        list::project_orders(&snapshot, &selected, &selected_pending, &customers)
    } else {
        let projected = project(&snapshot, &selected);
        let calculation_templates = page_calculation_templates(&state, &projected.maps).await;
        let mut payload = serde_json::to_value(projected).unwrap();
        payload["pending_orders"] = serde_json::json!(selected_pending);
        payload["calculation_templates"] = serde_json::json!(calculation_templates);
        payload["order_customers"] = serde_json::json!(
            customers
                .iter()
                .filter(|(id, _)| selected.contains(id))
                .collect::<BTreeMap<_, _>>()
        );
        payload
    };
    if state.production_maps.snapshot_revision() != revision {
        return Err(conflict("order_page_changed"));
    }
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
    payload["next_cursor"] = serde_json::json!(next_cursor(&page_version, end, total));
    Ok(Some(payload))
}

pub(crate) async fn page_calculation_templates(
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
