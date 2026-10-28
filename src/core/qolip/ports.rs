use async_trait::async_trait;

use crate::core::auth::models::Principal;

use super::models::{
    QolipBlock, QolipCellQr, QolipCheckout, QolipError, QolipLocation, QolipLocationMove,
    QolipProduct, QolipProductSpec, QolipProductTransfer,
};

#[async_trait]
pub trait QolipStorePort: Send + Sync {
    async fn qolip_storage_states(&self, _codes: &[String]) -> Result<std::collections::BTreeMap<String, String>, QolipError> {
        Ok(std::collections::BTreeMap::new())
    }
    async fn storage_read(&self, _kind: &str, _query: &super::QolipStorageQuery, _allowed: Option<&[String]>) -> Result<serde_json::Value, super::QolipStorageError> { Err(super::QolipStorageError::Store) }
    async fn storage_action(&self, _input: &super::QolipStorageAction, _archive: bool, _principal: &Principal, _allowed: Option<&[String]>) -> Result<serde_json::Value, super::QolipStorageError> { Err(super::QolipStorageError::Store) }

    async fn active_rework_codes(&self, _codes: &[String]) -> Result<Vec<String>, QolipError> { Ok(Vec::new()) }
    async fn reworks(&self, _status: &str, _allowed: Option<&[String]>) -> Result<Vec<super::QolipRework>, QolipError> { Err(QolipError::StoreFailed) }
    async fn send_reworks(&self, _input: &super::QolipReworkSend, _principal: &Principal, _allowed: Option<&[String]>) -> Result<Vec<super::QolipRework>, QolipError> { Err(QolipError::StoreFailed) }
    async fn return_reworks(&self, _input: &super::QolipReworkReturn, _principal: &Principal, _allowed: Option<&[String]>) -> Result<Vec<super::QolipRework>, QolipError> { Err(QolipError::StoreFailed) }
    async fn mileage(&self, _qolip_code: &str) -> Result<super::QolipMileage, QolipError> { Err(QolipError::StoreFailed) }

    async fn catalog_codes_for_molds(&self, codes: &[String]) -> Result<Vec<String>, QolipError> {
        let codes = codes.iter().map(|code| code.trim().to_lowercase()).collect::<std::collections::BTreeSet<_>>();
        Ok(self.products("", usize::MAX, true, None).await?.into_iter()
            .filter(|product| codes.contains(&product.qolip_code.trim().to_lowercase()))
            .map(|product| product.code).collect())
    }
    /// Distinct products, ordered by (lower(name), lower(code)). PostgreSQL
    /// implements a names-only query, without materializing mold details.
    async fn catalog_page(
        &self,
        query: &str,
        limit: usize,
        after: Option<&super::QolipCatalogPosition>,
        allowed_warehouses: Option<&[String]>,
    ) -> Result<Vec<super::QolipCatalogEntry>, QolipError> {
        let mut grouped = std::collections::BTreeMap::new();
        for product in self.products(query, usize::MAX, true, allowed_warehouses).await? {
            if product.qolip_code.trim().is_empty() { continue; }
            let key = product.code.trim().to_lowercase();
            grouped.entry(key).or_insert(super::QolipCatalogEntry {
                code: product.code,
                name: product.name,
            });
        }
        let mut entries = grouped.into_values().collect::<Vec<_>>();
        entries.sort_by_key(|entry| (entry.name.to_lowercase(), entry.code.to_lowercase()));
        if let Some(after) = after {
            entries.retain(|entry| (entry.name.to_lowercase(), entry.code.to_lowercase())
                > (after.name.clone(), after.code.clone()));
        }
        entries.truncate(limit);
        Ok(entries)
    }
    async fn catalog_detail(
        &self,
        item_code: &str,
        allowed_warehouses: Option<&[String]>,
    ) -> Result<Vec<QolipProduct>, QolipError> {
        Ok(self.products("", usize::MAX, true, allowed_warehouses).await?
            .into_iter().filter(|product| product.code.trim().eq_ignore_ascii_case(item_code.trim()))
            .collect())
    }
    async fn next_set_id(&self) -> Result<String, QolipError> {
        super::set_ids::next_memory_set_id()
    }
    async fn managed_sets(&self, _allowed: Option<&[String]>) -> Result<Vec<super::QolipManagedSet>, QolipError> { Err(QolipError::StoreFailed) }
    async fn rename_set(&self, _input: &super::QolipSetRename, _principal: &Principal, _allowed: Option<&[String]>) -> Result<super::QolipManagedSet, QolipError> { Err(QolipError::StoreFailed) }
    async fn create_set(&self, _input: &super::QolipSetCreate, _principal: &Principal, _allowed: &[String]) -> Result<super::QolipManagedSet, QolipError> { Err(QolipError::StoreFailed) }
    async fn edit_set(&self, _input: &super::QolipSetEdit, _principal: &Principal, _allowed: Option<&[String]>) -> Result<super::QolipManagedSet, QolipError> { Err(QolipError::StoreFailed) }
    async fn edit_product_molds(&self, _input: &super::QolipProductMoldEdits, _principal: &Principal, _allowed: Option<&[String]>) -> Result<(), QolipError> { Err(QolipError::StoreFailed) }
    async fn resolve_shared_set(&self, _identifier: &str) -> Result<Option<super::QolipSetSnapshot>, QolipError> { Ok(None) }
    async fn reuse_shared_set(&self, _set_id: &str, _item_code: &str, _fingerprint: &str, _principal: &Principal, _allowed: &[String]) -> Result<super::QolipSetSnapshot, QolipError> { Err(QolipError::StoreFailed) }
    async fn linked_product_specs(&self, _item_code: &str) -> Result<Vec<QolipProductSpec>, QolipError> { Ok(Vec::new()) }
    async fn transfer_product_specs(
        &self,
        _input: &QolipProductTransfer,
        _principal: &Principal,
        _allowed_warehouses: &[String],
    ) -> Result<Vec<QolipProductSpec>, QolipError> {
        Err(QolipError::StoreFailed)
    }
    /// Exact catalog identities and global QR readiness, without returning mold inventory.
    async fn order_products(&self, _item_codes: &[String]) -> Result<Vec<QolipProduct>, QolipError> {
        Err(QolipError::StoreFailed)
    }
    /// Bounded task eligibility only. PostgreSQL implements one batched EXISTS
    /// query; the fallback preserves the service's compatibility semantics.
    async fn order_product_readiness(
        &self,
        item_codes: &[String],
        warehouses: &[String],
    ) -> Result<std::collections::BTreeMap<String, bool>, QolipError> {
        let mut readiness = std::collections::BTreeMap::new();
        for product in self.order_products(item_codes).await? {
            let mut specs = self.product_specs(&product.code).await?
                .into_iter()
                .filter(|spec| spec.qolip_set_id.is_empty()
                    && super::service::qolip_spec_matches_order(spec, &product))
                .collect::<Vec<_>>();
            specs.extend(self.linked_product_specs(&product.code).await?);
            let ready = specs.iter().any(|spec| !spec.qolip_code.trim().is_empty()
                && !spec.warehouse.trim().is_empty()
                && warehouses.iter().any(|warehouse| warehouse.trim().eq_ignore_ascii_case(spec.warehouse.trim())));
            readiness.insert(product.code.trim().to_lowercase(), ready);
        }
        Ok(readiness)
    }
    /// Resolve a catalog identity by code, independently of mold rows and search limits.
    async fn product_by_code(&self, _item_code: &str) -> Result<Option<QolipProduct>, QolipError> {
        Err(QolipError::StoreFailed)
    }
    async fn assigned_warehouses(&self, principal: &Principal) -> Result<Vec<String>, QolipError>;
    async fn assigned_blocks(&self, principal: &Principal) -> Result<Vec<QolipBlock>, QolipError>;
    async fn all_blocks(&self) -> Result<Vec<QolipBlock>, QolipError>;
    async fn rename_block(
        &self,
        block: &str,
        new_block: &str,
        warehouse: &str,
    ) -> Result<QolipBlock, QolipError> {
        let _ = (block, new_block, warehouse);
        Err(QolipError::StoreFailed)
    }
    async fn products(
        &self,
        query: &str,
        limit: usize,
        with_qolip_only: bool,
        allowed_warehouses: Option<&[String]>,
    ) -> Result<Vec<QolipProduct>, QolipError>;
    async fn order_image_order_ids(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<(String, String)>, QolipError> {
        let _ = item_codes;
        Ok(Vec::new())
    }
    async fn product_spec(&self, item_code: &str) -> Result<Option<QolipProductSpec>, QolipError>;
    async fn product_specs(&self, item_code: &str) -> Result<Vec<QolipProductSpec>, QolipError> {
        Ok(self.product_spec(item_code).await?.into_iter().collect())
    }
    async fn product_spec_by_qolip_code(
        &self,
        qolip_code: &str,
    ) -> Result<Option<QolipProductSpec>, QolipError> {
        let _ = qolip_code;
        Ok(None)
    }
    /// Display the currently linked product without changing the physical mold's identity.
    async fn active_product_spec_by_qolip_code(
        &self,
        qolip_code: &str,
    ) -> Result<Option<QolipProductSpec>, QolipError> {
        self.product_spec_by_qolip_code(qolip_code).await
    }
    async fn put_product_spec(
        &self,
        spec: QolipProductSpec,
    ) -> Result<QolipProductSpec, QolipError>;
    async fn put_product_specs(
        &self,
        specs: Vec<QolipProductSpec>,
    ) -> Result<Vec<QolipProductSpec>, QolipError> {
        let mut saved = Vec::with_capacity(specs.len());
        for spec in specs {
            saved.push(self.put_product_spec(spec).await?);
        }
        Ok(saved)
    }
    async fn rename_product_spec(
        &self,
        previous_qolip_code: &str,
        spec: QolipProductSpec,
    ) -> Result<QolipProductSpec, QolipError> {
        let _ = (previous_qolip_code, spec);
        Err(QolipError::StoreFailed)
    }
    async fn delete_product_specs(&self, qolip_codes: &[String]) -> Result<usize, QolipError> {
        let _ = qolip_codes;
        Err(QolipError::StoreFailed)
    }
    async fn locations(&self, block: &str) -> Result<Vec<QolipLocation>, QolipError>;
    async fn location_by_id(&self, location_id: &str) -> Result<Option<QolipLocation>, QolipError>;
    async fn location_by_qolip_code(
        &self,
        qolip_code: &str,
    ) -> Result<Option<QolipLocation>, QolipError> {
        let _ = qolip_code;
        Ok(None)
    }
    async fn put_location(&self, location: QolipLocation) -> Result<QolipLocation, QolipError>;
    async fn get_or_create_cell_qr(&self, cell: QolipCellQr) -> Result<QolipCellQr, QolipError>;
    async fn issue_checkout(&self, checkout: QolipCheckout) -> Result<QolipCheckout, QolipError>;
    async fn open_checkout_by_qolip_code(
        &self,
        qolip_code: &str,
    ) -> Result<Option<QolipCheckout>, QolipError> {
        let qolip_code = qolip_code.trim();
        if qolip_code.is_empty() {
            return Ok(None);
        }
        Ok(self
            .checkouts(None, None, "open", 10_000)
            .await?
            .into_iter()
            .find(|checkout| checkout.qolip_code.trim().eq_ignore_ascii_case(qolip_code)))
    }
    async fn checkouts(
        &self,
        block: Option<&str>,
        allowed_blocks: Option<&[String]>,
        status: &str,
        limit: usize,
    ) -> Result<Vec<QolipCheckout>, QolipError>;
    async fn open_checkouts_for_worker(
        &self,
        worker_refs: &[String],
        _worker_name: &str,
        limit: usize,
    ) -> Result<Vec<QolipCheckout>, QolipError> {
        let refs = worker_refs
            .iter()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .collect::<std::collections::BTreeSet<_>>();
        let mut checkouts = self
            .checkouts(None, None, "open", 10_000)
            .await?
            .into_iter()
            .filter(|checkout| refs.contains(&checkout.issued_to_ref.trim().to_ascii_lowercase()))
            .collect::<Vec<_>>();
        checkouts.truncate(limit.max(1));
        Ok(checkouts)
    }
    async fn checkout_by_id(&self, checkout_id: &str) -> Result<Option<QolipCheckout>, QolipError>;
    async fn return_checkout(
        &self,
        checkout_id: &str,
        row_letter: &str,
        column_number: Option<i32>,
    ) -> Result<QolipCheckout, QolipError>;
    async fn move_location(
        &self,
        location_id: &str,
        block: &str,
        warehouse: &str,
        row_letter: &str,
        column_number: i32,
        quantity: i32,
    ) -> Result<QolipLocation, QolipError>;
    async fn move_locations(
        &self,
        moves: &[QolipLocationMove],
    ) -> Result<Vec<QolipLocation>, QolipError> {
        let mut saved = Vec::with_capacity(moves.len());
        for input in moves {
            let column_number = input.column_number.ok_or(QolipError::InvalidLocation)?;
            saved.push(
                self.move_location(
                    &input.location_id,
                    &input.block,
                    &input.warehouse,
                    &input.row_letter,
                    column_number,
                    input.quantity,
                )
                .await?,
            );
        }
        Ok(saved)
    }
    async fn cell_qr_by_payload(&self, qr_payload: &str)
    -> Result<Option<QolipCellQr>, QolipError>;
}
