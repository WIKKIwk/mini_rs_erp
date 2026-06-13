use super::*;

pub(super) fn customer_directory_entry(entry: AdminDirectoryEntry) -> CustomerDirectoryEntry {
    CustomerDirectoryEntry {
        ref_: entry.ref_,
        name: entry.name,
        phone: entry.phone,
    }
}

pub(super) fn dedupe_strings(values: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
            result.push(trimmed.to_string());
        }
    }
    result
}

pub(super) fn normalize_item_codes(values: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let key = trimmed.to_lowercase();
        if seen.insert(key) {
            result.push(trimmed.to_string());
        }
    }
    result
}

pub(super) fn normalize_admin_phone(phone: &str) -> Result<String, AdminPortError> {
    let mut clean = phone.replace([' ', '-', '(', ')'], "");
    if !clean.trim().starts_with('+') && clean.len() == 9 {
        clean = format!("998{clean}");
    }
    normalize_phone(&clean).map_err(|_| AdminPortError::LookupFailed)
}

pub(super) fn phone_matches(stored: &str, normalized: &str) -> bool {
    if normalize_admin_phone(stored)
        .map(|phone| phone.eq_ignore_ascii_case(normalized))
        .unwrap_or(false)
    {
        return true;
    }

    let stored_digits = stored.trim().trim_start_matches('+');
    let normalized_digits = normalized.trim().trim_start_matches('+');
    if stored_digits.eq_ignore_ascii_case(normalized_digits) {
        return true;
    }

    normalized_digits
        .strip_prefix("998")
        .map(|local| stored_digits.eq_ignore_ascii_case(local))
        .unwrap_or(false)
}

pub(super) fn phone_search_terms(raw: &str, normalized: &str) -> Vec<String> {
    let normalized_digits = normalized.trim().trim_start_matches('+');
    let local = normalized_digits
        .strip_prefix("998")
        .unwrap_or(normalized_digits);
    dedupe_strings(vec![
        normalized.to_string(),
        raw.trim().to_string(),
        normalized_digits.to_string(),
        local.to_string(),
    ])
}

pub(super) fn bump_code_regen_state(
    mut state: AdminState,
    now: OffsetDateTime,
) -> Result<AdminState, AdminPortError> {
    if state.code_locked(now) {
        return Err(AdminPortError::CodeRegenCooldown);
    }
    if state
        .regen_window_started_at
        .map(|started| now - started >= time::Duration::seconds(CODE_REGEN_WINDOW_SECONDS))
        .unwrap_or(true)
    {
        state.regen_window_started_at = Some(now);
        state.regen_window_count = 0;
        state.cooldown_until = None;
    }
    state.regen_window_count += 1;
    if state.regen_window_count >= MAX_CODE_REGENS_PER_WINDOW {
        state.cooldown_until = state
            .regen_window_started_at
            .map(|started| started + time::Duration::seconds(CODE_REGEN_WINDOW_SECONDS));
    }
    Ok(state)
}

pub(super) fn random_code(prefix: &str, existing: &mut BTreeMap<String, ()>) -> String {
    let prefix = if prefix.trim().is_empty() {
        "10"
    } else {
        prefix.trim()
    };
    loop {
        let suffix = (0..10)
            .map(|_| char::from(b'0' + rand::rng().random_range(0..10)))
            .collect::<String>();
        let code = format!("{prefix}{suffix}");
        if !existing.contains_key(&code) {
            existing.insert(code.clone(), ());
            return code;
        }
    }
}
