use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions};
use sqlx::{MySqlPool, query_as};
use time::Date;

use crate::config::DirectDbConfig;
use crate::core::werka::models::{
    CustomerDirectoryEntry, CustomerItemOption, DispatchRecord, StockEntryBarcodeEntry,
    SupplierDirectoryEntry, SupplierItem, WerkaArchiveResponse, WerkaHomeData, WerkaHomeSummary,
    WerkaStatusBreakdownEntry,
};
use crate::erpdb::reader_pending_sql::*;
use crate::erpdb::reader_rows::{
    WerkaDispatchRecordPushdownRow, WerkaStatusBreakdownPushdownRow, WerkaSummaryPushdownRow,
};
use crate::erpdb::reader_sql::*;
use crate::erpdb::werka_archive::read_werka_archive;
use crate::erpdb::werka_customers::read_werka_customers;
use crate::erpdb::werka_history::{SupplierAckRow, build_werka_history};
use crate::erpdb::werka_home::{DeliveryNoteSummaryRow, PurchaseReceiptSummaryRow};
use crate::erpdb::werka_items::{
    read_werka_customer_item_options, read_werka_customer_items, read_werka_supplier_items,
};
use crate::erpdb::werka_status_details::build_werka_status_details;
use crate::erpdb::werka_suppliers::read_werka_suppliers;

#[derive(Clone)]
pub struct DirectDbReader {
    pub(crate) pool: MySqlPool,
    pub(crate) encryption_key: String,
    pub(crate) default_warehouse: String,
}

impl DirectDbReader {
    pub fn new(config: DirectDbConfig) -> Self {
        let options = MySqlConnectOptions::new()
            .host(&config.host)
            .port(config.port)
            .username(&config.user)
            .password(&config.password)
            .database(&config.name);
        let pool = MySqlPoolOptions::new()
            .min_connections(config.pool.min_connections)
            .max_connections(config.pool.max_connections)
            .acquire_timeout(config.pool.acquire_timeout)
            .idle_timeout(config.pool.idle_timeout)
            .connect_lazy_with(options);

        Self {
            pool,
            encryption_key: config.encryption_key.trim().to_string(),
            default_warehouse: config.default_warehouse.trim().to_string(),
        }
    }

    pub(crate) async fn home(&self, pending_limit: usize) -> Result<WerkaHomeData, sqlx::Error> {
        Ok(WerkaHomeData {
            summary: self.summary().await?,
            pending_items: self.pending(pending_limit).await?,
        })
    }

    pub(crate) async fn summary(&self) -> Result<WerkaHomeSummary, sqlx::Error> {
        let summary = query_as::<_, WerkaSummaryPushdownRow>(WERKA_SUMMARY_PUSHDOWN_SQL)
            .fetch_one(&self.pool)
            .await?;
        Ok(summary.into())
    }

    pub(crate) async fn pending(&self, limit: usize) -> Result<Vec<DispatchRecord>, sqlx::Error> {
        let limit = clamp_limit(limit, 1000);
        let rows = if limit > 0 {
            query_as::<_, WerkaDispatchRecordPushdownRow>(WERKA_PENDING_PUSHDOWN_LIMIT_SQL)
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?
        } else {
            query_as::<_, WerkaDispatchRecordPushdownRow>(WERKA_PENDING_PUSHDOWN_SQL)
                .fetch_all(&self.pool)
                .await?
        };
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub(crate) async fn history(&self) -> Result<Vec<DispatchRecord>, sqlx::Error> {
        const RECENT_LIMIT: usize = 120;
        let receipts = query_as::<_, PurchaseReceiptSummaryRow>(PURCHASE_RECEIPT_ROWS_LIMIT_SQL)
            .bind(RECENT_LIMIT as i64)
            .fetch_all(&self.pool);
        let acks = query_as::<_, SupplierAckRow>(SUPPLIER_ACK_ROWS_LIMIT_SQL)
            .bind(RECENT_LIMIT as i64)
            .fetch_all(&self.pool);
        let delivery_notes = query_as::<_, DeliveryNoteSummaryRow>(DELIVERY_NOTE_ROWS_LIMIT_SQL)
            .bind(RECENT_LIMIT as i64)
            .fetch_all(&self.pool);
        let (receipts, acks, delivery_notes) = tokio::try_join!(receipts, acks, delivery_notes)?;

        Ok(build_werka_history(
            &receipts,
            &acks,
            &delivery_notes,
            RECENT_LIMIT,
        ))
    }

    pub(crate) async fn status_breakdown(
        &self,
        kind: &str,
    ) -> Result<Vec<WerkaStatusBreakdownEntry>, sqlx::Error> {
        let kind = match kind.trim() {
            "pending" | "confirmed" | "returned" => kind.trim(),
            _ => return Ok(Vec::new()),
        };
        let rows =
            query_as::<_, WerkaStatusBreakdownPushdownRow>(WERKA_STATUS_BREAKDOWN_PUSHDOWN_SQL)
                .bind(kind)
                .bind(kind)
                .bind(kind)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub(crate) async fn status_details(
        &self,
        kind: &str,
        supplier_ref: &str,
    ) -> Result<Vec<DispatchRecord>, sqlx::Error> {
        let supplier_ref = supplier_ref.trim();
        let sql = match kind.trim() {
            "pending" => {
                let receipts = query_as::<_, PurchaseReceiptSummaryRow>(PURCHASE_RECEIPT_ROWS_SQL)
                    .fetch_all(&self.pool);
                let delivery_notes = query_as::<_, DeliveryNoteSummaryRow>(DELIVERY_NOTE_ROWS_SQL)
                    .fetch_all(&self.pool);
                let (receipts, delivery_notes) = tokio::try_join!(receipts, delivery_notes)?;
                return Ok(build_werka_status_details(
                    &receipts,
                    &delivery_notes,
                    kind,
                    supplier_ref,
                ));
            }
            "confirmed" => WERKA_STATUS_DETAILS_CONFIRMED_SQL,
            "returned" => WERKA_STATUS_DETAILS_RETURNED_SQL,
            _ => return Ok(Vec::new()),
        };
        let rows = query_as::<_, WerkaDispatchRecordPushdownRow>(sql)
            .bind(supplier_ref)
            .bind(supplier_ref)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub(crate) async fn archive(
        &self,
        kind: &str,
        period: &str,
        from: Option<Date>,
        to: Option<Date>,
    ) -> Result<WerkaArchiveResponse, sqlx::Error> {
        read_werka_archive(&self.pool, kind, period, from, to).await
    }

    pub(crate) async fn suppliers(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierDirectoryEntry>, sqlx::Error> {
        read_werka_suppliers(&self.pool, query, limit, offset).await
    }

    pub(crate) async fn customers(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<CustomerDirectoryEntry>, sqlx::Error> {
        read_werka_customers(&self.pool, query, limit, offset).await
    }

    pub(crate) async fn supplier_items(
        &self,
        supplier_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, sqlx::Error> {
        read_werka_supplier_items(
            &self.pool,
            &self.default_warehouse,
            supplier_ref,
            query,
            limit,
            offset,
        )
        .await
    }

    pub(crate) async fn customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, sqlx::Error> {
        read_werka_customer_items(
            &self.pool,
            &self.default_warehouse,
            customer_ref,
            query,
            limit,
            offset,
        )
        .await
    }

    pub(crate) async fn customer_item_options(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<CustomerItemOption>, sqlx::Error> {
        read_werka_customer_item_options(&self.pool, &self.default_warehouse, query, limit, offset)
            .await
    }

    pub(crate) async fn stock_entries_by_barcode(
        &self,
        barcode: &str,
        limit: usize,
    ) -> Result<Vec<StockEntryBarcodeEntry>, sqlx::Error> {
        crate::erpdb::stock_entry::read_stock_entries_by_barcode(&self.pool, barcode, limit).await
    }
}

fn clamp_limit(limit: usize, max: usize) -> usize {
    if limit > max { max } else { limit }
}
