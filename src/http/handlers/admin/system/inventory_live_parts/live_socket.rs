async fn live_socket(
    state: AppState,
    mut socket: WebSocket,
    digest: [u8; 32],
    subscriptions: Vec<Subscription>,
) {
    // Subscribe before the baseline read: commits racing that read remain queued.
    let mut changes = state.warehouse_events.subscribe_inventory();
    let epoch = format!(
        "{}:{}:{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        NEXT_CONNECTION.fetch_add(1, Ordering::Relaxed)
    );
    let expected_scope = match scope(&state, &digest).await {
        Ok((_, value)) => value,
        Err(error) => {
            fail(&mut socket, error.into()).await;
            return;
        }
    };
    let mut views = match initial_views(
        &state,
        &digest,
        &mut socket,
        &epoch,
        &expected_scope,
        subscriptions,
    )
    .await
    {
        Ok(views) => views,
        Err(error) => {
            fail(&mut socket, error).await;
            return;
        }
    };
    let mut heartbeat = tokio::time::interval(Duration::from_secs(20));
    heartbeat.tick().await;
    loop {
        tokio::select! {
            received = changes.recv() => {
                if matches!(received, Err(tokio::sync::broadcast::error::RecvError::Closed)) { break }
                tokio::time::sleep(Duration::from_millis(75)).await;
                // A lag is repaired by re-reading this bounded canonical window,
                // not by trusting incomplete notifications or broadcasting rows.
                for _ in 0..128 { if changes.try_recv().is_err() { break } }
                if let Err(error) = refresh_views(&state, &digest, &mut socket, &epoch, &expected_scope, &mut views).await {
                    fail(&mut socket, error).await; break
                }
            }
            inbound = socket.recv() => {
                match inbound {
                    Some(Ok(Message::Text(text))) if text.len() <= MAX_MESSAGE_BYTES => {
                        let message = serde_json::from_str::<SubscribeMessage>(&text);
                        let Ok(message) = message else { break };
                        if message.kind != "subscribe" || validate_subscriptions(&message.subscriptions).is_err() { break }
                        match initial_views(&state, &digest, &mut socket, &epoch, &expected_scope, message.subscriptions).await {
                            Ok(loaded) => views = loaded,
                            Err(error) => { fail(&mut socket, error).await; break }
                        }
                    }
                    Some(Ok(Message::Ping(bytes))) => if socket.send(Message::Pong(bytes)).await.is_err() { break },
                    Some(Ok(Message::Pong(_))) => {},
                    _ => break,
                }
            }
            _ = heartbeat.tick() => {
                match scope(&state, &digest).await {
                    Ok((_, value)) if value == expected_scope => {},
                    Ok(_) => { fail(&mut socket, LiveError::Scope).await; break },
                    Err(error) => { fail(&mut socket, error.into()).await; break },
                }
                if !send(&mut socket, json!({"event":"inventory.heartbeat","epoch":epoch})).await { break }
            }
        }
    }
}
