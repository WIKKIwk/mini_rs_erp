async fn socket_loop(
    state: AppState,
    mut socket: WebSocket,
    digest: [u8; 32],
    principal: Principal,
    query: OrderListQuery,
) {
    if !query.version.is_empty() {
        optimized_socket_loop(state, socket, digest, principal, query).await;
        return;
    }
    let mut events = state.production_maps.subscribe_live();
    let mut cursor = (
        query.epoch.clone(),
        query.rev.unwrap_or(u64::MAX),
        query.scope.clone(),
    );
    let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    heartbeat.tick().await;
    loop {
        let current = match authorize_live_orders(&state, &digest).await {
            Ok(value) if value.role == principal.role && value.ref_ == principal.ref_ => value,
            Ok(_) => break,
            Err((status, Json(value))) => {
                let _ = send(
                    &mut socket,
                    json!({"type":"error","status":status.as_u16(),"error":value}),
                )
                .await;
                break;
            }
        };
        let scope = match read_scope(&state, &current, &query).await {
            Ok(value) => value,
            Err((status, Json(value))) => {
                let _ = send(
                    &mut socket,
                    json!({"type":"error","status":status.as_u16(),"error":value}),
                )
                .await;
                break;
            }
        };
        let next = (
            state.production_maps.snapshot_epoch().to_string(),
            state.production_maps.snapshot_revision(),
            scope.key,
        );
        let kind = if next != cursor {
            "invalidate"
        } else {
            "ready"
        };
        if !send(
            &mut socket,
            json!({"type":kind,"epoch":next.0,"rev":next.1,"scope":next.2}),
        )
        .await
        {
            break;
        }
        cursor = next;
        tokio::select! {
            event=events.recv()=>{if matches!(event,Err(tokio::sync::broadcast::error::RecvError::Closed)){break;}},
            _=heartbeat.tick()=>{},
            message=socket.recv()=>{
                match message {
                    Some(Ok(Message::Ping(bytes)))=>{if socket.send(Message::Pong(bytes)).await.is_err(){break;}},
                    Some(Ok(Message::Close(_)))|Some(Err(_))|None=>break,
                    _=>{},
                }
            }
        }
    }
    let _ = socket.send(Message::Close(None)).await;
}

/// A prefix is presentation data. Keep the socket alive while replacing only
/// its changed rows; a global revision alone must not force another HTTP read.
fn list_live_frame(previous: Option<&Value>, mut next: Value, query: &OrderListQuery) -> Value {
    let same_scope =
        |before: &Value| before["scope"] == next["scope"] && before["epoch"] == next["epoch"];
    let kind = match previous {
        None if next["version"].as_str() == Some(query.version.as_str())
            && next["scope"].as_str() == Some(query.scope.as_str())
            && next["epoch"].as_str() == Some(query.epoch.as_str()) =>
        {
            "ready"
        }
        None => "list_snapshot",
        Some(before) if !same_scope(before) => "list_snapshot",
        Some(before) if before["version"] == next["version"] => "checkpoint",
        Some(_) => "list_delta",
    };
    if kind == "list_delta" {
        let before = previous.expect("delta has a baseline");
        let old_rows = before["rows"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| Some((row["id"].as_str()?, row)))
            .collect::<BTreeMap<_, _>>();
        let rows = next["rows"].as_array().cloned().unwrap_or_default();
        let ids = rows
            .iter()
            .filter_map(|row| row["id"].as_str())
            .collect::<BTreeSet<_>>();
        let removed = old_rows
            .keys()
            .filter(|id| !ids.contains(**id))
            .copied()
            .collect::<Vec<_>>();
        let upserts = rows
            .iter()
            .filter(|row| {
                row["id"]
                    .as_str()
                    .is_some_and(|id| old_rows.get(id).copied() != Some(*row))
            })
            .cloned()
            .collect::<Vec<_>>();
        next["base_version"] = before["version"].clone();
        next["order_ids"] = json!(rows.iter().map(|row| &row["id"]).collect::<Vec<_>>());
        next["upserts"] = json!(upserts);
        next["removed_ids"] = json!(removed);
    }
    if kind != "list_snapshot" {
        next.as_object_mut()
            .expect("list window object")
            .remove("rows");
    }
    next["type"] = kind.into();
    next
}

async fn optimized_socket_loop(
    state: AppState,
    mut socket: WebSocket,
    digest: [u8; 32],
    principal: Principal,
    mut query: OrderListQuery,
) {
    let mut events = state.production_maps.subscribe_live();
    // A live subscription always reads its loaded prefix, never a cursor page.
    query.cursor.clear();
    query.if_version.clear();
    query.refresh = false;
    let mut previous: Option<Value> = None;
    let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    heartbeat.tick().await;
    loop {
        let current = match authorize_live_orders(&state, &digest).await {
            Ok(value) if value.role == principal.role && value.ref_ == principal.ref_ => value,
            Ok(_) => break,
            Err((status, Json(value))) => {
                let _ = send(
                    &mut socket,
                    json!({"type":"error","status":status.as_u16(),"error":value}),
                )
                .await;
                break;
            }
        };
        let result = timeout(READ_TIMEOUT, read_list(&state, &current, &query)).await;
        let next = match result {
            Ok(Ok(value)) => value,
            result => {
                let (status, Json(value)) =
                    result.ok().and_then(Result::err).unwrap_or_else(|| {
                        api_error(StatusCode::CONFLICT, "preparation_orders_timeout")
                    });
                let kind = if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
                    "error"
                } else {
                    "retry"
                };
                let _ = send(
                    &mut socket,
                    json!({"type":kind,"status":status.as_u16(),"error":value}),
                )
                .await;
                break;
            }
        };
        let frame = list_live_frame(previous.as_ref(), next.clone(), &query);
        if !send(&mut socket, frame).await {
            break;
        }
        previous = Some(next);
        tokio::select! {
            event=events.recv()=>{if matches!(event,Err(tokio::sync::broadcast::error::RecvError::Closed)){break;}},
            _=heartbeat.tick()=>{},
            message=socket.recv()=>{
                match message {
                    Some(Ok(Message::Ping(bytes)))=>{if socket.send(Message::Pong(bytes)).await.is_err(){break;}},
                    Some(Ok(Message::Close(_)))|Some(Err(_))|None=>break,
                    _=>{},
                }
            }
        }
    }
    let _ = socket.send(Message::Close(None)).await;
}
