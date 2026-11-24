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

include!("handle_inline_material_selection.rs");

include!("handle_order_callback.rs");
