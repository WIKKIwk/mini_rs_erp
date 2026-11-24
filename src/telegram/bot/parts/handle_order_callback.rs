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
