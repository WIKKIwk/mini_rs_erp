use std::collections::BTreeMap;

use async_trait::async_trait;
use serde_json::Value;
use sqlx::PgPool;
use time::{Date, OffsetDateTime, UtcOffset};

use crate::core::production_map::PaddonReceipt;
use crate::core::werka::models::{
    ArchiveTotalByUom, DispatchRecord, WerkaArchiveResponse, WerkaArchiveSummary, WerkaHomeData,
    WerkaHomeSummary,
};
use crate::core::werka::ports::{WerkaHomeLookup, WerkaPortError};

#[derive(Clone)]
pub struct PostgresWerkaHomeLookup {
    pool: PgPool,
}

impl PostgresWerkaHomeLookup {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn receipts(&self) -> Result<Vec<PaddonReceipt>, WerkaPortError> {
        let rows = sqlx::query_scalar::<_, Option<Value>>(
            "SELECT receipt_json FROM mini_paddons WHERE receipt_json IS NOT NULL ORDER BY updated_at DESC, code ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|error| WerkaPortError::Database(error.to_string()))?;

        rows.into_iter()
            .flatten()
            .map(|value| {
                serde_json::from_value(value)
                    .map_err(|error| WerkaPortError::Database(error.to_string()))
            })
            .collect()
    }

    async fn paddon_records(
        &self,
        from: Option<Date>,
        to: Option<Date>,
    ) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        let receipts = self.receipts().await?;
        Ok(records_from_receipts(&receipts, from, to))
    }
}

#[async_trait]
impl WerkaHomeLookup for PostgresWerkaHomeLookup {
    async fn werka_summary(&self) -> Result<WerkaHomeSummary, WerkaPortError> {
        let records = self.paddon_records(None, None).await?;
        Ok(WerkaHomeSummary {
            pending_count: 0,
            confirmed_count: records.len() as i64,
            returned_count: 0,
        })
    }

    async fn werka_home(&self, _pending_limit: usize) -> Result<WerkaHomeData, WerkaPortError> {
        Ok(WerkaHomeData {
            summary: self.werka_summary().await?,
            pending_items: Vec::new(),
        })
    }

    async fn werka_history(&self) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        self.paddon_records(None, None).await
    }

    async fn werka_archive(
        &self,
        kind: &str,
        period: &str,
        from: Option<Date>,
        to: Option<Date>,
    ) -> Result<WerkaArchiveResponse, WerkaPortError> {
        let kind = if kind.trim().is_empty() {
            "received"
        } else {
            kind.trim()
        };
        let period = if period.trim().is_empty() {
            "yearly"
        } else {
            period.trim()
        };
        let items = if kind.eq_ignore_ascii_case("received") {
            self.paddon_records(from, to).await?
        } else {
            Vec::new()
        };
        let mut totals = BTreeMap::<String, f64>::new();
        for item in &items {
            let quantity = if item.accepted_qty > 0.0 {
                item.accepted_qty
            } else {
                item.sent_qty
            };
            *totals.entry(item.uom.clone()).or_default() += quantity;
        }

        Ok(WerkaArchiveResponse {
            kind: kind.to_ascii_lowercase(),
            period: period.to_ascii_lowercase(),
            from: from.map(|value| value.to_string()).unwrap_or_default(),
            to: to.map(|value| value.to_string()).unwrap_or_default(),
            summary: WerkaArchiveSummary {
                record_count: items.len(),
                totals_by_uom: totals
                    .into_iter()
                    .map(|(uom, qty)| ArchiveTotalByUom { uom, qty })
                    .collect(),
            },
            items,
        })
    }
}

fn records_from_receipts(
    receipts: &[PaddonReceipt],
    from: Option<Date>,
    to: Option<Date>,
) -> Vec<DispatchRecord> {
    let tashkent_offset = UtcOffset::from_hms(5, 0, 0).expect("valid Tashkent UTC offset");
    let mut records = receipts
        .iter()
        .flat_map(|receipt| {
            receipt.stocks.iter().filter_map(|stock| {
                let timestamp = if stock.accepted_at_unix > 0 {
                    stock.accepted_at_unix
                } else {
                    receipt.accepted_at_unix
                };
                let date = OffsetDateTime::from_unix_timestamp(timestamp)
                    .ok()
                    .map(|value| value.to_offset(tashkent_offset).date())?;
                if from.is_some_and(|value| date < value) || to.is_some_and(|value| date > value) {
                    return None;
                }
                let item_code = if stock.item_code.trim().is_empty() {
                    stock.id.clone()
                } else {
                    stock.item_code.trim().to_string()
                };
                let item_name = if stock.item_name.trim().is_empty() {
                    item_code.clone()
                } else {
                    stock.item_name.trim().to_string()
                };
                let warehouse = if stock.warehouse.trim().is_empty() {
                    receipt.warehouse.trim()
                } else {
                    stock.warehouse.trim()
                };
                let created_label = OffsetDateTime::from_unix_timestamp(timestamp)
                    .ok()
                    .map(|value| {
                        value
                            .to_offset(tashkent_offset)
                            .format(&time::format_description::well_known::Rfc3339)
                            .unwrap_or_default()
                    })
                    .unwrap_or_default();
                Some((
                    timestamp,
                    DispatchRecord {
                        id: format!("paddon:{}:{}", receipt.paddon.code, stock.id),
                        record_type: "paddon_receipt".to_string(),
                        supplier_ref: format!("paddon:{}", receipt.paddon.code),
                        supplier_name: format!("Paddon {} • {}", receipt.paddon.code, warehouse),
                        item_code,
                        item_name,
                        uom: stock.uom.trim().to_string(),
                        sent_qty: stock.qty,
                        accepted_qty: stock.qty,
                        amount: 0.0,
                        currency: String::new(),
                        note: format!("{} omboriga paddon qabul qilindi", warehouse),
                        event_type: "paddon_received".to_string(),
                        highlight: warehouse.to_string(),
                        status: "accepted".to_string(),
                        created_label,
                    },
                ))
            })
        })
        .collect::<Vec<_>>();
    records.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.id.cmp(&right.1.id))
    });
    records.into_iter().map(|(_, record)| record).collect()
}
