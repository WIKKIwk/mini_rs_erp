async fn update(
    state: &AppState,
    principal: &Principal,
    socket: &mut WebSocket,
    subscriptions: &mut [Subscription],
) -> UpdateStatus {
    let mut pending = false;
    for sub in subscriptions {
        let mut feed = sub.feed.lock().await;
        let current = match feed.current(state, principal, &sub.request).await {
            Ok(current) => current,
            Err(error)
                if error.0 == StatusCode::CONFLICT && error.1.0.error == "order_page_changed" =>
            {
                // A concurrent write invalidated staging. Keep the last sent
                // baseline and retry the whole window after writes settle.
                drop(feed);
                pending = true;
                continue;
            }
            Err(error) => {
                drop(feed);
                let _ = send(socket, &json_research::value_to_string(&json!(error.1.0))).await;
                return UpdateStatus::Closed;
            }
        };
        let baseline = sub
            .baseline
            .clone()
            .or_else(|| feed.version(&sub.request.version));
        drop(feed);
        let initial = sub.baseline.is_none();
        if let Some(before) = baseline {
            if before.version != current.version {
                if !send(socket, &prepared(&before, &current, &sub.request.id).await).await {
                    return UpdateStatus::Closed;
                }
            }
            if initial
                && !send(
                    socket,
                    &json_research::value_to_string(&json!({"ok":true,"type":"page_window_ready",
                "window":sub.request.id,"epoch":current.epoch,"rev":current.revision,
                "version":current.version,"resync":false})),
                )
                .await
            {
                return UpdateStatus::Closed;
            }
        } else {
            if !send(
                socket,
                &json_research::value_to_string(&json!({"ok":true,"type":"page_window_ready",
                "window":sub.request.id,"epoch":current.epoch,"rev":current.revision,
                "version":current.version,"resync":sub.request.version != current.version})),
            )
            .await
            {
                return UpdateStatus::Closed;
            }
        }
        sub.baseline = Some(current);
    }
    if pending {
        UpdateStatus::Pending
    } else {
        UpdateStatus::Complete
    }
}

pub(super) async fn socket(
    state: AppState,
    mut socket: WebSocket,
    principal: Principal,
    requests: Vec<WindowRequest>,
) {
    let mut events = state.production_maps.subscribe_live();
    let mut subscriptions = Vec::new();
    for request in requests {
        let feed = state
            .production_page_live
            .subscribe(request.key(&principal))
            .await;
        subscriptions.push(Subscription {
            request,
            feed,
            baseline: None,
        });
    }
    let status = update(&state, &principal, &mut socket, &mut subscriptions).await;
    if status == UpdateStatus::Closed {
        return;
    }
    let mut retry_pending = status == UpdateStatus::Pending;
    let mut retry = tokio::time::interval(RETRY_READ_DELAY);
    retry.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    retry.reset();
    let mut heartbeat = tokio::time::interval(Duration::from_secs(25));
    heartbeat.tick().await;
    loop {
        tokio::select! {
            message = socket.recv() => match message {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                _ => {}
            },
            event = events.recv() => {
                if matches!(event, Err(tokio::sync::broadcast::error::RecvError::Closed)) { break; }
                // Pending reads retry on the timer, even if this was the last
                // notification. Draining events here avoids a conflict loop.
                if retry_pending { continue; }
                // Reads after lag use a fresh canonical view and diff from the
                // subscriber's retained baseline, so no partial chain is applied.
                let status = update(&state, &principal, &mut socket, &mut subscriptions).await;
                if status == UpdateStatus::Closed { break; }
                retry_pending = status == UpdateStatus::Pending;
                if retry_pending { retry.reset(); }
            },
            _ = retry.tick(), if retry_pending => {
                let status = update(&state, &principal, &mut socket, &mut subscriptions).await;
                if status == UpdateStatus::Closed { break; }
                retry_pending = status == UpdateStatus::Pending;
                if retry_pending { retry.reset(); }
            },
            _ = heartbeat.tick() => {
                if super::completion::require_paged_live_capability(&state, &principal).await.is_err() {
                    let _ = send(&mut socket, &json_research::value_to_string(&json!({"ok":false,"error":"forbidden"}))).await;
                    break;
                }
            }
        }
    }
}

pub(super) async fn notifications_socket(
    state: AppState,
    mut socket: WebSocket,
    principal: Principal,
    include_requests: bool,
) {
    let mut events = state.production_maps.subscribe_live();
    let mut previous: Option<Arc<str>> = None;
    let mut heartbeat = tokio::time::interval(Duration::from_secs(25));
    heartbeat.tick().await;
    loop {
        let frame = if include_requests {
            match state.production_page_live.notifications(&state).await {
                Ok(frame) => frame,
                Err(error) => {
                    let _ = send(&mut socket, &json_research::value_to_string(&json!(error.1.0))).await;
                    break;
                }
            }
        } else {
            // Preserve legacy role filtering: only Admin receives requests.
            previous.clone().unwrap_or_else(|| {
                json_research::value_to_string(&json!({"ok":true,
                "type":"notifications_snapshot","epoch":state.production_maps.snapshot_epoch(),
                "rev":state.production_maps.snapshot_revision(),"completion_requests":[]}))
                .into()
            })
        };
        if previous
            .as_ref()
            .is_none_or(|old| !Arc::ptr_eq(old, &frame))
            && !send(&mut socket, &frame).await
        {
            break;
        }
        previous = Some(frame);
        tokio::select! {
            event = events.recv() => {
                if matches!(event, Err(tokio::sync::broadcast::error::RecvError::Closed)) { break; }
            },
            message = socket.recv() => match message {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                _ => {}
            },
            _ = heartbeat.tick() => {
                if super::completion::require_any_live_capability(&state, &principal).await.is_err() {
                    let _ = send(&mut socket, &json_research::value_to_string(&json!({"ok":false,"error":"forbidden"}))).await;
                    break;
                }
            }
        }
    }
}
