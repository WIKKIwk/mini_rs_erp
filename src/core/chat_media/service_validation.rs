fn ticket_hash(ticket: &str) -> Vec<u8> {
    Sha256::digest(ticket.trim().as_bytes()).to_vec()
}

fn validate_initialize_input(
    mut input: ChatMediaInitializeInput,
) -> Result<ChatMediaInitializeInput, ChatMediaError> {
    input.client_upload_id = validate_identifier(&input.client_upload_id)?.to_string();
    input.filename = input.filename.trim().to_string();
    input.content_type = normalize_content_type(&input.content_type);
    if input.filename.is_empty()
        || input.filename.chars().count() > 255
        || input.filename.chars().any(char::is_control)
        || input.size_bytes <= 0
    {
        return Err(ChatMediaError::InvalidInput);
    }
    let (maximum_size, allowed_types) = match input.kind {
        ChatMediaKind::Image => (
            MAX_CHAT_IMAGE_SIZE_BYTES,
            &["image/jpeg", "image/png", "image/webp"][..],
        ),
        ChatMediaKind::Video => (
            MAX_CHAT_VIDEO_SIZE_BYTES,
            &["video/mp4", "video/quicktime", "video/webm"][..],
        ),
        ChatMediaKind::Audio => (
            MAX_CHAT_AUDIO_SIZE_BYTES,
            &[
                "audio/mp4",
                "audio/x-m4a",
                "audio/aac",
                "audio/mpeg",
                "audio/ogg",
                "audio/webm",
                "audio/wav",
                "audio/x-wav",
            ][..],
        ),
    };
    if input.size_bytes > maximum_size {
        return Err(ChatMediaError::TooLarge);
    }
    if !allowed_types.contains(&input.content_type.as_str()) {
        return Err(ChatMediaError::InvalidInput);
    }
    match (input.kind, input.duration_ms) {
        (ChatMediaKind::Image, Some(_)) => return Err(ChatMediaError::InvalidInput),
        (ChatMediaKind::Video, Some(duration)) if duration > MAX_CHAT_VIDEO_DURATION_MS => {
            return Err(ChatMediaError::DurationTooLong);
        }
        (ChatMediaKind::Video, Some(duration)) if duration <= 0 => {
            return Err(ChatMediaError::InvalidInput);
        }
        (ChatMediaKind::Audio, Some(duration)) if duration > MAX_CHAT_AUDIO_DURATION_MS => {
            return Err(ChatMediaError::AudioDurationTooLong);
        }
        (ChatMediaKind::Audio, Some(duration)) if duration <= 0 => {
            return Err(ChatMediaError::InvalidInput);
        }
        (ChatMediaKind::Audio, None) => return Err(ChatMediaError::InvalidInput),
        _ => {}
    }
    Ok(input)
}

pub(super) fn validate_identifier(value: &str) -> Result<&str, ChatMediaError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(ChatMediaError::InvalidInput);
    }
    Ok(value)
}

fn normalize_content_type(value: &str) -> String {
    let value = value.trim();
    const CONTENT_TYPES: &[(&str, &str)] = &[
        ("image/jpg", "image/jpeg"),
        ("image/jpeg", "image/jpeg"),
        ("image/png", "image/png"),
        ("image/webp", "image/webp"),
        ("video/mp4", "video/mp4"),
        ("video/quicktime", "video/quicktime"),
        ("video/webm", "video/webm"),
        ("audio/mp4", "audio/mp4"),
        ("audio/x-m4a", "audio/x-m4a"),
        ("audio/aac", "audio/aac"),
        ("audio/mpeg", "audio/mpeg"),
        ("audio/ogg", "audio/ogg"),
        ("audio/webm", "audio/webm"),
        ("audio/wav", "audio/wav"),
        ("audio/x-wav", "audio/x-wav"),
    ];
    CONTENT_TYPES
        .iter()
        .find_map(|(alias, canonical)| {
            value
                .eq_ignore_ascii_case(alias)
                .then(|| (*canonical).to_string())
        })
        .unwrap_or_else(|| value.to_ascii_lowercase())
}

fn ensure_idempotent_input_matches(
    created: &ChatMediaCreateResult,
    input: &ChatMediaInitializeInput,
) -> Result<(), ChatMediaError> {
    let record = &created.record;
    if record.client_upload_id != input.client_upload_id
        || record.kind != input.kind
        || record.original_filename != input.filename
        || record.declared_content_type != input.content_type
        || record.declared_size_bytes != input.size_bytes
        || record.declared_duration_ms != input.duration_ms
    {
        return Err(ChatMediaError::Conflict);
    }
    Ok(())
}

pub(super) fn validate_stored_object(
    record: &ChatMediaUploadRecord,
    stored: &ChatMediaStorageObject,
) -> Result<(), ChatMediaError> {
    if stored.size_bytes != record.declared_size_bytes {
        return Err(ChatMediaError::InvalidInput);
    }
    let maximum = match record.kind {
        ChatMediaKind::Image => MAX_CHAT_IMAGE_SIZE_BYTES,
        ChatMediaKind::Video => MAX_CHAT_VIDEO_SIZE_BYTES,
        ChatMediaKind::Audio => MAX_CHAT_AUDIO_SIZE_BYTES,
    };
    if stored.size_bytes > maximum {
        return Err(ChatMediaError::TooLarge);
    }
    Ok(())
}
