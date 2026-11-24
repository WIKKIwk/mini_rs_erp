fn derive_key(source: &str) -> [u8; 32] {
    let digest = Sha256::digest(source.as_bytes());
    let mut key = [0_u8; 32];
    key.copy_from_slice(&digest);
    key
}

fn load_or_create_key_file(path: &Path) -> Result<[u8; 32], TelegramStoreError> {
    // Nol manual qadam: fayl bo'lsa o'qiladi, bo'lmasa 32 bayt random
    // yaratilib 0600 bilan saqlanadi. Env override har doim ustun.
    if let Ok(raw) = std::fs::read_to_string(path)
        && let Some(key) = parse_key(&raw)
    {
        return Ok(key);
    }
    let key: [u8; 32] = rand::random();
    let hex: String = key.iter().map(|byte| format!("{byte:02x}")).collect();
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|_| TelegramStoreError::SessionCrypto)?;
    }
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            use std::io::Write;
            file.write_all(hex.as_bytes())
                .map_err(|_| TelegramStoreError::SessionCrypto)?;
            restrict_file_permissions_sync(path);
            tracing::info!(path = %path.display(), "telegram session key file created");
        }
        // Yonma-yon ikki process bir vaqtda yaratmoqchi bo'lsa: yutganining
        // kalitini o'qiymiz, o'zimiznikini ustiga yozmaymiz.
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let raw =
                std::fs::read_to_string(path).map_err(|_| TelegramStoreError::SessionCrypto)?;
            return parse_key(&raw).ok_or(TelegramStoreError::SessionCrypto);
        }
        Err(_) => return Err(TelegramStoreError::SessionCrypto),
    }
    Ok(key)
}

fn parse_key(raw: &str) -> Option<[u8; 32]> {
    let raw = raw.trim();
    if raw.len() != 64 {
        return None;
    }
    let mut key = [0_u8; 32];
    for (index, chunk) in raw.as_bytes().chunks(2).enumerate() {
        let text = std::str::from_utf8(chunk).ok()?;
        key[index] = u8::from_str_radix(text, 16).ok()?;
    }
    Some(key)
}

fn legacy_session_keys(data: &TelegramStoreData) -> Vec<[u8; 32]> {
    // Read-only compat for sessions encrypted before TELEGRAM_USER_SESSION_KEY
    // existed (key was API_HASH, then bot_token). Used only to migrate old
    // sessions forward; never used for new encryption.
    let mut sources = Vec::new();
    if let Ok(api_hash) = std::env::var("TELEGRAM_API_HASH")
        && !api_hash.trim().is_empty()
    {
        sources.push(api_hash);
    }
    if !data.bot_token.trim().is_empty() {
        sources.push(data.bot_token.clone());
    }
    sources
        .into_iter()
        .map(|source| {
            let digest = Sha256::digest(source.as_bytes());
            let mut key = [0_u8; 32];
            key.copy_from_slice(&digest);
            key
        })
        .collect()
}

async fn restrict_file_permissions(path: &Path) {
    // The store holds the bot token (plaintext) and encrypted MTProto
    // sessions: keep it owner-only. Best-effort; failures only warn.
    restrict_file_permissions_sync(path);
}

fn restrict_file_permissions_sync(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::Permissions::from_mode(0o600);
        if let Err(error) = std::fs::set_permissions(path, permissions) {
            tracing::warn!(%error, path = %path.display(), "telegram store chmod 0600 failed");
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

fn encrypt_session(session: &str, key: &[u8; 32]) -> Result<String, TelegramStoreError> {
    let iv = rand::random::<[u8; 16]>();
    let encrypted = SessionEncryptor::new_from_slices(key, &iv)
        .map_err(|_| TelegramStoreError::SessionCrypto)?
        .encrypt_padded_vec_mut::<Pkcs7>(session.as_bytes());
    let encoder = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    Ok(format!(
        "v1:{}:{}",
        encoder.encode(iv),
        encoder.encode(encrypted)
    ))
}

fn decrypt_session(value: &str, key: &[u8; 32]) -> Result<String, TelegramStoreError> {
    let mut parts = value.split(':');
    let (Some("v1"), Some(iv), Some(ciphertext), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(TelegramStoreError::SessionCrypto);
    };
    let decoder = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let iv = decoder
        .decode(iv)
        .map_err(|_| TelegramStoreError::SessionCrypto)?;
    let ciphertext = decoder
        .decode(ciphertext)
        .map_err(|_| TelegramStoreError::SessionCrypto)?;
    let decrypted = SessionDecryptor::new_from_slices(key, &iv)
        .map_err(|_| TelegramStoreError::SessionCrypto)?
        .decrypt_padded_vec_mut::<Pkcs7>(&ciphertext)
        .map_err(|_| TelegramStoreError::SessionCrypto)?;
    String::from_utf8(decrypted).map_err(|_| TelegramStoreError::SessionCrypto)
}

fn read_data(path: &Path) -> Result<TelegramStoreData, TelegramStoreError> {
    if !path.exists() {
        return Ok(TelegramStoreData::default());
    }
    let raw = std::fs::read(path).map_err(|_| TelegramStoreError::Read)?;
    if raw.is_empty() {
        return Ok(TelegramStoreData::default());
    }
    serde_json::from_slice(&raw).map_err(|_| TelegramStoreError::Read)
}
