async fn refresh_feed(
    state: &AppState,
    feed: &super::live_hub::StateFeed,
    principal: &Principal,
    include_requests: bool,
    worker_scope: bool,
    colour_event: Option<&ProductionMapLiveEvent>,
    window: &super::worker_snapshot::WorkerQueueWindow,
    force: bool,
) -> Result<(), AdminError> {
    // One async build per authorized actor/window/revision. Other sockets wait
    // for this same result rather than repeat database reads and serialization.
    let mut data = feed.data.lock().await;
    data.used = std::time::Instant::now();
    if !force
        && data
            .view
            .as_ref()
            .is_some_and(|view| view.revision == state.production_maps.snapshot_revision())
    {
        // A long-lived socket keeps its original registry key when an
        // assignment changes. Its feed may now contain another scope at this
        // same production revision. Recheck the live authorization before
        // sharing that cached view with a reconnect under the old key.
        let scope_matches = if worker_scope {
            let (_, assignment_scope) =
                super::worker_snapshot::scope_token(state, principal).await?;
            data.view.as_ref().is_some_and(|view| {
                view.scope == window.scope(&assignment_scope)
                    && view.revision == state.production_maps.snapshot_revision()
            })
        } else {
            true
        };
        if scope_matches {
            return Ok(());
        }
    }
    let colour_revision = colour_event.and_then(|event| match event {
        ProductionMapLiveEvent::PrintPreflight { revision, .. } => Some(*revision),
        _ => None,
    });
    if !force && worker_scope {
        if let (
            Some(before),
            Some(ProductionMapLiveEvent::PrintPreflight {
                revision,
                order_id,
                apparatus,
            }),
        ) = (data.view.as_ref(), colour_event)
        {
            if colour_history_unchanged(
                before.revision,
                state.production_maps.snapshot_revision(),
                *revision,
            ) && preflight_outside_view(&before.dependency_snapshot, order_id, apparatus)
            {
                let (assigned, scope) =
                    super::worker_snapshot::scope_token(state, principal).await?;
                if window.scope(&scope) == before.scope
                    && !queue_state::apparatus_matches_assigned(apparatus, &assigned)
                    && state.production_maps.snapshot_revision() == *revision
                {
                    // Exact evidence of a disjoint colour-only commit can be
                    // retained without rebuilding the graph or reading history.
                    data.advance_unchanged(*revision);
                    return Ok(());
                }
            }
        }
    }
    let next = read_view(
        state,
        principal,
        include_requests,
        worker_scope,
        colour_revision.and_then(|revision| data.view.as_ref().map(|view| (view, revision))),
        window,
    )
    .await?;
    let patch = data.view.as_ref().and_then(|before| {
        if before.scope != next.scope || before.epoch != next.epoch {
            return None;
        }
        let patch = if window.worker_list {
            json_research::to_string(&worker_list::patch(before, &next)).expect("worker list patch serialization").into()
        } else {
            let common = state.production_state_live.snapshot_patch_for_protocol(&before.snapshot, &next.snapshot, data.compact);
            super::live_hub::join_patch(&common, &personal_patch(before, &next))
        };
        (patch.len() <= MAX_STATE_DELTA_BYTES.saturating_sub(512)).then_some(patch)
    });
    data.replace(next, patch);
    Ok(())
}

struct Delivery {
    frames: Vec<Utf8Bytes>,
    epoch: String,
    scope: String,
    revision: u64,
    resync: bool,
    compact: bool,
}

async fn delivery(
    feed: &super::live_hub::StateFeed,
    client_epoch: &str,
    client_revision: Option<u64>,
    client_scope: &str,
    checkpoint: bool,
) -> Delivery {
    let mut data = feed.data.lock().await;
    data.used = std::time::Instant::now();
    let view = data.view.as_ref().expect("initialized feed");
    let epoch = view.epoch.clone();
    let scope = view.scope.clone();
    let revision = view.revision;
    let history = if client_epoch == epoch && client_scope == scope {
        client_revision.and_then(|from| data.history.recover(from, revision))
    } else {
        None
    };
    let Some(history) = history else {
        return Delivery {
            frames: Vec::new(),
            epoch,
            scope,
            revision,
            resync: true,
            compact: data.compact,
        };
    };
    let mut cursor = client_revision.expect("recovery requires cursor");
    let mut frames = Vec::new();
    for delta in &history {
        if delta.empty {
            continue;
        }
        frames.push(delta.frame(cursor));
        cursor = delta.revision;
    }
    if checkpoint && cursor != revision {
        // A heartbeat or successful catch-up advances proven empty revisions
        // without rebuilding rows. During normal live traffic no-op events are
        // silent, and the next material delta is rebased over this proven chain.
        let last = history.last().expect("non-empty revision gap");
        frames.push(last.frame(cursor));
        cursor = revision;
    }
    Delivery {
        frames,
        epoch,
        scope,
        revision: cursor,
        resync: false,
        compact: data.compact,
    }
}

async fn send_delivery(socket: &mut WebSocket, delivery: &Delivery, ready: bool) -> bool {
    for frame in &delivery.frames {
        if !timeout(
            Duration::from_secs(15),
            socket.send(Message::Text(frame.clone())),
        )
        .await
        .is_ok_and(|result| result.is_ok())
        {
            return false;
        }
    }
    if ready || delivery.resync {
        let mut value = json!({
            "ok": true,
            "type": if ready { "state_ready" } else { "state_resync" },
            "epoch": delivery.epoch, "scope": delivery.scope, "rev": delivery.revision,
            "resync": delivery.resync, "recovered": !delivery.resync,
            "recovery_supported": true,
        });
        if delivery.compact {
            value["protocol"] = json!("state_delta_v2");
        }
        return send(socket, value).await;
    }
    true
}

pub(super) fn production_map_state_live_socket(
    state: AppState,
    mut socket: WebSocket,
    principal: Principal,
    include_requests: bool,
    mut client_epoch: String,
    mut client_revision: Option<u64>,
    worker_scope: bool,
    mut client_scope: String,
    window: super::worker_snapshot::WorkerQueueWindow,
    compact: bool,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    Box::pin(async move {
        // Subscribe before any read so the initial cursor cannot miss a commit.
        let mut events = state.production_maps.subscribe_live();
        let assignment_scope = if worker_scope {
            match super::worker_snapshot::scope_token(&state, &principal).await {
                Ok((_, scope)) => scope,
                Err(error) => {
                    let _ = send(&mut socket, json!(error.1.0)).await;
                    return;
                }
            }
        } else {
            String::new()
        };
        let key = serde_json::to_string(&(
            state.production_maps.snapshot_epoch(),
            principal.role,
            &principal.ref_,
            include_requests,
            worker_scope,
            window.scope(&assignment_scope),
        ))
        .expect("authorized feed key");
        let feed = state.production_state_live.feed_for_protocol(key, compact);
        if let Err(error) = refresh_feed(
            &state,
            &feed,
            &principal,
            include_requests,
            worker_scope,
            None,
            &window,
            false,
        )
        .await
        {
            let _ = send(&mut socket, json!(error.1.0)).await;
            return;
        }
        let initial = delivery(&feed, &client_epoch, client_revision, &client_scope, true).await;
        if !send_delivery(&mut socket, &initial, true).await {
            return;
        }
        client_epoch = initial.epoch;
        client_scope = initial.scope;
        client_revision = Some(initial.revision);
        let mut heartbeat = tokio::time::interval(Duration::from_secs(25));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // The initial handshake already checked scope and advanced the cursor.
        heartbeat.tick().await;
        loop {
            tokio::select! {
                inbound = socket.recv() => match inbound {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                },
                received = events.recv() => {
                    let colour_event = match received {
                        Ok(event @ ProductionMapLiveEvent::PrintPreflight { .. }) => Some(event),
                        Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => None,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    };
                    if let Err(error) = refresh_feed(&state, &feed, &principal, include_requests,
                        worker_scope, colour_event.as_ref(), &window, false).await {
                        let _ = send(&mut socket, json!(error.1.0)).await; break;
                    }
                    let next = delivery(&feed, &client_epoch, client_revision, &client_scope, false).await;
                    if !send_delivery(&mut socket, &next, false).await { break; }
                    client_epoch = next.epoch;
                    client_scope = next.scope;
                    client_revision = Some(next.revision);
                },
                _ = heartbeat.tick() => {
                    if super::completion::require_any_live_capability(&state, &principal).await.is_err() {
                        let _ = send(&mut socket, json!({"ok": false, "error": "forbidden"})).await;
                        break;
                    }
                    let scope_changed = if worker_scope {
                        match super::worker_snapshot::scope_token(&state, &principal).await {
                            Ok((_, scope)) => window.scope(&scope) != client_scope,
                            Err(_) => { let _ = send(&mut socket, json!({"ok": false, "error": "forbidden"})).await; break; }
                        }
                    } else { false };
                    if let Err(error) = refresh_feed(&state, &feed, &principal, include_requests,
                        worker_scope, None, &window, scope_changed).await {
                        let _ = send(&mut socket, json!(error.1.0)).await; break;
                    }
                    let next = delivery(&feed, &client_epoch, client_revision, &client_scope, true).await;
                    if !send_delivery(&mut socket, &next, false).await { break; }
                    client_epoch = next.epoch;
                    client_scope = next.scope;
                    client_revision = Some(next.revision);
                    // Native keepalive and Pong deadlines belong to the transport.
                }
            }
        }
    })
}
