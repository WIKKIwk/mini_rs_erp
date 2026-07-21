use async_trait::async_trait;
use sqlx::PgPool;

use crate::core::auth::models::Principal;
use crate::core::qolip::{
    QolipBlock, QolipCellQr, QolipCheckout, QolipError, QolipLocation, QolipProduct,
    QolipProductSpec, QolipStorePort,
};

mod catalog;
mod cell_qr;
mod checkouts;
mod locations;
mod rows;

use self::catalog::{
    delete_product_specs, load_all_blocks, load_assigned_blocks, load_assigned_warehouses,
    load_product_spec, load_product_spec_by_qolip_code, load_products,
    rename_block as rename_qolip_block, save_product_spec,
};
use self::cell_qr::{load_cell_qr_by_payload, save_cell_qr};
pub(crate) use self::checkouts::save_checkout_tx;
use self::checkouts::{
    load_checkout_by_id, load_checkouts, load_open_checkouts_for_worker,
    return_checkout_to_location, save_checkout,
};
use self::locations::{
    load_location_by_id, load_location_by_qolip_code, load_locations, move_location_to_cell,
    save_location,
};

#[derive(Clone)]
pub struct PostgresQolipStore {
    pool: PgPool,
}

impl PostgresQolipStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl QolipStorePort for PostgresQolipStore {
    async fn assigned_warehouses(&self, principal: &Principal) -> Result<Vec<String>, QolipError> {
        load_assigned_warehouses(&self.pool, principal).await
    }

    async fn assigned_blocks(&self, principal: &Principal) -> Result<Vec<QolipBlock>, QolipError> {
        load_assigned_blocks(&self.pool, principal).await
    }

    async fn all_blocks(&self) -> Result<Vec<QolipBlock>, QolipError> {
        load_all_blocks(&self.pool).await
    }

    async fn rename_block(
        &self,
        block: &str,
        new_block: &str,
        warehouse: &str,
    ) -> Result<QolipBlock, QolipError> {
        rename_qolip_block(&self.pool, block, new_block, warehouse).await
    }

    async fn products(
        &self,
        query: &str,
        limit: usize,
        with_qolip_only: bool,
    ) -> Result<Vec<QolipProduct>, QolipError> {
        load_products(&self.pool, query, limit, with_qolip_only).await
    }

    async fn product_spec(&self, item_code: &str) -> Result<Option<QolipProductSpec>, QolipError> {
        load_product_spec(&self.pool, item_code).await
    }

    async fn product_spec_by_qolip_code(
        &self,
        qolip_code: &str,
    ) -> Result<Option<QolipProductSpec>, QolipError> {
        load_product_spec_by_qolip_code(&self.pool, qolip_code).await
    }

    async fn put_product_spec(
        &self,
        spec: QolipProductSpec,
    ) -> Result<QolipProductSpec, QolipError> {
        save_product_spec(&self.pool, spec).await
    }

    async fn delete_product_specs(&self, qolip_codes: &[String]) -> Result<usize, QolipError> {
        delete_product_specs(&self.pool, qolip_codes).await
    }

    async fn locations(&self, block: &str) -> Result<Vec<QolipLocation>, QolipError> {
        load_locations(&self.pool, block).await
    }

    async fn put_location(&self, location: QolipLocation) -> Result<QolipLocation, QolipError> {
        save_location(&self.pool, location).await
    }

    async fn get_or_create_cell_qr(&self, cell: QolipCellQr) -> Result<QolipCellQr, QolipError> {
        save_cell_qr(&self.pool, cell).await
    }

    async fn location_by_id(&self, location_id: &str) -> Result<Option<QolipLocation>, QolipError> {
        load_location_by_id(&self.pool, location_id).await
    }

    async fn location_by_qolip_code(
        &self,
        qolip_code: &str,
    ) -> Result<Option<QolipLocation>, QolipError> {
        load_location_by_qolip_code(&self.pool, qolip_code).await
    }

    async fn issue_checkout(&self, checkout: QolipCheckout) -> Result<QolipCheckout, QolipError> {
        save_checkout(&self.pool, checkout).await
    }

    async fn checkouts(
        &self,
        block: Option<&str>,
        allowed_blocks: Option<&[String]>,
        status: &str,
        limit: usize,
    ) -> Result<Vec<QolipCheckout>, QolipError> {
        load_checkouts(&self.pool, block, allowed_blocks, status, limit).await
    }

    async fn open_checkouts_for_worker(
        &self,
        worker_refs: &[String],
        worker_name: &str,
        limit: usize,
    ) -> Result<Vec<QolipCheckout>, QolipError> {
        load_open_checkouts_for_worker(&self.pool, worker_refs, worker_name, limit).await
    }

    async fn checkout_by_id(&self, checkout_id: &str) -> Result<Option<QolipCheckout>, QolipError> {
        load_checkout_by_id(&self.pool, checkout_id).await
    }

    async fn return_checkout(
        &self,
        checkout_id: &str,
        row_letter: &str,
        column_number: Option<i32>,
    ) -> Result<QolipCheckout, QolipError> {
        return_checkout_to_location(&self.pool, checkout_id, row_letter, column_number).await
    }

    async fn move_location(
        &self,
        location_id: &str,
        row_letter: &str,
        column_number: i32,
        quantity: i32,
    ) -> Result<QolipLocation, QolipError> {
        move_location_to_cell(&self.pool, location_id, row_letter, column_number, quantity).await
    }

    async fn cell_qr_by_payload(
        &self,
        qr_payload: &str,
    ) -> Result<Option<QolipCellQr>, QolipError> {
        load_cell_qr_by_payload(&self.pool, qr_payload).await
    }
}
