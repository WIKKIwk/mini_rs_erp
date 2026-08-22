
fn push_retry_delay_seconds(attempts: i32) -> i64 {
    let exponent = attempts.clamp(1, 10) as u32;
    (1_i64 << exponent).clamp(2, 900)
}

fn message_preview(message: &super::ChatMessage) -> String {
    message_preview_text(&message.message_type, &message.body)
}

fn message_preview_text(message_type: &str, body: &str) -> String {
    let fallback = match message_type {
        "image" => "Rasm",
        "video" => "Video",
        "audio" => "Ovozli xabar",
        _ => "Xabar",
    };
    let body = body.trim();
    let mut chars = if body.is_empty() { fallback } else { body }.chars();
    let preview = chars.by_ref().take(120).collect::<String>();
    if chars.next().is_some() {
        format!("{preview}…")
    } else {
        preview
    }
}

fn ensure_chat_role(role: &PrincipalRole) -> Result<(), ChatError> {
    can_participate_in_chat(role)
        .then_some(())
        .ok_or(ChatError::Forbidden)
}

fn chat_role_from_code(value: &str) -> Result<PrincipalRole, ChatError> {
    match value.trim() {
        "supplier" => Ok(PrincipalRole::Supplier),
        "werka" => Ok(PrincipalRole::Werka),
        "aparatchi" => Ok(PrincipalRole::Aparatchi),
        "qolipchi" => Ok(PrincipalRole::Qolipchi),
        "boyoqchi" => Ok(PrincipalRole::Boyoqchi),
        "material_taminotchi" => Ok(PrincipalRole::MaterialTaminotchi),
        "customer" => Ok(PrincipalRole::Customer),
        "admin" => Ok(PrincipalRole::Admin),
        _ => Err(ChatError::InvalidInput),
    }
}

include!("../service_inline_tests.rs");
