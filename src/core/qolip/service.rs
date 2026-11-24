use std::collections::BTreeSet;
use std::sync::Arc;

use crate::core::auth::models::Principal;

use super::models::{
    QolipBlock, QolipCellQr, QolipCellQrInput, QolipCheckout, QolipCheckoutCreate,
    QolipCheckoutReturn, QolipError, QolipLocation, QolipLocationMove, QolipLocationUpsert,
    QolipOrderStartPreparation, QolipProduct, QolipProductSpec, QolipProductSpecUpsert,
};
use super::normalize::{
    normalize_cell_qr, normalize_checkout, normalize_location, normalize_move_target,
    normalize_product_spec, resolve_cell_qr_from_payload,
};
use super::ports::QolipStorePort;
use crate::core::text::trim_owned;

#[derive(Clone)]
pub struct QolipService {
    pub(super) store: Arc<dyn QolipStorePort>,
    storage_epoch: u64,
    storage_revision: tokio::sync::watch::Sender<u64>,
}

impl QolipService {
    pub(crate) fn storage_revision(&self) -> String {
        self.storage_token(*self.storage_revision.borrow())
    }

    fn storage_token(&self, revision: u64) -> String {
        format!("{:016x}.{revision}", self.storage_epoch)
    }

    pub(crate) fn notify_storage_changed(&self) {
        self.storage_revision.send_modify(|revision| *revision += 1);
    }

    pub(crate) async fn wait_for_storage_revision(&self, after: &str) -> String {
        let mut receiver = self.storage_revision.subscribe();
        let baseline = *receiver.borrow_and_update();
        if self.storage_token(baseline) == after {
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(12),
                receiver.changed(),
            )
            .await;
        }
        self.storage_token(*receiver.borrow())
    }

    pub(crate) async fn unavailable_molds(
        &self,
        codes: &[String],
    ) -> Result<std::collections::BTreeMap<String, String>, QolipError> {
        self.store.unavailable_molds(codes).await
    }

    pub(crate) async fn mark_unavailable_sets<'a>(
        &self,
        sets: impl IntoIterator<Item = &'a mut super::QolipSetSnapshot>,
    ) -> Result<(), QolipError> {
        let sets: Vec<_> = sets.into_iter().collect();
        let codes = sets
            .iter()
            .flat_map(|set| &set.members)
            .map(|member| member.qolip_code.clone())
            .collect::<Vec<_>>();
        let unavailable = self.unavailable_molds(&codes).await?;
        for set in sets {
            // Keep the canonical composition and fingerprint intact.
            set.unavailable_reason = set.members
                .iter()
                .filter_map(|member| unavailable.get(&member.qolip_code.trim().to_lowercase()))
                .min()
                .cloned()
                .unwrap_or_default();
        }
        Ok(())
    }

    pub(crate) async fn placement_products(
        &self,
        query: &str,
        limit: usize,
        warehouse: &str,
    ) -> Result<Vec<QolipProduct>, QolipError> {
        self.store
            .placement_products(query, limit.clamp(1, 20_000), warehouse)
            .await
    }

    pub(crate) async fn active_rework_codes(&self, codes: &[String]) -> Result<Vec<String>, QolipError> {
        self.store.active_rework_codes(codes).await
    }
    pub async fn order_product_readiness(
        &self, item_codes: &[String], warehouses: &[String],
    ) -> Result<std::collections::BTreeMap<String, bool>, QolipError> {
        self.store.order_product_readiness(item_codes, warehouses).await
    }
    pub async fn visible_order_products(
        &self, item_codes: &[String], principal: &Principal,
    ) -> Result<Vec<QolipProduct>, QolipError> {
        let allowed = self.warehouses_for_principal(principal, false).await?;
        let mut products = self.store.order_products(item_codes).await?;
        for product in &mut products {
            product.has_qolip_spec = self.compatible_specs_for_product(product).await?.iter().any(|spec| {
                !spec.qolip_code.trim().is_empty()
                    && !spec.warehouse.trim().is_empty() && allowed.iter().any(|warehouse| {
                    warehouse.trim().eq_ignore_ascii_case(spec.warehouse.trim())
                })
            });
        }
        Ok(products)
    }

    pub async fn order_products(&self, item_codes: &[String]) -> Result<Vec<QolipProduct>, QolipError> {
        self.store.order_products(item_codes).await
    }
}

include!("service_impl_parts/part_01.rs");
include!("service_impl_parts/part_02.rs");

include!("service_matches.rs");
include!("service_shared_sets.rs");
include!("service_management.rs");
include!("service_reworks.rs");
include!("service_mileage.rs");

impl QolipService {
    pub async fn transfer_product_specs(
        &self,
        mut input: super::models::QolipProductTransfer,
        principal: &Principal,
    ) -> Result<Vec<QolipProductSpec>, QolipError> {
        if principal.role != crate::core::auth::models::PrincipalRole::Qolipchi {
            return Err(QolipError::AccessDenied);
        }
        input.request_id = input.request_id.trim().to_string();
        input.from_item_code = input.from_item_code.trim().to_string();
        input.to_item_code = input.to_item_code.trim().to_string();
        if input.request_id.is_empty()
            || input.request_id.len() > 128
            || input.from_item_code.is_empty()
            || input.to_item_code.is_empty()
            || input
                .from_item_code
                .eq_ignore_ascii_case(&input.to_item_code)
            || input.qolip_codes.is_empty()
            || input.qolip_codes.len() > 100
            || input
                .qolip_codes
                .iter()
                .any(|code| code.trim().is_empty() || code.len() > 512)
        {
            return Err(QolipError::ProductTransferConflict);
        }
        input.qolip_codes = input
            .qolip_codes
            .into_iter()
            .map(|code| code.trim().to_lowercase())
            .collect();
        input.qolip_codes.sort();
        input.qolip_codes.dedup();
        let allowed = self.warehouses_for_principal(principal, false).await?;
        if allowed.is_empty() {
            return Err(QolipError::AccessDenied);
        }
        self.store
            .transfer_product_specs(&input, principal, &allowed)
            .await
    }
}
