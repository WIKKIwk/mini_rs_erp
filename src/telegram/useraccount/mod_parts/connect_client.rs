async fn connect_client(
    api_id: i32,
    api_hash: &str,
    session: Option<&str>,
) -> Result<(Client, ShutdownToken), UserAccountError> {
    ferogram::Client::builder()
        .api_id(api_id)
        .api_hash(api_hash)
        .session_string(session.unwrap_or_default())
        // Pyrofork's working userbot uses a single Abridged connection.  Do
        // not enable ferogram's fresh-connection transport race here: it
        // deliberately replaces the requested transport with Full or
        // Obfuscated on a new login and can produce a different Telegram
        // authentication path from the one that was tested successfully.
        .transport(TransportKind::Abridged)
        .probe_transport(false)
        .device_model("Mini RS ERP")
        .system_version("Rust")
        .app_version("Mini RS ERP 1.0")
        .system_lang_code("en-US")
        .lang_code("en")
        .resilient_connect(true)
        .connect()
        .await
        .map_err(|error| UserAccountError::Transport(error.to_string()))
}

async fn api_credentials(
    store: &TelegramStore,
) -> Result<(i32, String), UserAccountError> {
    store
        .user_api_credentials()
        .await
        .map_err(|error| UserAccountError::Store(error.to_string()))?
        .ok_or(UserAccountError::NotConfigured)
}

async fn list_writable_groups(client: &Client) -> Result<Vec<TelegramUserGroup>, UserAccountError> {
    let mut groups = Vec::new();
    for folder_id in [0, 1] {
        let mut iter = client.iter_dialogs().folder_id(Some(folder_id));
        while let Some(dialog) = iter.next(client).await.map_err(map_transport)? {
            if let Some(group) = writable_group_from_dialog(&dialog) {
                groups.push(group);
            }
        }
    }
    groups.sort_by(|left, right| {
        left.chat_type
            .cmp(&right.chat_type)
            .then_with(|| left.chat_id.cmp(&right.chat_id))
    });
    groups
        .dedup_by(|left, right| left.chat_type == right.chat_type && left.chat_id == right.chat_id);
    groups.sort_by(|left, right| {
        left.title
            .to_lowercase()
            .cmp(&right.title.to_lowercase())
            .then_with(|| left.chat_id.cmp(&right.chat_id))
    });
    Ok(groups)
}

async fn send_to_selected_group(
    client: &Client,
    selected_chat_id: &str,
    selected_chat_type: &str,
    text: &str,
) -> Result<(), UserAccountError> {
    let peer = selected_group_peer(client, selected_chat_id, selected_chat_type).await?;
    client
        .send_message(peer, InputMessage::text(text))
        .await
        .map_err(map_transport)?;
    Ok(())
}
