async fn handle_order_text(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
    telegram_user_id: &str,
    text: &str,
) -> Result<bool, TelegramError> {
    let Some(mut draft) = service.order_draft(telegram_user_id).await? else {
        return Ok(false);
    };
    let value = text.trim();
    let catalog = service.order_catalog().await?;
    match draft.step {
        TelegramOrderStep::CustomerName => {
            if value.is_empty() {
                send_order_text(service, token, chat_id, "Mijoz ismini kiriting.").await?;
                return Ok(true);
            }
            // Bu yerda mijoz BAZAGA YARATILMAYDI: nom draftda saqlanadi,
            // yaratish rasm kelganda (zakaz yuborilayotganda) bo'ladi.
            // Bekor qilinsa hech qanday axlat qolmaydi.
            let existing = match catalog.find_customer_by_name(value).await {
                Ok(existing) => existing,
                Err(error) => return Err(TelegramError::OrderCatalog(error)),
            };
            let prefix = if let Some(customer) = existing {
                draft.customer_ref = customer.ref_.clone();
                draft.customer_name = customer.name.clone();
                format!(
                    "ℹ️ Bunday mijoz allaqachon bor: {}. Shu mijoz tanlandi.",
                    customer.name
                )
            } else {
                draft.customer_ref.clear();
                draft.customer_name = value.to_string();
                format!(
                    "✅ Yangi mijoz: {value}. U bazaga zakaz yuborilganda qo'shiladi. Mahsulotni «➕ Mahsulot qo'shish» orqali yozing."
                )
            };
            draft.step = TelegramOrderStep::Product;
            service.save_order_draft(telegram_user_id, draft).await?;
            send_or_edit_order_prompt(
                service,
                token,
                chat_id,
                &format!("{prefix}\n\n📦 Mahsulot nomini tanlang:"),
                Some(product_step_keyboard()),
            )
            .await?;
        }
        TelegramOrderStep::ProductName => {
            if value.is_empty() {
                send_order_text(service, token, chat_id, "Mahsulot nomini kiriting.").await?;
                return Ok(true);
            }
            // Mijoz singari mahsulot ham bu yerda yaratilmaydi: nom draftda
            // saqlanadi, yaratish zakaz yuborilayotganda bo'ladi.
            // Mijoz hali bazada bo'lmasa (yangi mijoz) qidiruv ishlamaydi.
            let prefix = if draft.customer_ref.trim().is_empty() {
                draft.product_code.clear();
                draft.product_name = value.to_string();
                format!("✅ Yangi mahsulot: {value}. U bazaga zakaz yuborilganda qo'shiladi.")
            } else {
                let item = match catalog
                    .find_customer_item_by_name(&draft.customer_ref, value)
                    .await
                {
                    Ok(item) => item,
                    Err(error) => return Err(TelegramError::OrderCatalog(error)),
                };
                if let Some(item) = item {
                    draft.product_code = item.code.clone();
                    draft.product_name = item.name.clone();
                    format!(
                        "ℹ️ Bu mahsulot allaqachon mavjud: {}. Shu mahsulot tanlandi.",
                        item.name
                    )
                } else {
                    draft.product_code.clear();
                    draft.product_name = value.to_string();
                    format!(
                        "✅ Yangi mahsulot: {value}. U bazaga zakaz yuborilganda qo'shiladi."
                    )
                }
            };
            let editing_basics =
                draft.edit_section == Some(TelegramOrderEditSection::Basics);
            if editing_basics {
                draft.edit_section = None;
                draft.step = TelegramOrderStep::Review;
                service
                    .save_order_draft(telegram_user_id, draft.clone())
                    .await?;
                send_order_review(service, token, chat_id).await?;
            } else {
                draft.step = TelegramOrderStep::Status;
                service.save_order_draft(telegram_user_id, draft).await?;
                send_or_edit_order_prompt(
                    service,
                    token,
                    chat_id,
                    &format!("{prefix}\n\nHolatni tanlang:"),
                    Some(status_keyboard()),
                )
                .await?;
            }
        }
        TelegramOrderStep::EdgeAllowance => {
            let Some(allowance) = parse_edge_allowance(value) else {
                send_order_text(
                    service, token, chat_id,
                    "Qo‘shimcha uzunlik 0 yoki undan katta raqam bo‘lishi kerak (mm).",
                ).await?;
                return Ok(true);
            };
            draft.edge_allowance_mm = Some(allowance);
            draft.step = TelegramOrderStep::ColdGlue;
            service.save_order_draft(telegram_user_id, draft).await?;
            send_or_edit_order_prompt(
                service,
                token,
                chat_id,
                "Holodniy kley bo‘ladimi?",
                Some(cold_glue_keyboard()),
            )
            .await?;
        }
        TelegramOrderStep::Micron => {
            let Some(micron) = parse_micron(value) else {
                send_order_text(
                    service,
                    token,
                    chat_id,
                    "Mikron faqat musbat butun son bo‘lishi kerak (masalan: 19).",
                )
                .await?;
                return Ok(true);
            };
            let Some(material) = catalog
                .material_by_id(&draft.pending_material_id)
                .await
                .map_err(TelegramError::OrderCatalog)?
            else {
                send_order_text(
                    service,
                    token,
                    chat_id,
                    "Material topilmadi. Qavatni qayta tanlang.",
                )
                .await?;
                return Ok(true);
            };
            draft.layers.push(TelegramOrderLayer {
                material_id: material.id,
                material: draft.pending_material_name.clone(),
                micron,
            });
            draft.pending_material_id.clear();
            draft.pending_material_name.clear();
            draft.step = TelegramOrderStep::LayerOptions;
            service
                .save_order_draft(telegram_user_id, draft.clone())
                .await?;
            send_layer_options(service, token, chat_id, &draft).await?;
        }
        TelegramOrderStep::Tiraj => {
            let Some(tiraj) = parse_tiraj(value) else {
                send_order_text(
                    service,
                    token,
                    chat_id,
                    "Iltimos, tirajni raqamda yuboring (kg).",
                )
                .await?;
                return Ok(true);
            };
            draft.tiraj_kg = Some(tiraj);
            draft.step = TelegramOrderStep::FrameSize;
            service.save_order_draft(telegram_user_id, draft).await?;
            send_order_text(service, token, chat_id, "1 ta kadrdagi mahsulot o‘lchamini mm da kiriting:").await?;
        }
        TelegramOrderStep::FrameSize => {
            let Some(size) = parse_tiraj(value) else {
                send_order_text(service, token, chat_id, "O‘lcham musbat raqam bo‘lishi kerak (mm).").await?;
                return Ok(true);
            };
            draft.frame_product_size_mm = Some(size);
            draft.step = TelegramOrderStep::FrameCount;
            service.save_order_draft(telegram_user_id, draft).await?;
            send_order_text(service, token, chat_id, "Kadr sonini kiriting (butun son):").await?;
        }
        TelegramOrderStep::FrameCount => {
            let Some(count) = parse_frame_count(value) else {
                send_order_text(service, token, chat_id, "Kadr soni musbat butun son bo‘lishi kerak.").await?;
                return Ok(true);
            };
            draft.frame_count = Some(count);
            draft.step = TelegramOrderStep::Diameter;
            service.save_order_draft(telegram_user_id, draft).await?;
            send_order_text(
                service,
                token,
                chat_id,
                "Diametrni mm da kiriting (masalan: 45.5):",
            )
            .await?;
        }
        TelegramOrderStep::Diameter => {
            let Some(diameter) = parse_diameter(value) else {
                send_order_text(
                    service,
                    token,
                    chat_id,
                    "Diametr musbat son bo‘lishi kerak (masalan: 45.5).",
                )
                .await?;
                return Ok(true);
            };
            draft.diameter_mm = Some(diameter);
            let editing_dimensions =
                draft.edit_section == Some(TelegramOrderEditSection::Dimensions);
            if editing_dimensions {
                draft.edit_section = None;
                draft.step = TelegramOrderStep::Review;
                service
                    .save_order_draft(telegram_user_id, draft.clone())
                    .await?;
                send_order_review(service, token, chat_id).await?;
            } else {
                draft.step = TelegramOrderStep::Material;
                service.save_order_draft(telegram_user_id, draft).await?;
                send_material_step(service, token, chat_id, 1).await?;
            }
        }
        TelegramOrderStep::ValCount => {
            let Some(roll_count) = parse_roll_count(value) else {
                send_order_text(
                    service,
                    token,
                    chat_id,
                    "Val/rang soni musbat butun son bo‘lishi kerak (masalan: 6).",
                )
                .await?;
                return Ok(true);
            };
            draft.roll_count = Some(roll_count);
            let order_number = if draft.order_number.trim().is_empty() {
                catalog
                    .next_order_number()
                    .await
                    .map_err(TelegramError::OrderCatalog)?
            } else {
                draft.order_number.clone()
            };
            draft.order_number = order_number.clone();
            let flexo = matches!(
                draft.print_method,
                Some(crate::core::production_map::automatic::PrintMethod::Flexo)
            );
            draft.step = if flexo {
                TelegramOrderStep::EdgeAllowance
            } else {
                TelegramOrderStep::ColdGlue
            };
            service.save_order_draft(telegram_user_id, draft).await?;
            if flexo {
                send_order_text(
                    service,
                    token,
                    chat_id,
                    "Flexo uchun edge allowance qiymatini mm da kiriting (0 mumkin):",
                )
                .await?;
            } else {
            send_or_edit_order_prompt(
                    service,
                    token,
                    chat_id,
                    "Holodniy kley bo‘ladimi?",
                    Some(cold_glue_keyboard()),
                )
                .await?;
            }
        }
        TelegramOrderStep::Attachment => {
            send_order_text(
                service,
                token,
                chat_id,
                "Orderni yuborish uchun rasm yoki rasm faylini yuboring.",
            )
            .await?;
        }
        TelegramOrderStep::Side => {
            send_side_step(service, token, chat_id).await?;
        }
        TelegramOrderStep::Review => {
            send_order_text(
                service,
                token,
                chat_id,
                "Orderni yuborish uchun «✅ Tasdiqlash va yuborish» tugmasini bosing yoki kerakli bo‘limni tahrirlang.",
            )
            .await?;
        }
        _ => {
            send_order_text(
                service,
                token,
                chat_id,
                "Tanlovni pastdagi tugmalar orqali davom ettiring.",
            )
            .await?;
        }
    }
    Ok(true)
}

async fn handle_inline_customer_selection(
    service: &TelegramService,
    token: &str,
    message: &TelegramMessage,
) -> Result<(), TelegramError> {
    let Some(user) = message.from.as_ref() else {
        return Ok(());
    };
    let telegram_user_id = user.id.to_string();
    let Some(mut draft) = service.order_draft(&telegram_user_id).await? else {
        return Ok(());
    };
    if draft.step != TelegramOrderStep::Customer {
        return Ok(());
    }
    let Some(customer_name) = message
        .text
        .as_deref()
        .and_then(|text| text.strip_prefix("Mijoz:").map(str::trim))
        .filter(|name| !name.is_empty())
    else {
        return Ok(());
    };
    let catalog = service.order_catalog().await?;
    let Some(customer) = catalog
        .find_customer_by_name(customer_name)
        .await
        .map_err(TelegramError::OrderCatalog)?
    else {
        return Ok(());
    };
    let previous_prompt_message_id = draft.prompt_message_id.take();
    draft.customer_ref = customer.ref_.clone();
    draft.customer_name = customer.name.clone();
    draft.step = TelegramOrderStep::CustomerConfirmation;
    draft.prompt_message_id = Some(message.message_id);
    draft.prompt_inline_message_id = None;
    service.save_order_draft(&telegram_user_id, draft).await?;
    if let Some(message_id) = previous_prompt_message_id
        && message_id != message.message_id
    {
        delete_message(
            service,
            token,
            &message.chat.id.to_string(),
            message_id,
        )
        .await
        .ok();
    }
    show_customer_confirmation(
        service,
        token,
        &message.chat.id.to_string(),
        &customer.name,
    )
    .await
}

async fn handle_inline_product_selection(
    service: &TelegramService,
    token: &str,
    message: &TelegramMessage,
) -> Result<(), TelegramError> {
    let Some(user) = message.from.as_ref() else {
        return Ok(());
    };
    let telegram_user_id = user.id.to_string();
    let Some(mut draft) = service.order_draft(&telegram_user_id).await? else {
        return Ok(());
    };
    if draft.step != TelegramOrderStep::Product {
        return Ok(());
    }
    let Some(product_name) = message
        .text
        .as_deref()
        .and_then(|text| text.strip_prefix("Mahsulot:").map(str::trim))
        .filter(|name| !name.is_empty())
    else {
        return Ok(());
    };
    let catalog = service.order_catalog().await?;
    let Some(item) = catalog
        .find_customer_item_by_name(&draft.customer_ref, product_name)
        .await
        .map_err(TelegramError::OrderCatalog)?
    else {
        return Ok(());
    };
    let previous_prompt_message_id = draft.prompt_message_id.take();
    draft.product_code = item.code;
    draft.product_name = item.name.clone();
    draft.step = TelegramOrderStep::ProductConfirmation;
    draft.prompt_message_id = Some(message.message_id);
    draft.prompt_inline_message_id = None;
    service.save_order_draft(&telegram_user_id, draft).await?;
    if let Some(message_id) = previous_prompt_message_id
        && message_id != message.message_id
    {
        delete_message(
            service,
            token,
            &message.chat.id.to_string(),
            message_id,
        )
        .await
        .ok();
    }
    show_product_confirmation(
        service,
        token,
        &message.chat.id.to_string(),
        &item.name,
    )
    .await
}

async fn handle_inline_material_selection(
    service: &TelegramService,
    token: &str,
    message: &TelegramMessage,
) -> Result<(), TelegramError> {
    let Some(user) = message.from.as_ref() else {
        return Ok(());
    };
    let telegram_user_id = user.id.to_string();
    let Some(mut draft) = service.order_draft(&telegram_user_id).await? else {
        return Ok(());
    };
    if draft.step != TelegramOrderStep::Material {
        return Ok(());
    }
    let Some(material_name) = message
        .text
        .as_deref()
        .and_then(|text| text.strip_prefix("Material:").map(str::trim))
        .filter(|name| !name.is_empty())
    else {
        return Ok(());
    };
    let catalog = service.order_catalog().await?;
    let material = catalog
        .search_materials(material_name, 50)
        .await
        .map_err(TelegramError::OrderCatalog)?
        .into_iter()
        .find(|item| normalize_order_text(&item.name) == normalize_order_text(material_name));
    let Some(material) = material else {
        return Ok(());
    };
    let previous_prompt_message_id = draft.prompt_message_id.take();
    draft.pending_material_id = material.id;
    draft.pending_material_name = material.name;
    draft.step = TelegramOrderStep::Micron;
    draft.prompt_message_id = Some(message.message_id);
    draft.prompt_inline_message_id = None;
    service.save_order_draft(&telegram_user_id, draft).await?;
    if let Some(message_id) = previous_prompt_message_id
        && message_id != message.message_id
    {
        delete_message(
            service,
            token,
            &message.chat.id.to_string(),
            message_id,
        )
        .await
        .ok();
    }
    send_micron_step(service, token, &message.chat.id.to_string()).await
}

async fn show_customer_confirmation(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
    customer_name: &str,
) -> Result<(), TelegramError> {
    let text = format!(
        "Mijoz: «{customer_name}»\n\n\
Tanlovni tasdiqlash uchun pastdagi «✅ Tanlash» tugmasini bosing.\n\
Boshqa mijoz tanlash uchun «👤 Mijoz» tugmasini yoki «❌ Bekor qilish» tugmasini bosing."
    );
    send_or_edit_order_prompt(
        service,
        token,
        chat_id,
        &text,
        Some(customer_confirmation_keyboard()),
    )
    .await
}

async fn show_product_confirmation(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
    product_name: &str,
) -> Result<(), TelegramError> {
    let text = format!(
        "Mahsulot: «{product_name}»\n\n\
Tanlovni tasdiqlash uchun pastdagi «✅ Tanlash» tugmasini bosing.\n\
Boshqa mahsulot tanlash uchun «📦 Mahsulot» tugmasini yoki «❌ Bekor qilish» tugmasini bosing."
    );
    send_or_edit_order_prompt(
        service,
        token,
        chat_id,
        &text,
        Some(product_confirmation_keyboard()),
    )
    .await
}

async fn handle_order_callback(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
    telegram_user_id: &str,
    data: &str,
    callback_message: Option<&TelegramMessage>,
    inline_message_id: Option<&str>,
) -> Result<(), TelegramError> {
    let Some(mut draft) = service.order_draft(telegram_user_id).await? else {
        send_order_text(
            service,
            token,
            chat_id,
            "Joriy order jarayoni topilmadi. /new_order yuboring.",
        )
        .await?;
        return Ok(());
    };
    let is_side_prompt = callback_message.is_some_and(|message| {
        draft.side_prompt_message_id == Some(message.message_id)
    });
    if data.starts_with("order:side:") && (draft.step != TelegramOrderStep::Side || !is_side_prompt) {
        return Ok(());
    }
    if let Some(inline_message_id) = inline_message_id {
        let previous_prompt_message_id = draft.prompt_message_id.take();
        draft.prompt_inline_message_id = Some(inline_message_id.to_string());
        service
            .save_order_draft(telegram_user_id, draft.clone())
            .await?;
        if let Some(message_id) = previous_prompt_message_id {
            delete_message(service, token, chat_id, message_id).await.ok();
        }
    } else if let Some(message) = callback_message.filter(|_| !is_side_prompt) {
        draft.prompt_inline_message_id = None;
        draft.prompt_message_id = Some(message.message_id);
        service
            .save_order_draft(telegram_user_id, draft.clone())
            .await?;
    }
    let payload = data.trim_start_matches("order:");
    let (action, value) = payload.split_once(':').unwrap_or((payload, ""));
    if action == "side" {
        if draft.select_side(value) {
            service.save_order_draft(telegram_user_id, draft).await?;
            send_order_review(service, token, chat_id).await?;
        }
        return Ok(());
    }
    if action == "cancel" {
        if draft.pending_order_saved {
            send_order_text(
                service,
                token,
                chat_id,
                "Order allaqachon saqlangan. Uni bekor qilib bo‘lmaydi; guruhni tanlab qayta tasdiqlang.",
            )
            .await?;
            return Ok(());
        }
        clear_side_prompt(service, token, chat_id).await?;
        send_order_text(
            service,
            token,
            chat_id,
            "Order ochish jarayoni bekor qilindi.",
        )
        .await?;
        service.clear_order_draft(telegram_user_id).await?;
        return Ok(());
    }
    if action == "confirm" {
        if draft.step == TelegramOrderStep::Side {
            send_side_step(service, token, chat_id).await?;
        } else if draft.step == TelegramOrderStep::Review {
            confirm_order(service, token, chat_id, telegram_user_id).await?;
        } else {
            send_order_text(
                service,
                token,
                chat_id,
                "Avval order ma’lumotlarini to‘liq kiriting va rasm yuboring.",
            )
            .await?;
        }
        return Ok(());
    }
    if action == "customer_confirm" {
        if draft.step != TelegramOrderStep::CustomerConfirmation {
            send_order_text(service, token, chat_id, "Avval mijozni tanlang.").await?;
            return Ok(());
        }
        let customer_name = draft.customer_name.clone();
        draft.step = TelegramOrderStep::Product;
        service.save_order_draft(telegram_user_id, draft).await?;
        send_product_step(service, token, chat_id, &customer_name, telegram_user_id).await?;
        return Ok(());
    }
    if action == "customer_reselect" {
        if draft.step != TelegramOrderStep::CustomerConfirmation {
            send_order_text(service, token, chat_id, "Avval mijozni tanlang.").await?;
            return Ok(());
        }
        draft.customer_ref.clear();
        draft.customer_name.clear();
        draft.step = TelegramOrderStep::Customer;
        service.save_order_draft(telegram_user_id, draft).await?;
        send_customer_step(service, token, chat_id, telegram_user_id).await?;
        return Ok(());
    }
    if action == "product_confirm" {
        if draft.step != TelegramOrderStep::ProductConfirmation {
            send_order_text(service, token, chat_id, "Avval mahsulotni tanlang.").await?;
            return Ok(());
        }
        let editing_basics = draft.edit_section == Some(TelegramOrderEditSection::Basics);
        if editing_basics {
            draft.edit_section = None;
            draft.step = TelegramOrderStep::Review;
            service
                .save_order_draft(telegram_user_id, draft.clone())
                .await?;
            send_order_review(service, token, chat_id).await?;
        } else {
            draft.step = TelegramOrderStep::Status;
            service.save_order_draft(telegram_user_id, draft).await?;
            send_status_step(service, token, chat_id).await?;
        }
        return Ok(());
    }
    if action == "product_reselect" {
        if draft.step != TelegramOrderStep::ProductConfirmation {
            send_order_text(service, token, chat_id, "Avval mahsulotni tanlang.").await?;
            return Ok(());
        }
        let customer_name = draft.customer_name.clone();
        draft.product_code.clear();
        draft.product_name.clear();
        draft.step = TelegramOrderStep::Product;
        service.save_order_draft(telegram_user_id, draft).await?;
        send_product_step(service, token, chat_id, &customer_name, telegram_user_id).await?;
        return Ok(());
    }
    if action == "review" {
        if draft.step == TelegramOrderStep::Review {
            send_order_review(service, token, chat_id).await?;
        }
        return Ok(());
    }
    if action == "edit" {
        if draft.step != TelegramOrderStep::Review {
            send_order_text(
                service,
                token,
                chat_id,
                "Tahrirlash faqat yakuniy tekshiruv bosqichida ishlaydi.",
            )
            .await?;
            return Ok(());
        }
        if draft.pending_order_saved {
            send_order_text(
                service,
                token,
                chat_id,
                "Order allaqachon saqlangan. Endi faqat guruhga qayta yuborish mumkin.",
            )
            .await?;
            return Ok(());
        }
        if value.is_empty() {
            send_order_edit_menu(service, token, chat_id).await?;
            return Ok(());
        }
        match value {
            "basics" => {
                draft.edit_section = Some(TelegramOrderEditSection::Basics);
                draft.customer_ref.clear();
                draft.customer_name.clear();
                draft.product_code.clear();
                draft.product_name.clear();
                draft.step = TelegramOrderStep::Customer;
                service.save_order_draft(telegram_user_id, draft).await?;
                send_customer_step(service, token, chat_id, telegram_user_id).await?;
            }
            "dimensions" => {
                draft.edit_section = Some(TelegramOrderEditSection::Dimensions);
                draft.tiraj_kg = None;
                draft.frame_product_size_mm = None;
                draft.frame_count = None;
                draft.diameter_mm = None;
                draft.step = TelegramOrderStep::Tiraj;
                service.save_order_draft(telegram_user_id, draft).await?;
                send_order_text(service, token, chat_id, "Tirajni kg da raqam bilan yuboring:").await?;
            }
            "layers" => {
                draft.edit_section = Some(TelegramOrderEditSection::Layers);
                draft.layers.clear();
                draft.pending_material_id.clear();
                draft.pending_material_name.clear();
                draft.step = TelegramOrderStep::Material;
                service.save_order_draft(telegram_user_id, draft).await?;
                send_material_step(service, token, chat_id, 1).await?;
            }
            "print" => {
                draft.edit_section = Some(TelegramOrderEditSection::Print);
                draft.print_method = None;
                draft.roll_count = None;
                draft.edge_allowance_mm = None;
                draft.cold_glue = None;
                draft.step = TelegramOrderStep::PrintMethod;
                service.save_order_draft(telegram_user_id, draft).await?;
                send_print_method_step(service, token, chat_id).await?;
            }
            "image" => {
                draft.edit_section = Some(TelegramOrderEditSection::Image);
                draft.step = TelegramOrderStep::Attachment;
                service.clear_order_attachment(telegram_user_id).await;
                service.save_order_draft(telegram_user_id, draft).await?;
                send_order_text(
                    service,
                    token,
                    chat_id,
                    "Yangi order rasmini photo yoki file ko‘rinishida yuboring.",
                )
                .await?;
            }
            "side" => {
                draft.request_side();
                draft.edit_section = Some(TelegramOrderEditSection::Side);
                service.save_order_draft(telegram_user_id, draft).await?;
                send_side_step(service, token, chat_id).await?;
            }
            _ => send_order_edit_menu(service, token, chat_id).await?,
        }
        return Ok(());
    }
    let catalog = service.order_catalog().await?;
    match action {
        "add_customer" if draft.step == TelegramOrderStep::Customer => {
            draft.step = TelegramOrderStep::CustomerName;
            service.save_order_draft(telegram_user_id, draft).await?;
            send_order_text(service, token, chat_id, "Mijoz ismini kiriting:").await?;
        }
        "customer" if draft.step == TelegramOrderStep::Customer => {
            let Some(customer_ref) = service.take_order_choice(telegram_user_id, value).await
            else {
                send_order_text(
                    service,
                    token,
                    chat_id,
                    "Mijoz tanlovi eskirgan. Qayta qidiring.",
                )
                .await?;
                return Ok(());
            };
            let customer = catalog
                .customer_by_ref(&customer_ref)
                .await
                .map_err(TelegramError::OrderCatalog)?;
            draft.customer_ref = customer.ref_.clone();
            draft.customer_name = customer.name.clone();
            draft.step = TelegramOrderStep::CustomerConfirmation;
            service.save_order_draft(telegram_user_id, draft).await?;
            show_customer_confirmation(
                service,
                token,
                chat_id,
                &customer.name,
            )
            .await?;
        }
        "add_product" if draft.step == TelegramOrderStep::Product => {
            draft.step = TelegramOrderStep::ProductName;
            service.save_order_draft(telegram_user_id, draft).await?;
            send_order_text(service, token, chat_id, "Mahsulot nomini kiriting:").await?;
        }
        "product" if draft.step == TelegramOrderStep::Product => {
            let Some(item_code) = service.take_order_choice(telegram_user_id, value).await else {
                send_order_text(
                    service,
                    token,
                    chat_id,
                    "Mahsulot tanlovi eskirgan. Qayta qidiring.",
                )
                .await?;
                return Ok(());
            };
            let Some(item) = catalog
                .customer_item_by_code(&draft.customer_ref, &item_code)
                .await
                .map_err(TelegramError::OrderCatalog)?
            else {
                send_order_text(
                    service,
                    token,
                    chat_id,
                    "Bu mahsulot tanlangan mijozga ulanmagan.",
                )
                .await?;
                return Ok(());
            };
            draft.product_code = item.code;
            draft.product_name = item.name.clone();
            draft.step = TelegramOrderStep::ProductConfirmation;
            service.save_order_draft(telegram_user_id, draft).await?;
            show_product_confirmation(
                service,
                token,
                chat_id,
                &item.name,
            )
            .await?;
        }
        "status" if draft.step == TelegramOrderStep::Status => {
            draft.status = match value {
                "roll" => "rulon".to_string(),
                "package" => "paket".to_string(),
                _ => return Ok(()),
            };
            draft.step = TelegramOrderStep::Tiraj;
            service.save_order_draft(telegram_user_id, draft).await?;
            send_order_text(
                service,
                token,
                chat_id,
                "Tirajni kg da raqam bilan yuboring:",
            )
            .await?;
        }
        "print" if draft.step == TelegramOrderStep::PrintMethod => {
            use crate::core::production_map::automatic::PrintMethod;
            let method = match value {
                "flexo" => PrintMethod::Flexo,
                "metal" => PrintMethod::Metal,
                _ => return Ok(()),
            };
            draft.print_method = Some(method);
            draft.edge_allowance_mm = None;
            draft.cold_glue = None;
            draft.step = TelegramOrderStep::ValCount;
            service.save_order_draft(telegram_user_id, draft).await?;
            send_order_text(
                service,
                token,
                chat_id,
                "Val/rang sonini kiriting (musbat butun son):",
            )
            .await?;
        }
        "material" if draft.step == TelegramOrderStep::Material => {
            let Some(material_id) = service.take_order_choice(telegram_user_id, value).await else {
                send_order_text(
                    service,
                    token,
                    chat_id,
                    "Material tanlovi eskirgan. Qayta qidiring.",
                )
                .await?;
                return Ok(());
            };
            let Some(material) = catalog
                .material_by_id(&material_id)
                .await
                .map_err(TelegramError::OrderCatalog)?
            else {
                send_order_text(
                    service,
                    token,
                    chat_id,
                    "Material topilmadi. Qayta tanlang.",
                )
                .await?;
                return Ok(());
            };
            draft.pending_material_id = material.id;
            draft.pending_material_name = material.name;
            draft.step = TelegramOrderStep::Micron;
            service.save_order_draft(telegram_user_id, draft).await?;
            send_micron_step(service, token, chat_id).await?;
        }
        "add_layer" if draft.step == TelegramOrderStep::LayerOptions => {
            draft.step = TelegramOrderStep::Material;
            let layer_number = draft.layers.len() + 1;
            service.save_order_draft(telegram_user_id, draft).await?;
            send_material_step(service, token, chat_id, layer_number).await?;
        }
        "next_layers" if draft.step == TelegramOrderStep::LayerOptions => {
            let editing_layers =
                draft.edit_section == Some(TelegramOrderEditSection::Layers);
            if editing_layers {
                draft.edit_section = None;
                draft.step = TelegramOrderStep::Review;
                service
                    .save_order_draft(telegram_user_id, draft.clone())
                    .await?;
                send_order_review(service, token, chat_id).await?;
            } else {
                draft.step = TelegramOrderStep::PrintMethod;
                service.save_order_draft(telegram_user_id, draft).await?;
                send_print_method_step(service, token, chat_id).await?;
            }
        }
        "cold" if draft.step == TelegramOrderStep::ColdGlue => {
            draft.cold_glue = Some(match value {
                "yes" => true,
                "no" => false,
                _ => return Ok(()),
            });
            let editing_print =
                draft.edit_section == Some(TelegramOrderEditSection::Print);
            if editing_print {
                draft.edit_section = None;
                draft.step = TelegramOrderStep::Review;
                service
                    .save_order_draft(telegram_user_id, draft.clone())
                    .await?;
                send_order_review(service, token, chat_id).await?;
            } else {
                draft.step = TelegramOrderStep::Attachment;
                service.save_order_draft(telegram_user_id, draft).await?;
                send_order_text(
                    service,
                    token,
                    chat_id,
                    "Endi order rasmini photo yoki file ko‘rinishida yuboring.",
                )
                .await?;
            }
        }
        _ => {}
    }
    Ok(())
}
