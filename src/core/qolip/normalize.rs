use crate::core::auth::models::{Principal, PrincipalRole};

use super::models::{
    QolipCellQr, QolipCellQrInput, QolipError, QolipLocation, QolipLocationUpsert,
    QolipProductSpec, QolipProductSpecUpsert,
};

pub(super) fn normalize_cell_qr(
    input: QolipCellQrInput,
    principal: &Principal,
) -> Result<QolipCellQr, QolipError> {
    let block = input.block.trim().to_string();
    if block.is_empty() {
        return Err(QolipError::MissingBlock);
    }
    let row_letter = normalize_row_letter(&input.row_letter)?.ok_or(QolipError::InvalidLocation)?;
    let column_number = normalize_column_number(input.column_number, Some(&row_letter))?
        .ok_or(QolipError::InvalidLocation)?;
    let warehouse = input.warehouse.trim().to_string();
    let location_label = format!("{row_letter}{column_number}");
    let id = qolip_cell_id(&warehouse, &block, &row_letter, column_number);
    let qr_payload = qolip_cell_qr_payload(&id);
    Ok(QolipCellQr {
        id,
        block,
        warehouse,
        row_letter,
        column_number,
        location_label,
        qr_payload,
        created_by_role: role_code(&principal.role).to_string(),
        created_by_ref: principal.ref_.trim().to_string(),
        created_by_name: principal.display_name.trim().to_string(),
    })
}

pub(super) fn normalize_location(
    input: QolipLocationUpsert,
    principal: &Principal,
) -> Result<QolipLocation, QolipError> {
    let block = input.block.trim().to_string();
    if block.is_empty() {
        return Err(QolipError::MissingBlock);
    }
    let qolip_code = input.qolip_code.trim().to_string();
    if qolip_code.is_empty() {
        return Err(QolipError::MissingQolipCode);
    }
    let item_code = match input.item_code.trim() {
        "" => qolip_code.clone(),
        value => value.to_string(),
    };
    let item_name = match input.item_name.trim() {
        "" => qolip_code.clone(),
        value => value.to_string(),
    };
    if input.size <= 0 {
        return Err(QolipError::InvalidSize);
    }
    if input.quantity <= 0 {
        return Err(QolipError::InvalidQuantity);
    }
    let row_letter = normalize_row_letter(&input.row_letter)?;
    let column_number = normalize_column_number(input.column_number, row_letter.as_deref())?;
    let location_label = match (row_letter.as_deref(), column_number) {
        (Some(row), Some(column)) => format!("{row}{column}"),
        _ => String::new(),
    };
    let role = role_code(&principal.role).to_string();
    let warehouse = input.warehouse.trim().to_string();
    let id = qolip_location_id(
        &block,
        &item_code,
        &qolip_code,
        input.size,
        row_letter.as_deref().unwrap_or(""),
        column_number,
    );
    Ok(QolipLocation {
        id,
        block,
        warehouse,
        item_code,
        item_name,
        qolip_code,
        size: input.size,
        quantity: input.quantity,
        row_letter: row_letter.unwrap_or_default(),
        column_number,
        location_label,
        created_by_role: role,
        created_by_ref: principal.ref_.trim().to_string(),
        created_by_name: principal.display_name.trim().to_string(),
    })
}

pub(super) fn normalize_product_spec(
    input: QolipProductSpecUpsert,
    principal: &Principal,
) -> Result<QolipProductSpec, QolipError> {
    let item_code = input.item_code.trim().to_string();
    let item_name = input.item_name.trim().to_string();
    let qolip_code = input.qolip_code.trim().to_string();
    if item_code.is_empty() || item_name.is_empty() {
        return Err(QolipError::MissingItem);
    }
    if qolip_code.is_empty() {
        return Err(QolipError::MissingQolipCode);
    }
    if input.size <= 0 {
        return Err(QolipError::InvalidSize);
    }
    Ok(QolipProductSpec {
        item_code,
        item_name,
        item_group: input.item_group.trim().to_string(),
        qolip_code,
        size: input.size,
        created_by_role: role_code(&principal.role).to_string(),
        created_by_ref: principal.ref_.trim().to_string(),
        created_by_name: principal.display_name.trim().to_string(),
    })
}

pub fn role_code(role: &PrincipalRole) -> &'static str {
    match role {
        PrincipalRole::Supplier => "supplier",
        PrincipalRole::Werka => "werka",
        PrincipalRole::Customer => "customer",
        PrincipalRole::Aparatchi => "aparatchi",
        PrincipalRole::Qolipchi => "qolipchi",
        PrincipalRole::Admin => "admin",
    }
}

fn normalize_row_letter(value: &str) -> Result<Option<String>, QolipError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let mut chars = trimmed.chars();
    let Some(ch) = chars.next() else {
        return Ok(None);
    };
    if chars.next().is_some() || !ch.is_ascii_alphabetic() {
        return Err(QolipError::InvalidLocation);
    }
    Ok(Some(ch.to_ascii_uppercase().to_string()))
}

fn normalize_column_number(
    value: Option<i32>,
    row_letter: Option<&str>,
) -> Result<Option<i32>, QolipError> {
    match (row_letter, value) {
        (None, None) => Ok(None),
        (Some(_), Some(column)) if (1..=9).contains(&column) => Ok(Some(column)),
        _ => Err(QolipError::InvalidLocation),
    }
}

fn qolip_location_id(
    block: &str,
    item_code: &str,
    qolip_code: &str,
    size: i32,
    row_letter: &str,
    column_number: Option<i32>,
) -> String {
    format!(
        "qolip:{}:{}:{}:{}:{}:{}",
        compact_key(block),
        compact_key(item_code),
        compact_key(qolip_code),
        size,
        compact_key(row_letter),
        column_number.unwrap_or_default()
    )
}

fn qolip_cell_id(warehouse: &str, block: &str, row_letter: &str, column_number: i32) -> String {
    format!(
        "qolip-cell:{}:{}:{}:{}",
        compact_key(warehouse),
        compact_key(block),
        compact_key(row_letter),
        column_number
    )
}

fn qolip_cell_qr_payload(cell_id: &str) -> String {
    let hash = fnv1a64(cell_id);
    let checksum = (hash & 0xffff) as u16;
    format!("4002{hash:016X}{checksum:04X}")
}

fn fnv1a64(value: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in value.trim().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn compact_key(value: &str) -> String {
    let mut key = value
        .trim()
        .to_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>();
    while key.contains("__") {
        key = key.replace("__", "_");
    }
    key.trim_matches('_').to_string()
}

#[cfg(test)]
mod tests {
    use crate::core::auth::models::{Principal, PrincipalRole};

    use super::super::models::{QolipError, QolipLocationUpsert};
    use super::normalize_location;

    fn principal() -> Principal {
        Principal {
            role: PrincipalRole::Qolipchi,
            display_name: "Ali".to_string(),
            legal_name: "Ali".to_string(),
            ref_: "worker-1".to_string(),
            phone: "+998901234567".to_string(),
            avatar_url: String::new(),
        }
    }

    #[test]
    fn normalize_location_requires_numeric_size_and_column_range() {
        let base = QolipLocationUpsert {
            block: "A".to_string(),
            item_code: "VELONA".to_string(),
            item_name: "Velona".to_string(),
            qolip_code: "Q-1".to_string(),
            size: 12,
            quantity: 9,
            row_letter: "a".to_string(),
            column_number: Some(1),
            ..QolipLocationUpsert::default()
        };
        let normalized = normalize_location(base.clone(), &principal()).expect("valid location");
        assert_eq!(normalized.row_letter, "A");
        assert_eq!(normalized.location_label, "A1");

        let invalid = QolipLocationUpsert {
            column_number: Some(10),
            ..base
        };
        assert_eq!(
            normalize_location(invalid, &principal()),
            Err(QolipError::InvalidLocation)
        );
    }
}
