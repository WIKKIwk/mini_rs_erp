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
    store: Arc<dyn QolipStorePort>,
}

impl QolipService {
    pub async fn order_products(&self, item_codes: &[String]) -> Result<Vec<QolipProduct>, QolipError> {
        self.store.order_products(item_codes).await
    }
}

include!("service_impl_parts/part_01.rs");
include!("service_impl_parts/part_02.rs");

include!("service_matches.rs");

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
