use crate::core::auth::models::Principal;

use super::models::{QolipBlock, QolipCellQr};
use super::normalize::{compact_key, role_code};

pub(super) fn qolip_cell_id(
    warehouse: &str,
    block: &str,
    row_letter: &str,
    column_number: i32,
) -> String {
    qolip_cell_id_from_compact(
        &compact_key(warehouse),
        &compact_key(block),
        &compact_key(row_letter),
        column_number,
    )
}

fn qolip_cell_id_from_compact(
    warehouse: &str,
    block: &str,
    row_letter: &str,
    column_number: i32,
) -> String {
    format!("qolip-cell:{warehouse}:{block}:{row_letter}:{column_number}")
}

pub(super) fn qolip_cell_qr_payload(cell_id: &str) -> String {
    let hash = fnv1a64(cell_id);
    let checksum = (hash & 0xffff) as u16;
    format!("4002{hash:016X}{checksum:04X}")
}

pub(crate) fn resolve_cell_qr_from_payload(
    payload: &str,
    blocks: &[QolipBlock],
    principal: &Principal,
) -> Option<QolipCellQr> {
    let expected_hash = parse_qolip_cell_qr_payload(payload)?;
    for block in blocks {
        let block_name = block.name.trim();
        let warehouse = block.warehouse.trim();
        let block_key = compact_key(block_name);
        let warehouse_key = compact_key(warehouse);
        for row in b'A'..=b'Z' {
            let row_key = [row.to_ascii_lowercase()];
            for column in 1..=13 {
                if qolip_cell_hash(&warehouse_key, &block_key, row_key[0], column) != expected_hash
                {
                    continue;
                }
                let row_letter = char::from(row).to_string();
                let location_label = format!("{row_letter}{column}");
                let id = qolip_cell_id_from_compact(
                    &warehouse_key,
                    &block_key,
                    std::str::from_utf8(&row_key).ok()?,
                    column,
                );
                return Some(QolipCellQr {
                    qr_payload: qolip_cell_qr_payload(&id),
                    id,
                    block: block_name.to_string(),
                    warehouse: warehouse.to_string(),
                    row_letter,
                    column_number: column,
                    location_label,
                    created_by_role: role_code(&principal.role).to_string(),
                    created_by_ref: principal.ref_.trim().to_string(),
                    created_by_name: principal.display_name.trim().to_string(),
                });
            }
        }
    }
    None
}

fn parse_qolip_cell_qr_payload(payload: &str) -> Option<u64> {
    let payload = payload.trim();
    if payload.len() != 24 || !payload.get(..4)?.eq_ignore_ascii_case("4002") {
        return None;
    }
    let hash = u64::from_str_radix(payload.get(4..20)?, 16).ok()?;
    let checksum = u16::from_str_radix(payload.get(20..24)?, 16).ok()?;
    (checksum == (hash & 0xffff) as u16).then_some(hash)
}

fn qolip_cell_hash(warehouse: &str, block: &str, row: u8, column: i32) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for part in [
        b"qolip-cell:".as_slice(),
        warehouse.as_bytes(),
        b":".as_slice(),
        block.as_bytes(),
        b":".as_slice(),
        std::slice::from_ref(&row),
        b":".as_slice(),
    ] {
        hash = fnv1a64_extend(hash, part);
    }
    let mut digits = [0_u8; 2];
    let length = if column >= 10 {
        digits[0] = b'0' + (column / 10) as u8;
        digits[1] = b'0' + (column % 10) as u8;
        2
    } else {
        digits[0] = b'0' + column as u8;
        1
    };
    fnv1a64_extend(hash, &digits[..length])
}

fn fnv1a64_extend(mut hash: u64, value: &[u8]) -> u64 {
    for byte in value {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn fnv1a64(value: &str) -> u64 {
    fnv1a64_extend(0xcbf2_9ce4_8422_2325_u64, value.trim().as_bytes())
}
