use std::sync::Arc;

use tokio::sync::Mutex;

use crate::core::auth::models::{Principal, PrincipalRole};

use crate::core::gscale::models::{MaterialReceiptPrintRequest, MaterialReceiptPrintResponse};

use super::models::{
    RpsBatchPrintEntry, RpsBatchPrintRequest, RpsBatchResponse, RpsBatchSession,
    RpsBatchStartRequest,
};
use super::ports::{RpsBatchStoreError, RpsBatchStorePort};

#[derive(Clone)]
pub struct RpsBatchService {
    store: Arc<dyn RpsBatchStorePort>,
    mutation_lock: Arc<Mutex<()>>,
}

impl RpsBatchService {
    pub fn new(store: Arc<dyn RpsBatchStorePort>) -> Self {
        Self {
            store,
            mutation_lock: Arc::new(Mutex::new(())),
        }
    }

    pub async fn start(
        &self,
        principal: &Principal,
        request: RpsBatchStartRequest,
    ) -> Result<RpsBatchResponse, RpsBatchServiceError> {
        let _guard = self.mutation_lock.lock().await;
        let owner = BatchOwner::from_principal(principal);
        if self
            .store
            .get(&owner.key)
            .await?
            .is_some_and(|batch| batch.active)
        {
            return Err(RpsBatchServiceError::BatchAlreadyActive);
        }
        let now = now_string();
        let batch = normalize_start(owner, request, now)?;
        self.store.put(batch.clone()).await?;
        Ok(RpsBatchResponse::new(batch))
    }

    pub async fn state(
        &self,
        principal: &Principal,
    ) -> Result<RpsBatchResponse, RpsBatchServiceError> {
        let owner = BatchOwner::from_principal(principal);
        let batch = self
            .store
            .get(&owner.key)
            .await?
            .unwrap_or_else(|| owner.inactive_batch());
        Ok(RpsBatchResponse::new(batch))
    }

    pub async fn stop(
        &self,
        principal: &Principal,
    ) -> Result<RpsBatchResponse, RpsBatchServiceError> {
        let _guard = self.mutation_lock.lock().await;
        let owner = BatchOwner::from_principal(principal);
        let mut batch = self
            .store
            .get(&owner.key)
            .await?
            .unwrap_or_else(|| owner.inactive_batch());
        batch.active = false;
        batch.updated_at = now_string();
        self.store.put(batch.clone()).await?;
        Ok(RpsBatchResponse::new(batch))
    }

    pub async fn record_late_error(
        &self,
        principal: &Principal,
        batch_id: &str,
        detail: impl Into<String>,
    ) -> Result<bool, RpsBatchServiceError> {
        let _guard = self.mutation_lock.lock().await;
        let owner = BatchOwner::from_principal(principal);
        let Some(mut batch) = self.store.get(&owner.key).await? else {
            return Ok(false);
        };
        if batch.id != batch_id {
            return Ok(false);
        }
        batch.last_error = detail.into();
        batch.last_error_at = now_string();
        batch.updated_at = batch.last_error_at.clone();
        self.store.put(batch).await?;
        Ok(true)
    }

    pub async fn record_print(
        &self,
        principal: &Principal,
        batch_id: &str,
        response: &MaterialReceiptPrintResponse,
    ) -> Result<bool, RpsBatchServiceError> {
        let _guard = self.mutation_lock.lock().await;
        let owner = BatchOwner::from_principal(principal);
        let Some(mut batch) = self.store.get(&owner.key).await? else {
            return Ok(false);
        };
        if batch.id != batch_id {
            return Ok(false);
        }
        if !response.epc.trim().is_empty()
            && batch
                .prints
                .iter()
                .any(|entry| entry.epc == response.epc)
        {
            return Ok(true);
        }
        let printed_at = now_string();
        batch.prints.push(RpsBatchPrintEntry {
            epc: response.epc.clone(),
            draft_name: response.draft_name.clone(),
            status: response.status.clone(),
            qty: response.qty,
            net_qty: response.net_qty,
            gross_qty: response.gross_qty,
            unit: response.unit.clone(),
            printer: response.printer.clone(),
            print_mode: response.print_mode.clone(),
            print_count: response.print_count,
            printed_at: printed_at.clone(),
        });
        batch.updated_at = printed_at;
        self.store.put(batch).await?;
        Ok(true)
    }

    pub async fn material_receipt_request(
        &self,
        principal: &Principal,
        request: RpsBatchPrintRequest,
    ) -> Result<(String, MaterialReceiptPrintRequest), RpsBatchServiceError> {
        let owner = BatchOwner::from_principal(principal);
        let Some(batch) = self.store.get(&owner.key).await? else {
            return Err(RpsBatchServiceError::BatchNotActive);
        };
        if !batch.active {
            return Err(RpsBatchServiceError::BatchNotActive);
        }
        Ok((batch.id.clone(), batch.material_receipt_request(request)))
    }
}

#[derive(Debug, Clone)]
struct BatchOwner {
    key: String,
    role: String,
    ref_: String,
}

impl BatchOwner {
    fn from_principal(principal: &Principal) -> Self {
        let role = role_name(&principal.role).to_string();
        let ref_ = first_non_empty([&principal.ref_, &principal.phone, &principal.display_name]);
        Self {
            key: format!("{role}:{ref_}"),
            role,
            ref_,
        }
    }

    fn inactive_batch(&self) -> RpsBatchSession {
        RpsBatchSession::inactive(self.key.clone(), self.role.clone(), self.ref_.clone())
    }
}

fn normalize_start(
    owner: BatchOwner,
    request: RpsBatchStartRequest,
    now: String,
) -> Result<RpsBatchSession, RpsBatchServiceError> {
    let item_code = request.item_code.trim().to_string();
    let warehouse = request.warehouse.trim().to_string();
    if item_code.is_empty() || warehouse.is_empty() {
        return Err(RpsBatchServiceError::InvalidInput(
            "item_code_and_warehouse_required".to_string(),
        ));
    }
    let driver_url = request.driver_url.trim().trim_end_matches('/').to_string();
    if driver_url.is_empty() {
        return Err(RpsBatchServiceError::InvalidInput(
            "driver_url_required".to_string(),
        ));
    }

    Ok(RpsBatchSession {
        id: batch_id(&request.client_batch_id, &owner.key),
        active: true,
        owner_key: owner.key,
        owner_role: owner.role,
        owner_ref: owner.ref_,
        driver_url,
        item_name: fallback(&request.item_name, &item_code),
        item_code,
        warehouse,
        printer: fallback(&request.printer.to_ascii_lowercase(), "zebra"),
        print_mode: fallback(&request.print_mode.to_ascii_lowercase(), "rfid"),
        quantity_source: fallback(&request.quantity_source.to_ascii_lowercase(), "scale"),
        manual_qty_kg: positive_or_zero(request.manual_qty_kg),
        tare_enabled: request.tare_enabled || request.tare_kg > 0.0,
        tare_kg: positive_or_zero(request.tare_kg),
        last_error: String::new(),
        last_error_at: String::new(),
        prints: Vec::new(),
        created_at: now.clone(),
        updated_at: now,
    })
}

fn batch_id(client_batch_id: &str, owner_key: &str) -> String {
    let client_batch_id = client_batch_id.trim();
    if !client_batch_id.is_empty() {
        return client_batch_id.to_string();
    }
    let owner = owner_key.replace([':', ' ', '/'], "_");
    format!(
        "rps_batch_{}_{}",
        time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
        owner
    )
}

fn now_string() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

fn role_name(role: &PrincipalRole) -> &'static str {
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

fn first_non_empty(values: [&str; 3]) -> String {
    values
        .into_iter()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .unwrap_or("unknown")
        .to_string()
}

fn fallback(value: &str, default: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        default.to_string()
    } else {
        value.to_string()
    }
}

fn positive_or_zero(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        0.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RpsBatchServiceError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("batch not active")]
    BatchNotActive,
    #[error("batch already active")]
    BatchAlreadyActive,
    #[error("store failed")]
    StoreFailed,
}

impl From<RpsBatchStoreError> for RpsBatchServiceError {
    fn from(_: RpsBatchStoreError) -> Self {
        Self::StoreFailed
    }
}
