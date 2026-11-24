pub async fn production_tasks_live(
    State(state): State<AppState>,
    Query(query): Query<TasksQuery>,
    axum::Extension(session): axum::Extension<crate::http::router::LiveSession>,
    ws: WebSocketUpgrade,
) -> Result<Response, AdminError> {
    let query = query.normalized()?;
    let capability = task_capability(&query.kind, session.principal.role)?;
    let ws = session.protect(ws, &state, move |state, actor| async move {
        state.admin.principal_has_capability(&actor, capability).await
    }).with_denial_notices(
        Some(r#"{"ok":false,"type":"tasks_error","error":"unauthorized","status":401,"resync":false}"#),
        Some(r#"{"ok":false,"type":"tasks_error","error":"forbidden","status":403,"resync":false}"#),
    );
    let principal = session.principal;
    read_scope(&state, &principal, &query).await?;
    Ok(ws
        .on_upgrade(move |socket| socket_loop(state, socket, principal, session.digest, query))
        .into_response())
}

fn delta(before: &TaskWindow, after: &TaskWindow) -> Value {
    let previous = before
        .rows
        .iter()
        .map(|row| (&row.id, row))
        .collect::<BTreeMap<_, _>>();
    let next = after
        .rows
        .iter()
        .map(|row| (&row.id, row))
        .collect::<BTreeMap<_, _>>();
    let upsert = after
        .rows
        .iter()
        .filter(|row| previous.get(&row.id).copied() != Some(*row))
        .collect::<Vec<_>>();
    let remove = before
        .rows
        .iter()
        .filter(|row| !next.contains_key(&row.id))
        .map(|row| &row.id)
        .collect::<Vec<_>>();
    json!({"ok":true,"type":"tasks_delta","epoch":after.epoch,"scope":after.scope,
        "base_rev":before.rev,"rev":after.rev,"version":after.version,
        "checked_count":after.checked_count,"missing_count":after.missing_count,
        "order_ids":after.rows.iter().map(|row| &row.id).collect::<Vec<_>>(),"upsert":upsert,"remove":remove})
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

async fn socket_loop(
    state: AppState,
    mut socket: WebSocket,
    principal: Principal,
    digest: [u8; 32],
    query: TasksQuery,
) {
    // Subscribe before reading; an intervening commit remains queued.
    let mut events = state.production_maps.subscribe_live();
    let mut previous = match read_window(&state, &principal, &query).await {
        Ok(window) => window,
        Err(error) => {
            let _ = send_error(&mut socket, &error).await;
            return;
        }
    };
    let initial = if query.epoch == previous.epoch
        && query.scope == previous.scope
        && query.rev == Some(previous.rev)
    {
        json!({"ok":true,"type":"tasks_ready","epoch":previous.epoch,"scope":previous.scope,
            "rev":previous.rev,"version":previous.version,"resync":false})
    } else {
        json!({"ok":true,"type":"tasks_snapshot","window":&*previous})
    };
    if !send(&mut socket, initial).await {
        return;
    }
    let mut heartbeat = tokio::time::interval(HEARTBEAT);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    heartbeat.tick().await;
    loop {
        let recheck = tokio::select! {
            event = events.recv() => match event { Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => true, Err(_) => break },
            _ = heartbeat.tick() => false,
            message = socket.recv() => {
                match message {
                    Some(Ok(Message::Ping(bytes))) => { if socket.send(Message::Pong(bytes)).await.is_err() { break; } },
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    _ => {},
                }
                continue;
            }
        };
        // A long-lived socket must not retain revoked capabilities or assignments.
        let current_principal = match state.sessions.get_by_digest(&digest).await {
            Ok(value) => value,
            Err(_) => {
                let _ = send_error(&mut socket, &unauthorized()).await;
                break;
            }
        };
        if current_principal.role != principal.role || current_principal.ref_ != principal.ref_ {
            let _ = send_error(&mut socket, &unauthorized()).await;
            break;
        }
        let scope = match read_scope(&state, &current_principal, &query).await {
            Ok(value) => value,
            Err(error) => {
                let _ = send_error(&mut socket, &error).await;
                break;
            }
        };
        if !recheck
            && scope.scope == previous.scope
            && state.production_maps.snapshot_revision() == previous.rev
        {
            if !send(
                &mut socket,
                json!({"ok":true,"type":"tasks_ready","epoch":previous.epoch,"scope":previous.scope,
                "rev":previous.rev,"version":previous.version,"resync":false}),
            )
            .await
            {
                break;
            }
            continue;
        }
        let next = match read_window(&state, &current_principal, &query).await {
            Ok(window) => window,
            Err(error) => {
                let _ = send_error(&mut socket, &error).await;
                break;
            }
        };
        if next.scope != previous.scope
            || next.epoch != previous.epoch
            || next.apparatus != previous.apparatus
        {
            if !send(
                &mut socket,
                json!({"ok":true,"type":"tasks_snapshot","window":&*next}),
            )
            .await
            {
                break;
            }
        } else if next.rev != previous.rev || next.version != previous.version {
            // Empty task changes still advance this socket's exact cursor.
            if !send(&mut socket, delta(&previous, &next)).await {
                break;
            }
        }
        previous = next;
    }
    let _ = socket.send(Message::Close(None)).await;
}

async fn send_error(socket: &mut WebSocket, error: &AdminError) -> bool {
    let terminal = matches!(error.0, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN);
    send(socket, json!({"ok":false,"type":if terminal { "tasks_error" } else { "tasks_resync" },
        "error":if error.0 == StatusCode::UNAUTHORIZED { "unauthorized" } else if terminal { "forbidden" } else { "supply_tasks_resync" },
        "status":error.0.as_u16(),"resync":!terminal})).await
}
