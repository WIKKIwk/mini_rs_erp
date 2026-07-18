use crate::core::qolip::{QolipCellQr, QolipCheckout, QolipLocation, QolipProductSpec};

#[derive(sqlx::FromRow)]
pub(super) struct QolipBlockRow {
    pub(super) block: String,
    pub(super) warehouse: String,
}

#[derive(sqlx::FromRow)]
pub(super) struct QolipProductRow {
    pub(super) code: String,
    pub(super) name: String,
    pub(super) item_group: String,
    pub(super) qolip_code: String,
    pub(super) size: i32,
    pub(super) has_qolip_spec: bool,
    pub(super) is_in_use: bool,
}

#[derive(sqlx::FromRow)]
pub(super) struct QolipProductSpecRow {
    item_code: String,
    item_name: String,
    item_group: String,
    qolip_code: String,
    size: i32,
    created_by_role: String,
    created_by_ref: String,
    created_by_name: String,
}

#[derive(Clone, sqlx::FromRow)]
pub(super) struct QolipLocationRow {
    pub(super) id: String,
    pub(super) block: String,
    pub(super) warehouse: String,
    pub(super) item_code: String,
    pub(super) item_name: String,
    pub(super) qolip_code: String,
    pub(super) size: i32,
    pub(super) quantity: i32,
    pub(super) row_letter: String,
    pub(super) column_number: Option<i32>,
    pub(super) location_label: String,
    pub(super) created_by_role: String,
    pub(super) created_by_ref: String,
    pub(super) created_by_name: String,
}

#[derive(sqlx::FromRow)]
pub(super) struct QolipCellQrRow {
    id: String,
    block: String,
    warehouse: String,
    row_letter: String,
    column_number: i32,
    location_label: String,
    qr_payload: String,
    created_by_role: String,
    created_by_ref: String,
    created_by_name: String,
}

#[derive(sqlx::FromRow)]
pub(super) struct QolipCheckoutRow {
    id: String,
    location_id: String,
    block: String,
    warehouse: String,
    item_code: String,
    item_name: String,
    qolip_code: String,
    size: i32,
    quantity: i32,
    row_letter: String,
    column_number: Option<i32>,
    location_label: String,
    issued_to_ref: String,
    issued_to_name: String,
    status: String,
    issued_by_role: String,
    issued_by_ref: String,
    issued_by_name: String,
    issued_at: String,
}

pub(super) fn row_to_location(row: QolipLocationRow) -> QolipLocation {
    QolipLocation {
        id: row.id,
        block: row.block,
        warehouse: row.warehouse,
        item_code: row.item_code,
        item_name: row.item_name,
        qolip_code: row.qolip_code,
        size: row.size,
        quantity: row.quantity,
        row_letter: row.row_letter,
        column_number: row.column_number,
        location_label: row.location_label,
        created_by_role: row.created_by_role,
        created_by_ref: row.created_by_ref,
        created_by_name: row.created_by_name,
    }
}

pub(super) fn row_to_product_spec(row: QolipProductSpecRow) -> QolipProductSpec {
    QolipProductSpec {
        item_code: row.item_code,
        item_name: row.item_name,
        item_group: row.item_group,
        qolip_code: row.qolip_code,
        size: row.size,
        created_by_role: row.created_by_role,
        created_by_ref: row.created_by_ref,
        created_by_name: row.created_by_name,
    }
}

pub(super) fn row_to_checkout(row: QolipCheckoutRow) -> QolipCheckout {
    QolipCheckout {
        id: row.id,
        location_id: row.location_id,
        block: row.block,
        warehouse: row.warehouse,
        item_code: row.item_code,
        item_name: row.item_name,
        item_group: String::new(),
        qolip_code: row.qolip_code,
        size: row.size,
        quantity: row.quantity,
        row_letter: row.row_letter,
        column_number: row.column_number,
        location_label: row.location_label,
        issued_to_ref: row.issued_to_ref,
        issued_to_name: row.issued_to_name,
        status: row.status,
        issued_by_role: row.issued_by_role,
        issued_by_ref: row.issued_by_ref,
        issued_by_name: row.issued_by_name,
        issued_at: row.issued_at,
    }
}

pub(super) fn row_to_cell_qr(row: QolipCellQrRow) -> QolipCellQr {
    QolipCellQr {
        id: row.id,
        block: row.block,
        warehouse: row.warehouse,
        row_letter: row.row_letter,
        column_number: row.column_number,
        location_label: row.location_label,
        qr_payload: row.qr_payload,
        created_by_role: row.created_by_role,
        created_by_ref: row.created_by_ref,
        created_by_name: row.created_by_name,
    }
}
