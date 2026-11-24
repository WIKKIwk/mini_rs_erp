async fn read_list(
    state: &AppState,
    principal: &Principal,
    query: &OrderListQuery,
) -> Result<Value, ApiError> {
    let requested_at = Instant::now();
    for _ in 0..3 {
        let revision = state.production_maps.snapshot_revision();
        let scope = read_scope(state, principal, query).await?;
        let epoch = state.production_maps.snapshot_epoch();
        let entry = row_cache_entry(digest((&scope.key, epoch, revision))).await;
        // One build per exact actor/query/revision. Only compact rows survive
        // the build; topology and fallback canonical snapshots are not cached here.
        let mut cached = entry.lock().await;
        // First/prefix reads validate cheap tuple metadata before reusing the
        // exact revision. Cursor pages retain the same coherent baseline and
        // do not reopen map tables just to advance their display window.
        let metadata_stamp = if cached.is_none() || query.cursor.is_empty() {
            Some(read_metadata_stamp(state, &scope).await?)
        } else {
            None
        };
        if let (Some(value), Some(stamp)) = (cached.as_ref(), metadata_stamp.as_deref()) {
            if !value.reusable(stamp, query.refresh, requested_at) {
                *cached = None;
            }
        }
        if cached.is_none() {
            let catalog = store(state)?
                .order_list_catalog(&principal.ref_, &scope.apparatus_id, scope.all_apparatus)
                .await
                .map_err(error)?;
            if responsibilities(&catalog.responsibilities) != scope.responsibilities {
                continue;
            }
            let maps = catalog
                .candidates
                .iter()
                .map(|c| c.map.clone())
                .collect::<Vec<_>>();
            let mut ids = if scope.all_apparatus {
                catalog
                    .candidates
                    .iter()
                    .filter(|candidate| candidate.allowed)
                    .map(|candidate| candidate.map.id.clone())
                    .collect::<Vec<_>>()
            } else {
                scope
                    .selected
                    .as_ref()
                    .map(|selected| {
                        ProductionMapService::preparation_catalog_sequence(
                            &maps,
                            &catalog.sequence,
                            selected,
                            &BTreeSet::new(),
                        )
                    })
                    .unwrap_or_default()
            };
            let affected = catalog
                .candidates
                .iter()
                .filter(|c| !scope.all_apparatus && c.needs_authority && ids.contains(&c.map.id))
                .map(|c| c.map.id.clone())
                .collect::<Vec<_>>();
            let mut priority = BTreeMap::<String, (u8, i64)>::new();
            let mut hidden = BTreeSet::new();
            let mut frozen_sequence_ids = BTreeSet::new();
            // Only executed/cooperative orders expand into the existing authority.
            // Untouched rows never build controls, programs, histories or WIP.
            // The expansion is bounded to order IDs, never a global snapshot build.
            let mut changed = false;
            for chunk in affected.chunks(50) {
                let (snapshot, read_revision) = state
                    .production_maps
                    .order_snapshot_shared_with_revision(chunk)
                    .await
                    .map_err(|_| {
                        api_error(StatusCode::INTERNAL_SERVER_ERROR, "preparation_orders_read")
                    })?;
                if read_revision != revision {
                    changed = true;
                    break;
                }
                let visible = order_page::ordered_ids(&snapshot, &scope.apparatus_id)
                    .into_iter()
                    .collect::<BTreeSet<_>>();
                for id in chunk {
                    let control = snapshot
                        .queue_action_controls
                        .get(&scope.apparatus_id)
                        .and_then(|v| v.get(id));
                    let raw = snapshot
                        .queue_states
                        .get(&scope.apparatus_id)
                        .and_then(|v| v.get(id))
                        .map(String::as_str)
                        .or_else(|| control.map(|c| c.state.as_str()))
                        .unwrap_or("pending");
                    let global = snapshot.order_controls.get(id);
                    let status = snapshot.order_statuses.get(id);
                    if !visible.contains(id) || !snapshot.maps.iter().any(|m| &m.map.id == id) {
                        hidden.insert(id.clone());
                    }
                    if raw == "frozen"
                        || global.is_some_and(|c| c.state == OrderControlState::Frozen)
                    {
                        frozen_sequence_ids.insert(id.clone());
                    }
                    let red = global.is_some_and(|c| c.state != OrderControlState::Active)
                        || status.is_some_and(|s| {
                            matches!(s.order_status.as_str(), "frozen" | "completed_with_issue")
                                || s.completed_with_issue_count > 0
                        })
                        || raw == "frozen";
                    let rank = if red {
                        3
                    } else if matches!(raw, "in_progress" | "print_preflight") {
                        0
                    } else if raw == "paused" {
                        1
                    } else {
                        2
                    };
                    priority.insert(
                        id.clone(),
                        (rank, control.map(|c| c.last_worked_at_unix).unwrap_or(0)),
                    );
                }
            }
            if changed || state.production_maps.snapshot_revision() != revision {
                continue;
            }
            if !frozen_sequence_ids.is_empty() {
                ids = scope
                    .selected
                    .as_ref()
                    .map(|selected| {
                        ProductionMapService::preparation_catalog_sequence(
                            &maps,
                            &catalog.sequence,
                            selected,
                            &frozen_sequence_ids,
                        )
                    })
                    .unwrap_or_default();
            }
            ids.retain(|id| !hidden.contains(id));
            if scope
                .selected
                .as_ref()
                .is_some_and(|c| c.queue.discipline == QueueDiscipline::FreePick)
            {
                ids.sort_by_key(|id| {
                    let (priority, last) = priority.get(id).copied().unwrap_or((2, 0));
                    (priority, std::cmp::Reverse(last))
                });
            }
            let by_id = catalog
                .candidates
                .iter()
                .map(|c| (c.map.id.as_str(), c))
                .collect::<BTreeMap<_, _>>();
            let rows = ids
                .iter()
                .enumerate()
                .filter_map(|(index, id)| {
                    let candidate = by_id.get(id.as_str())?;
                    if !candidate.allowed {
                        return None;
                    }
                    let row = row(&candidate.map, index + 1);
                    (query.search.is_empty()
                        || [
                            &row.id,
                            &row.code,
                            &row.order_number,
                            &row.title,
                            &row.customer_name,
                            &candidate.map.product_code,
                        ]
                        .into_iter()
                        .map(|s| s.to_lowercase())
                        .collect::<Vec<_>>()
                        .join(" ")
                        .contains(&query.search))
                    .then_some(row)
                })
                .collect::<Vec<_>>();
            // A display list only changes when its actual content changes.
            // Unrelated global writes advance rev but can keep a checkpoint.
            let version = list_version(&scope.key, epoch, &rows);
            *cached = Some(CompactRows {
                rows,
                version,
                metadata_stamp: metadata_stamp.clone().unwrap_or_default(),
                built_at: Instant::now(),
            });
        }
        let value = cached.as_ref().expect("compact rows populated");
        let rows = &value.rows;
        let version = &value.version;
        let start = offset(&query.cursor, version)?;
        if start > rows.len() {
            return Err(api_error(
                StatusCode::CONFLICT,
                "preparation_orders_changed",
            ));
        }
        let length = if query.cursor.is_empty() {
            query.length.unwrap_or(query.limit)
        } else {
            query.limit
        };
        let end = start.saturating_add(length).min(rows.len());
        let next_cursor = (end < rows.len()).then(|| {
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&(&version, end)).expect("order list cursor"))
        });
        // Material responsibility changes have their own authority and need not
        // advance the production revision. Recheck before releasing the result.
        let latest = read_scope(state, principal, query).await?;
        if latest.key != scope.key
            || state.production_maps.snapshot_revision() != revision
            || (metadata_stamp.is_some()
                && read_metadata_stamp(state, &latest).await? != value.metadata_stamp)
        {
            continue;
        }
        let mut payload = json!({"ok":true,"projection":PROJECTION,"scope":scope.key,"epoch":epoch,
            "rev":revision,"version":version,"apparatus_id":scope.apparatus_id,
            "apparatus":scope.apparatus,"search":query.search,"limit":query.limit,
            "rows":&rows[start..end],"total":rows.len(),
            "next_cursor":next_cursor});
        if scope.all_apparatus {
            payload["all_apparatus"] = true.into();
        }
        return Ok(payload);
    }
    Err(api_error(
        StatusCode::CONFLICT,
        "preparation_orders_changed",
    ))
}

async fn read_metadata_stamp(state: &AppState, scope: &ReadScope) -> Result<String, ApiError> {
    if (!scope.all_apparatus && scope.selected.is_none()) || scope.responsibilities.is_empty() {
        return Ok(String::new());
    }
    store(state)?
        .order_list_metadata_stamp(&scope.apparatus_id, scope.all_apparatus)
        .await
        .map_err(error)
}

fn list_version(scope: &str, epoch: &str, rows: &[OrderRow]) -> String {
    digest((scope, epoch, rows))
}

pub async fn order_list(
    State(state): State<AppState>,
    Query(query): Query<OrderListQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let query = query.normalized()?;
    let principal = authorize_orders(&state, &headers).await?;
    let payload = timeout(READ_TIMEOUT, read_list(&state, &principal, &query))
        .await
        .map_err(|_| api_error(StatusCode::CONFLICT, "preparation_orders_timeout"))??;
    Ok(list_response(&query, payload))
}

fn list_response(query: &OrderListQuery, payload: Value) -> Response {
    let metadata = [
        (
            "x-production-epoch",
            payload["epoch"].as_str().unwrap_or_default().to_string(),
        ),
        ("x-production-revision", payload["rev"].to_string()),
        (
            "x-preparation-scope",
            payload["scope"].as_str().unwrap_or_default().to_string(),
        ),
        (
            "x-preparation-version",
            payload["version"].as_str().unwrap_or_default().to_string(),
        ),
    ];
    let mut response = if query.cursor.is_empty()
        && !query.if_version.is_empty()
        && payload["version"].as_str() == Some(query.if_version.as_str())
    {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        crate::http::json_research::response(payload)
    };
    let response_headers = response.headers_mut();
    response_headers.insert(
        axum::http::header::CACHE_CONTROL,
        "no-store".parse().unwrap(),
    );
    for (name, value) in metadata {
        response_headers.insert(
            name,
            value
                .parse()
                .expect("encoded preparation response metadata"),
        );
    }
    response
}

pub async fn order_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let principal = authorize_orders(&state, &headers).await?;
    if id.len() > 256 || !id.starts_with("zakaz-") {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "preparation_order_invalid",
        ));
    }
    if !store(&state)?
        .order_list_detail_allowed(&principal.ref_, &id)
        .await
        .map_err(error)?
    {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            "preparation_order_not_available",
        ));
    }
    let payload = timeout(READ_TIMEOUT, async {
        for _ in 0..3 {
            let revision = state.production_maps.snapshot_revision();
            let mut saved = state
                .production_maps
                .map(&id)
                .await
                .map_err(|_| {
                    api_error(StatusCode::INTERNAL_SERVER_ERROR, "preparation_order_read")
                })?
                .ok_or_else(|| {
                    api_error(StatusCode::NOT_FOUND, "preparation_order_not_available")
                })?;
            store(&state)?
                .fill_order_customer(&mut saved.map)
                .await
                .map_err(error)?;
            let templates =
                order_page::page_calculation_templates(&state, std::slice::from_ref(&saved)).await;
            if state.production_maps.snapshot_revision() != revision {
                continue;
            }
            let customers = json!({&id:&saved.map.customer_name});
            // The master sheet is a summary: execution authority, progress and
            // cross-machine controls are not visible and are not built here.
            let mut payload = serde_json::to_value(ProductionMapLiveSnapshot {
                maps: vec![saved],
                ..Default::default()
            })
            .expect("preparation order detail");
            payload["ok"] = true.into();
            payload["projection"] = "order_detail_v1".into();
            payload["order_id"] = id.clone().into();
            payload["apparatus"] = "".into();
            payload["epoch"] = state.production_maps.snapshot_epoch().into();
            payload["rev"] = revision.into();
            payload["scope"] = digest((
                &principal.role,
                &principal.ref_,
                "preparation_order_detail_v1",
                &id,
            ))
            .into();
            payload["order_customers"] = customers;
            payload["calculation_templates"] = json!(templates);
            return Ok(payload);
        }
        Err(api_error(
            StatusCode::CONFLICT,
            "preparation_orders_changed",
        ))
    })
    .await
    .map_err(|_| api_error(StatusCode::CONFLICT, "preparation_orders_timeout"))??;
    authorize_orders(&state, &headers).await?;
    if !store(&state)?
        .order_list_detail_allowed(&principal.ref_, &id)
        .await
        .map_err(error)?
    {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            "preparation_order_not_available",
        ));
    }
    Ok(crate::http::json_research::response(payload))
}

pub async fn order_list_live(
    State(state): State<AppState>,
    Query(query): Query<OrderListQuery>,
    axum::Extension(session): axum::Extension<crate::http::router::LiveSession>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let query = query.normalized()?;
    let ws = session.protect(ws, &state, |state, actor| async move {
        require_preparation_access(&state, actor).await.is_ok()
    }).with_denial_notices(
        Some(r#"{"type":"error","status":401,"error":{"error":"unauthorized"}}"#),
        Some(r#"{"type":"error","status":403,"error":{"error":"forbidden"}}"#),
    );
    let principal = require_preparation_access(&state, session.principal).await?;
    read_scope(&state, &principal, &query).await?;
    Ok(ws
        .on_upgrade(move |socket| socket_loop(state, socket, session.digest, principal, query))
        .into_response())
}

async fn send(socket: &mut WebSocket, value: Value) -> bool {
    timeout(
        READ_TIMEOUT,
        socket.send(Message::Text(
            crate::http::json_research::value_to_string(&value).into(),
        )),
    )
    .await
    .is_ok_and(|result| result.is_ok())
}

async fn authorize_live_orders(state: &AppState, digest: &[u8; 32]) -> Result<Principal, ApiError> {
    let principal = state.sessions.get_by_digest(digest).await
        .map_err(|_| api_error(StatusCode::UNAUTHORIZED, "unauthorized"))?;
    require_preparation_access(state, principal).await
}
