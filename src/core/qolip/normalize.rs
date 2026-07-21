use crate::core::auth::models::{Principal, PrincipalRole};

use super::models::{
    QolipBlock, QolipCellQr, QolipCellQrInput, QolipCheckout, QolipError, QolipLocation,
    QolipLocationUpsert, QolipProductSpec, QolipProductSpecUpsert,
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
    if input.item_group.trim().is_empty() {
        return Err(QolipError::MissingItemGroup);
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
        PrincipalRole::Boyoqchi => "boyoqchi",
        PrincipalRole::MaterialTaminotchi => "material_taminotchi",
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
        (Some(_), Some(column)) if (1..=13).contains(&column) => Ok(Some(column)),
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

pub(super) fn normalize_checkout(
    location: QolipLocation,
    quantity: i32,
    worker_id: &str,
    worker_name: &str,
    principal: &Principal,
) -> Result<QolipCheckout, QolipError> {
    if quantity <= 0 {
        return Err(QolipError::InvalidQuantity);
    }
    if quantity > location.quantity {
        return Err(QolipError::InsufficientStock);
    }
    let worker_id = worker_id.trim();
    let worker_name = worker_name.trim();
    if worker_id.is_empty() {
        return Err(QolipError::MissingWorker);
    }
    if worker_name.is_empty() {
        return Err(QolipError::MissingWorker);
    }
    Ok(QolipCheckout {
        id: new_checkout_id(),
        location_id: location.id,
        block: location.block,
        warehouse: location.warehouse,
        item_code: location.item_code,
        item_name: location.item_name,
        item_group: String::new(),
        qolip_code: location.qolip_code,
        size: location.size,
        quantity,
        row_letter: location.row_letter,
        column_number: location.column_number,
        location_label: location.location_label,
        issued_to_ref: worker_id.to_string(),
        issued_to_name: worker_name.to_string(),
        status: "open".to_string(),
        issued_by_role: role_code(&principal.role).to_string(),
        issued_by_ref: principal.ref_.trim().to_string(),
        issued_by_name: principal.display_name.trim().to_string(),
        issued_at: String::new(),
    })
}

pub(crate) fn location_from_checkout(checkout: &QolipCheckout) -> QolipLocation {
    QolipLocation {
        id: qolip_location_id(
            &checkout.block,
            &checkout.item_code,
            &checkout.qolip_code,
            checkout.size,
            &checkout.row_letter,
            checkout.column_number,
        ),
        block: checkout.block.clone(),
        warehouse: checkout.warehouse.clone(),
        item_code: checkout.item_code.clone(),
        item_name: checkout.item_name.clone(),
        qolip_code: checkout.qolip_code.clone(),
        size: checkout.size,
        quantity: checkout.quantity,
        row_letter: checkout.row_letter.clone(),
        column_number: checkout.column_number,
        location_label: checkout.location_label.clone(),
        created_by_role: checkout.issued_by_role.clone(),
        created_by_ref: checkout.issued_by_ref.clone(),
        created_by_name: checkout.issued_by_name.clone(),
    }
}

pub(crate) fn location_from_checkout_target(
    checkout: &QolipCheckout,
    row_letter: &str,
    column_number: Option<i32>,
) -> Result<QolipLocation, QolipError> {
    let row_letter = row_letter.trim();
    if row_letter.is_empty() && column_number.is_none() {
        return Ok(location_from_checkout(checkout));
    }
    let row_letter = normalize_row_letter(row_letter)?.ok_or(QolipError::InvalidLocation)?;
    let column_number = normalize_column_number(column_number, Some(&row_letter))?
        .ok_or(QolipError::InvalidLocation)?;
    let mut location = location_from_checkout(checkout);
    location.row_letter = row_letter;
    location.column_number = Some(column_number);
    location.location_label = format!("{}{}", location.row_letter, column_number);
    location.id = qolip_location_id(
        &location.block,
        &location.item_code,
        &location.qolip_code,
        location.size,
        &location.row_letter,
        location.column_number,
    );
    Ok(location)
}

pub(crate) fn normalize_move_target(
    source: &QolipLocation,
    block: &str,
    warehouse: &str,
    row_letter: &str,
    column_number: i32,
    quantity: i32,
) -> Result<QolipLocation, QolipError> {
    if quantity <= 0 {
        return Err(QolipError::InvalidQuantity);
    }
    if quantity > source.quantity {
        return Err(QolipError::InsufficientStock);
    }
    let row_letter = normalize_row_letter(row_letter)?.ok_or(QolipError::InvalidLocation)?;
    let column_number = normalize_column_number(Some(column_number), Some(&row_letter))?
        .ok_or(QolipError::InvalidLocation)?;
    let block = if block.trim().is_empty() {
        source.block.trim()
    } else {
        block.trim()
    };
    let warehouse = if warehouse.trim().is_empty()
        && block.eq_ignore_ascii_case(source.block.trim())
    {
        source.warehouse.trim()
    } else {
        warehouse.trim()
    };
    if block.is_empty() || warehouse.is_empty() {
        return Err(QolipError::InvalidLocation);
    }
    let location_label = format!("{row_letter}{column_number}");
    let target_id = qolip_location_id(
        block,
        &source.item_code,
        &source.qolip_code,
        source.size,
        &row_letter,
        Some(column_number),
    );
    if target_id == source.id {
        return Err(QolipError::InvalidLocation);
    }
    Ok(QolipLocation {
        id: target_id,
        block: block.to_string(),
        warehouse: warehouse.to_string(),
        item_code: source.item_code.clone(),
        item_name: source.item_name.clone(),
        qolip_code: source.qolip_code.clone(),
        size: source.size,
        quantity,
        row_letter,
        column_number: Some(column_number),
        location_label,
        created_by_role: source.created_by_role.clone(),
        created_by_ref: source.created_by_ref.clone(),
        created_by_name: source.created_by_name.clone(),
    })
}

fn new_checkout_id() -> String {
    let bytes: [u8; 12] = rand::random();
    format!("qolip-checkout-{}", data_encoding::HEXLOWER.encode(&bytes))
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

pub(crate) fn location_identity_matches(
    existing: &QolipLocation,
    incoming: &QolipLocation,
) -> bool {
    existing
        .block
        .trim()
        .eq_ignore_ascii_case(incoming.block.trim())
        && existing
            .warehouse
            .trim()
            .eq_ignore_ascii_case(incoming.warehouse.trim())
        && existing
            .item_code
            .trim()
            .eq_ignore_ascii_case(incoming.item_code.trim())
        && existing
            .qolip_code
            .trim()
            .eq_ignore_ascii_case(incoming.qolip_code.trim())
        && existing.size == incoming.size
        && existing
            .row_letter
            .trim()
            .eq_ignore_ascii_case(incoming.row_letter.trim())
        && existing.column_number == incoming.column_number
}

pub(crate) fn resolve_cell_qr_from_payload(
    payload: &str,
    blocks: &[QolipBlock],
    principal: &Principal,
) -> Option<QolipCellQr> {
    let payload = payload.trim();
    if payload.is_empty() {
        return None;
    }
    for block in blocks {
        for row in 'A'..='Z' {
            for column in 1..=13 {
                let input = QolipCellQrInput {
                    block: block.name.clone(),
                    warehouse: block.warehouse.clone(),
                    row_letter: row.to_string(),
                    column_number: Some(column),
                };
                let Ok(cell) = normalize_cell_qr(input, principal) else {
                    continue;
                };
                if cell.qr_payload.eq_ignore_ascii_case(payload) {
                    return Some(cell);
                }
            }
        }
    }
    None
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

include!("normalize_inline_tests.rs");
