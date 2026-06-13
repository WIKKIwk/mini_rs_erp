use sqlx::MySqlPool;

use crate::core::werka::models::StockEntryBarcodeEntry;

pub(crate) async fn read_stock_entries_by_barcode(
    pool: &MySqlPool,
    barcode: &str,
    limit: usize,
) -> Result<Vec<StockEntryBarcodeEntry>, sqlx::Error> {
    let limit = clamp_limit(limit, 20, 50);
    let normalized = barcode.trim().to_uppercase();
    let rows = sqlx::query_as::<_, StockEntryBarcodeRow>(STOCK_ENTRY_BARCODE_SQL)
        .bind(normalized)
        .bind(limit as i64)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(stock_entry_barcode_entry).collect())
}

#[derive(Debug, sqlx::FromRow)]
struct StockEntryBarcodeRow {
    stock_entry_name: String,
    stock_entry_type: String,
    doc_status: i32,
    status: String,
    company: String,
    posting_date: String,
    posting_time: String,
    creation: String,
    modified: String,
    remarks: String,
    line_index: i32,
    item_code: String,
    item_name: String,
    qty: f64,
    uom: String,
    stock_uom: String,
    barcode: String,
    source_warehouse: String,
    target_warehouse: String,
}

fn stock_entry_barcode_entry(row: StockEntryBarcodeRow) -> StockEntryBarcodeEntry {
    StockEntryBarcodeEntry {
        stock_entry_name: row.stock_entry_name.trim().to_string(),
        stock_entry_type: row.stock_entry_type.trim().to_string(),
        doc_status: row.doc_status,
        status: row.status.trim().to_string(),
        company: row.company.trim().to_string(),
        posting_date: row.posting_date.trim().to_string(),
        posting_time: row.posting_time.trim().to_string(),
        creation: row.creation.trim().to_string(),
        modified: row.modified.trim().to_string(),
        remarks: row.remarks.trim().to_string(),
        line_index: row.line_index,
        item_code: row.item_code.trim().to_string(),
        item_name: row.item_name.trim().to_string(),
        qty: row.qty,
        uom: row.uom.trim().to_string(),
        stock_uom: row.stock_uom.trim().to_string(),
        barcode: row.barcode.trim().to_string(),
        source_warehouse: row.source_warehouse.trim().to_string(),
        target_warehouse: row.target_warehouse.trim().to_string(),
    }
}

fn clamp_limit(value: usize, fallback: usize, max: usize) -> usize {
    match value {
        0 => fallback,
        value if value > max => max,
        value => value,
    }
}

const STOCK_ENTRY_BARCODE_SQL: &str = r#"
    SELECT
        se.name AS stock_entry_name,
        COALESCE(se.stock_entry_type, '') AS stock_entry_type,
        COALESCE(se.docstatus, 0) AS doc_status,
        CASE COALESCE(se.docstatus, 0)
            WHEN 0 THEN 'Draft'
            WHEN 1 THEN 'Submitted'
            WHEN 2 THEN 'Cancelled'
            ELSE ''
        END AS status,
        COALESCE(se.company, '') AS company,
        COALESCE(CAST(se.posting_date AS CHAR), '') AS posting_date,
        COALESCE(CAST(se.posting_time AS CHAR), '') AS posting_time,
        COALESCE(CAST(se.creation AS CHAR), '') AS creation,
        COALESCE(CAST(se.modified AS CHAR), '') AS modified,
        COALESCE(se.remarks, '') AS remarks,
        COALESCE(sed.idx, 0) AS line_index,
        COALESCE(sed.item_code, '') AS item_code,
        COALESCE(NULLIF(i.item_name, ''), sed.item_code, '') AS item_name,
        CAST(COALESCE(sed.qty, 0) AS DOUBLE) AS qty,
        COALESCE(NULLIF(sed.uom, ''), NULLIF(sed.stock_uom, ''), '') AS uom,
        COALESCE(NULLIF(sed.stock_uom, ''), NULLIF(sed.uom, ''), '') AS stock_uom,
        COALESCE(sed.barcode, '') AS barcode,
        COALESCE(NULLIF(sed.s_warehouse, ''), NULLIF(se.from_warehouse, ''), '') AS source_warehouse,
        COALESCE(NULLIF(sed.t_warehouse, ''), NULLIF(se.to_warehouse, ''), '') AS target_warehouse
    FROM `tabStock Entry Detail` sed
    INNER JOIN `tabStock Entry` se ON se.name = sed.parent
    LEFT JOIN tabItem i ON i.name = sed.item_code
    WHERE sed.barcode = ?
    ORDER BY se.modified DESC, se.creation DESC, se.name DESC, sed.idx ASC
    LIMIT ?
"#;

#[cfg(test)]
mod tests {
    use super::clamp_limit;

    #[test]
    fn stock_entry_limit_matches_go_bounds() {
        assert_eq!(clamp_limit(0, 20, 50), 20);
        assert_eq!(clamp_limit(75, 20, 50), 50);
        assert_eq!(clamp_limit(7, 20, 50), 7);
    }
}
