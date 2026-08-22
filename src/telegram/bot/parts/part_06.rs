#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OrderInlineKind {
    Customer,
    Product,
    Material,
    Micron,
}

fn parse_order_inline_query(query: &str) -> Option<(OrderInlineKind, String)> {
    let mut parts = query.trim_start().splitn(2, char::is_whitespace);
    let prefix = parts.next()?.to_ascii_lowercase();
    let value = parts.next().unwrap_or_default().trim().to_string();
    let kind = match prefix.as_str() {
        "c7" => OrderInlineKind::Customer,
        "i7" => OrderInlineKind::Product,
        "m7" => OrderInlineKind::Material,
        "n7" => OrderInlineKind::Micron,
        _ => return None,
    };
    Some((kind, value))
}

async fn order_inline_results(
    service: &TelegramService,
    telegram_user_id: &str,
    query: &str,
) -> Result<Vec<serde_json::Value>, TelegramError> {
    let Some((kind, value)) = parse_order_inline_query(query) else {
        return Ok(Vec::new());
    };
    let Some(draft) = service.order_draft(telegram_user_id).await? else {
        return Ok(Vec::new());
    };
    let catalog = service.order_catalog().await?;
    let mut results = Vec::new();
    match kind {
        OrderInlineKind::Customer if draft.step == TelegramOrderStep::Customer => {
            for customer in catalog
                .search_customers(&value, 20)
                .await
                .map_err(TelegramError::OrderCatalog)?
            {
                let token = service
                    .remember_order_choice(telegram_user_id, customer.ref_.clone())
                    .await;
                results.push(inline_article(
                    &token,
                    &customer.name,
                    &customer.ref_,
                    &format!("Mijoz: {}", customer.name),
                    &format!("order:customer:{token}"),
                ));
            }
        }
        OrderInlineKind::Product if draft.step == TelegramOrderStep::Product => {
            for item in catalog
                .search_customer_items(&draft.customer_ref, &value, 20)
                .await
                .map_err(TelegramError::OrderCatalog)?
            {
                let token = service
                    .remember_order_choice(telegram_user_id, item.code.clone())
                    .await;
                results.push(inline_article(
                    &token,
                    &item.name,
                    &format!("{} · {}", item.code, item.uom),
                    &format!("Mahsulot: {}", item.name),
                    &format!("order:product:{token}"),
                ));
            }
        }
        OrderInlineKind::Material if draft.step == TelegramOrderStep::Material => {
            for material in catalog
                .search_materials(&value, 20)
                .await
                .map_err(TelegramError::OrderCatalog)?
            {
                let token = service
                    .remember_order_choice(telegram_user_id, material.id.clone())
                    .await;
                results.push(inline_article(
                    &token,
                    &material.name,
                    &format!("{} ta mikron", material.variants.len()),
                    &format!("Material: {}", material.name),
                    &format!("order:material:{token}"),
                ));
            }
        }
        OrderInlineKind::Micron if draft.step == TelegramOrderStep::Micron => {
            let Some(material) = catalog
                .material_by_id(&draft.pending_material_id)
                .await
                .map_err(TelegramError::OrderCatalog)?
            else {
                return Ok(Vec::new());
            };
            for variant in material
                .variants
                .into_iter()
                .filter(|variant| value.is_empty() || variant.micron.to_string().contains(&value))
            {
                let micron = variant.micron.to_string();
                let token = service
                    .remember_order_choice(telegram_user_id, micron.clone())
                    .await;
                results.push(inline_article(
                    &token,
                    &format!("{} mikron", micron),
                    &material.name,
                    &format!("{} {} mikron", material.name, micron),
                    &format!("order:micron:{token}"),
                ));
            }
        }
        _ => {}
    }
    Ok(results)
}

fn inline_article(
    id: &str,
    title: &str,
    description: &str,
    message_text: &str,
    callback_data: &str,
) -> serde_json::Value {
    serde_json::json!({
        "type": "article",
        "id": id,
        "title": title,
        "description": description,
        "input_message_content": {"message_text": message_text},
        "reply_markup": {
            "inline_keyboard": [[{
                "text": "✅ Tanlash",
                "callback_data": callback_data
            }]]
        }
    })
}

fn customer_step_keyboard() -> serde_json::Value {
    serde_json::json!({
        "inline_keyboard": [
            [{"text": "🔎 Mijoz tanlash", "switch_inline_query_current_chat": INLINE_CUSTOMER_PREFIX}],
            [{"text": "➕ Mijoz qo‘shish", "callback_data": "order:add_customer"}],
            [{"text": "❌ Bekor qilish", "callback_data": "order:cancel"}]
        ]
    })
}

fn product_step_keyboard() -> serde_json::Value {
    serde_json::json!({
        "inline_keyboard": [
            [{"text": "🔎 Mahsulot tanlash", "switch_inline_query_current_chat": INLINE_PRODUCT_PREFIX}],
            [{"text": "➕ Mahsulot qo‘shish", "callback_data": "order:add_product"}],
            [{"text": "❌ Bekor qilish", "callback_data": "order:cancel"}]
        ]
    })
}

fn status_keyboard() -> serde_json::Value {
    serde_json::json!({
        "inline_keyboard": [
            [
                {"text": "🧻 Rulon", "callback_data": "order:status:roll"},
                {"text": "📦 Paket", "callback_data": "order:status:package"}
            ],
            [{"text": "❌ Bekor qilish", "callback_data": "order:cancel"}]
        ]
    })
}

fn material_step_keyboard() -> serde_json::Value {
    serde_json::json!({
        "inline_keyboard": [
            [{"text": "🔎 Material tanlash", "switch_inline_query_current_chat": INLINE_MATERIAL_PREFIX}],
            [{"text": "❌ Bekor qilish", "callback_data": "order:cancel"}]
        ]
    })
}

fn micron_step_keyboard() -> serde_json::Value {
    serde_json::json!({
        "inline_keyboard": [
            [{"text": "🔎 Mikron tanlash", "switch_inline_query_current_chat": INLINE_MICRON_PREFIX}],
            [{"text": "❌ Bekor qilish", "callback_data": "order:cancel"}]
        ]
    })
}

fn layer_options_keyboard() -> serde_json::Value {
    serde_json::json!({
        "inline_keyboard": [
            [{"text": "+1 qavat", "callback_data": "order:add_layer"}],
            [{"text": "Keyingi", "callback_data": "order:next_layers"}],
            [{"text": "❌ Bekor qilish", "callback_data": "order:cancel"}]
        ]
    })
}

fn parse_tiraj(value: &str) -> Option<f64> {
    let value = value.trim().replace(',', ".");
    let tiraj = value.parse::<f64>().ok()?;
    tiraj
        .is_finite()
        .then_some(tiraj)
        .filter(|value| *value > 0.0)
}

async fn get_telegram_file(
    service: &TelegramService,
    token: &str,
    file_id: &str,
) -> Result<TelegramFile, TelegramError> {
    request_json(
        service,
        token,
        "getFile",
        &serde_json::json!({"file_id": file_id}),
    )
    .await
}

async fn download_telegram_file(
    service: &TelegramService,
    token: &str,
    file_path: &str,
) -> Result<Vec<u8>, TelegramError> {
    let response = service
        .http_client()
        .get(format!("{TELEGRAM_FILE_BASE}{token}/{file_path}"))
        .send()
        .await
        .map_err(|error| TelegramError::Transport(error.to_string()))?;
    let status = response.status();
    if !status.is_success() {
        return Err(TelegramError::Transport(format!(
            "telegram file download returned HTTP {status}"
        )));
    }
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|error| TelegramError::Transport(error.to_string()))
}

async fn request_json<T: for<'de> Deserialize<'de> + Default, P: Serialize>(
    service: &TelegramService,
    token: &str,
    method: &str,
    payload: &P,
) -> Result<T, TelegramError> {
    let response = service
        .http_client()
        .post(bot_url(token, method))
        .json(payload)
        .send()
        .await
        .map_err(|error| TelegramError::Transport(error.to_string()))?;
    parse_api_response(response).await
}

async fn parse_api_response<T: for<'de> Deserialize<'de> + Default>(
    response: reqwest::Response,
) -> Result<T, TelegramError> {
    let status = response.status();
    let payload = response
        .json::<TelegramApiResponse<T>>()
        .await
        .map_err(|error| TelegramError::Transport(error.to_string()))?;
    if !status.is_success() || !payload.ok {
        return Err(TelegramError::Transport(
            payload
                .description
                .unwrap_or_else(|| format!("telegram api http status {status}")),
        ));
    }
    payload
        .result
        .ok_or_else(|| TelegramError::Transport("telegram api returned no result".to_string()))
}
