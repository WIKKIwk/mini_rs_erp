#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OrderInlineKind {
    Customer,
    Product,
    Material,
}

fn parse_order_inline_query(query: &str) -> Option<(OrderInlineKind, String)> {
    let mut parts = query.trim_start().splitn(2, char::is_whitespace);
    let prefix = parts.next()?.to_ascii_lowercase();
    let value = parts.next().unwrap_or_default().trim().to_string();
    let kind = match prefix.as_str() {
        "c7" => OrderInlineKind::Customer,
        "i7" => OrderInlineKind::Product,
        "m7" => OrderInlineKind::Material,
        _ => return None,
    };
    Some((kind, value))
}

fn parse_group_inline_query(query: &str) -> Option<String> {
    let mut parts = query.trim_start().splitn(2, char::is_whitespace);
    let prefix = parts.next()?.to_ascii_lowercase();
    if prefix != "g7" {
        return None;
    }
    Some(parts.next().unwrap_or_default().trim().to_string())
}

async fn group_inline_results(
    service: &TelegramService,
    telegram_user_id: &str,
    query: &str,
) -> Result<Vec<serde_json::Value>, TelegramError> {
    let Some(value) = parse_group_inline_query(query) else {
        return Ok(Vec::new());
    };
    let query_key = value.to_lowercase();
    let mut groups = service
        .writable_user_groups(telegram_user_id)
        .await?
        .into_iter()
        .filter(|group| {
            query_key.is_empty()
                || group.title.to_lowercase().contains(&query_key)
                || group.username.to_lowercase().contains(&query_key)
        })
        .collect::<Vec<_>>();
    groups.sort_by_key(|group| group.title.to_lowercase());

    let mut results = Vec::new();
    for group in groups.into_iter().take(20) {
        let choice = format!("{}:{}", group.chat_type, group.chat_id);
        let result_id = format!("group-{}", group.chat_id);
        let description = if group.username.trim().is_empty() {
            group.chat_type.clone()
        } else {
            format!(
                "{} · @{}",
                group.chat_type,
                group.username.trim_start_matches('@')
            )
        };
        let token = service
            .remember_order_choice(telegram_user_id, choice)
            .await;
        results.push(inline_article(
            &result_id,
            &group.title,
            &description,
            &format!("Guruh: {}", group.title),
            &format!("user_group_inline:{token}"),
        ));
    }
    Ok(results)
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
                results.push(inline_article_without_markup(
                    &format!("customer-{}", customer.ref_),
                    &customer.name,
                    &customer.ref_,
                    &format!("Mijoz: {}", customer.name),
                ));
            }
        }
        OrderInlineKind::Product if draft.step == TelegramOrderStep::Product => {
            for item in catalog
                .search_customer_items(&draft.customer_ref, &value, 20)
                .await
                .map_err(TelegramError::OrderCatalog)?
            {
                results.push(inline_article_without_markup(
                    &format!("product-{}", item.code),
                    &item.name,
                    &format!("{} · {}", item.code, item.uom),
                    &format!("Mahsulot: {}", item.name),
                ));
            }
        }
        OrderInlineKind::Material if draft.step == TelegramOrderStep::Material => {
            for material in catalog
                .search_materials(&value, 20)
                .await
                .map_err(TelegramError::OrderCatalog)?
            {
                results.push(inline_article_without_markup(
                    &format!("material-{}", material.id),
                    &material.name,
                    &format!("{} ta mikron", material.variants.len()),
                    &format!("Material: {}", material.name),
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

fn inline_article_without_markup(
    id: &str,
    title: &str,
    description: &str,
    message_text: &str,
) -> serde_json::Value {
    serde_json::json!({
        "type": "article",
        "id": id,
        "title": title,
        "description": description,
        "input_message_content": {"message_text": message_text}
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

fn customer_confirmation_keyboard() -> serde_json::Value {
    serde_json::json!({
        "inline_keyboard": [
            [{"text": "✅ Tanlash", "callback_data": "order:customer_confirm"}],
            [{"text": "👤 Mijoz", "callback_data": "order:customer_reselect"}],
            [{"text": "❌ Bekor qilish", "callback_data": "order:cancel"}]
        ]
    })
}

fn product_confirmation_keyboard() -> serde_json::Value {
    serde_json::json!({
        "inline_keyboard": [
            [{"text": "✅ Tanlash", "callback_data": "order:product_confirm"}],
            [{"text": "📦 Mahsulot", "callback_data": "order:product_reselect"}],
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

fn print_method_keyboard() -> serde_json::Value {
    serde_json::json!({"inline_keyboard": [
        [{"text":"🖨 Flexo", "callback_data":"order:print:flexo"},
         {"text":"⚙️ Temir", "callback_data":"order:print:metal"}],
        [{"text":"❌ Bekor qilish", "callback_data":"order:cancel"}]
    ]})
}

fn cold_glue_keyboard() -> serde_json::Value {
    serde_json::json!({"inline_keyboard": [
        [{"text":"Ha", "callback_data":"order:cold:yes"},
         {"text":"Yo‘q", "callback_data":"order:cold:no"}],
        [{"text":"❌ Bekor qilish", "callback_data":"order:cancel"}]
    ]})
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

fn side_keyboard() -> serde_json::Value {
    serde_json::json!({"inline_keyboard": [
        [
            {"text": "1", "callback_data": "order:side:1"},
            {"text": "2", "callback_data": "order:side:2"},
            {"text": "3", "callback_data": "order:side:3"},
            {"text": "4", "callback_data": "order:side:4"}
        ],
        [{"text": "❌ Bekor qilish", "callback_data": "order:cancel"}]
    ]})
}

fn order_review_keyboard() -> serde_json::Value {
    serde_json::json!({
        "inline_keyboard": [
            [{"text": "✅ Tasdiqlash va yuborish", "callback_data": "order:confirm"}],
            [{"text": "✏️ Tahrirlash", "callback_data": "order:edit"}],
            [{"text": "❌ Bekor qilish", "callback_data": "order:cancel"}]
        ]
    })
}

fn order_edit_keyboard() -> serde_json::Value {
    serde_json::json!({
        "inline_keyboard": [
            [
                {"text": "📌 Buyurtma asoslari", "callback_data": "order:edit:basics"},
                {"text": "📐 O‘lchamlar", "callback_data": "order:edit:dimensions"}
            ],
            [
                {"text": "🧱 Material qatlamlari", "callback_data": "order:edit:layers"},
                {"text": "🖨 Bosma parametrlari", "callback_data": "order:edit:print"}
            ],
            [
                {"text": "🖼 Order rasmi", "callback_data": "order:edit:image"},
                {"text": "🔄 Taraf", "callback_data": "order:edit:side"}
            ],
            [{"text": "↩️ Tekshiruvga qaytish", "callback_data": "order:review"}]
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

fn parse_frame_count(value: &str) -> Option<f64> {
    parse_tiraj(value).filter(|n| n.fract() == 0.0 && *n <= i32::MAX as f64)
}

fn parse_edge_allowance(value: &str) -> Option<f64> {
    let allowance = value.trim().replace(',', ".").parse::<f64>().ok()?;
    (allowance.is_finite() && allowance >= 0.0).then_some(allowance)
}

fn parse_micron(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let micron = value.parse::<u32>().ok()?;
    (micron > 0).then(|| micron.to_string())
}

fn parse_diameter(value: &str) -> Option<f64> {
    let diameter = value.trim().replace(',', ".").parse::<f64>().ok()?;
    (diameter.is_finite() && diameter > 0.0).then_some(diameter)
}

fn parse_roll_count(value: &str) -> Option<i64> {
    let roll_count = value.trim().parse::<i64>().ok()?;
    (roll_count > 0).then_some(roll_count)
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
