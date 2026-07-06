use crate::core::auth::access_codes::{SupplierAccessInput, supplier_access_code};
use crate::core::auth::ports::{
    AdminAccessState, CustomerRecord, MaterialTaminotchiRecord, SupplierRecord, WorkerRecord,
};

use super::AuthError;

pub fn normalize_phone(input: &str) -> Result<String, AuthError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(AuthError::InvalidCredentials);
    }

    let mut digits = String::new();
    for ch in trimmed.chars() {
        if ch == '+' {
            continue;
        }
        if !ch.is_ascii_digit() {
            return Err(AuthError::InvalidCredentials);
        }
        digits.push(ch);
    }

    if !trimmed.starts_with('+') && digits.len() == 9 {
        digits = format!("998{digits}");
    }

    if digits.len() < 9 || digits.len() > 12 {
        return Err(AuthError::InvalidCredentials);
    }

    Ok(format!("+{digits}"))
}

pub(super) fn normalize_config_phone(phone: &str) -> Result<String, AuthError> {
    let mut clean = phone.replace([' ', '-', '(', ')'], "");

    if !clean.trim().starts_with('+') && clean.len() == 9 {
        clean = format!("998{clean}");
    }

    normalize_phone(&clean)
}

pub(super) fn blank_default(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

pub(super) fn phone_matches_normalized(stored_phone: &str, normalized_login_phone: &str) -> bool {
    normalize_phone(stored_phone)
        .map(|phone| phone.eq_ignore_ascii_case(normalized_login_phone))
        .unwrap_or_else(|_| {
            stored_phone
                .trim()
                .eq_ignore_ascii_case(normalized_login_phone)
        })
}

pub(super) fn merge_customer_records(
    customers: &mut Vec<CustomerRecord>,
    extra: Vec<CustomerRecord>,
) {
    for record in extra {
        if customers
            .iter()
            .any(|existing| existing.id.trim() == record.id.trim())
        {
            continue;
        }
        customers.push(record);
    }
}

pub(super) fn merge_material_taminotchi_records(
    materials: &mut Vec<MaterialTaminotchiRecord>,
    extra: Vec<MaterialTaminotchiRecord>,
) {
    for record in extra {
        if materials
            .iter()
            .any(|existing| existing.id.trim() == record.id.trim())
        {
            continue;
        }
        materials.push(record);
    }
}

pub(super) fn merge_worker_records(workers: &mut Vec<WorkerRecord>, extra: Vec<WorkerRecord>) {
    for record in extra {
        if workers
            .iter()
            .any(|existing| existing.id.trim() == record.id.trim())
        {
            continue;
        }
        workers.push(record);
    }
}

pub(super) fn local_phone_query(normalized_phone: &str) -> Option<String> {
    let digits = normalized_phone.trim().strip_prefix('+')?;
    (digits.len() == 12 && digits.starts_with("998")).then(|| digits[3..].to_string())
}

pub(super) fn supplier_access_code_for(
    supplier: &SupplierRecord,
    state: &AdminAccessState,
) -> Result<String, AuthError> {
    let custom = state.custom_code.trim();
    if !custom.is_empty() {
        return Ok(custom.to_string());
    }

    supplier_access_code(&SupplierAccessInput {
        ref_: supplier.id.clone(),
        name: supplier.name.clone(),
        phone: supplier.phone.clone(),
    })
}
