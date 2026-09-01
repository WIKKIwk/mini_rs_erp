use std::sync::Arc;

use tokio::sync::{Mutex, OwnedRwLockReadGuard, RwLock};

use crate::core::auth::models::{Principal, PrincipalRole};

use crate::core::gscale::models::{MaterialReceiptPrintRequest, MaterialReceiptPrintResponse};

use super::models::{
    RpsBatchHistoryResponse, RpsBatchPrintEntry, RpsBatchPrintRequest, RpsBatchResponse,
    RpsBatchSession, RpsBatchStartRequest, RpsBatchStopRequest, RpsBatchUpdateRequest,
    new_batch_code, trimmed_or,
};
use super::ports::{RpsBatchStoreError, RpsBatchStorePort};
use crate::core::text::trim_owned;

#[derive(Clone)]
pub struct RpsBatchService {
    store: Arc<dyn RpsBatchStorePort>,
    mutation_lock: Arc<Mutex<()>>,
    lifecycle_lock: Arc<RwLock<()>>,
}

impl RpsBatchService {
    pub fn new(store: Arc<dyn RpsBatchStorePort>) -> Self {
        Self {
            store,
            mutation_lock: Arc::new(Mutex::new(())),
            lifecycle_lock: Arc::new(RwLock::new(())),
        }
    }

    pub async fn start(
        &self,
        principal: &Principal,
        request: RpsBatchStartRequest,
    ) -> Result<RpsBatchResponse, RpsBatchServiceError> {
        let _lifecycle_guard = self.lifecycle_lock.write().await;
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
        self.store.put(&batch).await?;
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
            .unwrap_or_else(|| owner.into_inactive_batch());
        Ok(RpsBatchResponse::new(batch))
    }

    pub async fn update(
        &self,
        principal: &Principal,
        request: RpsBatchUpdateRequest,
    ) -> Result<RpsBatchResponse, RpsBatchServiceError> {
        let _lifecycle_guard = self.lifecycle_lock.write().await;
        let _guard = self.mutation_lock.lock().await;
        let owner = BatchOwner::from_principal(principal);
        let Some(mut batch) = self.store.get(&owner.key).await? else {
            return Err(RpsBatchServiceError::BatchNotActive);
        };
        batch.ensure_context();
        if !batch.active {
            return Err(RpsBatchServiceError::BatchNotActive);
        }
        validate_update_context(&batch, &request)?;
        apply_update(&mut batch, request)?;
        batch.revision = next_revision(batch.revision);
        batch.updated_at = now_string();
        self.store.put(&batch).await?;
        Ok(RpsBatchResponse::new(batch))
    }

    pub async fn stop(
        &self,
        principal: &Principal,
        request: RpsBatchStopRequest,
    ) -> Result<RpsBatchResponse, RpsBatchServiceError> {
        let _lifecycle_guard = self.lifecycle_lock.write().await;
        let _guard = self.mutation_lock.lock().await;
        let owner = BatchOwner::from_principal(principal);
        let Some(mut batch) = self.store.get(&owner.key).await? else {
            return Err(RpsBatchServiceError::BatchNotActive);
        };
        batch.ensure_context();
        validate_stop_context(&batch, &request)?;
        if !batch.active {
            return Ok(RpsBatchResponse::new(batch));
        }
        batch.active = false;
        batch.revision = next_revision(batch.revision);
        batch.updated_at = now_string();
        self.store.complete(&batch).await?;
        Ok(RpsBatchResponse::new(batch))
    }

    pub async fn history(
        &self,
        principal: &Principal,
        limit: usize,
    ) -> Result<RpsBatchHistoryResponse, RpsBatchServiceError> {
        let owner = BatchOwner::from_principal(principal);
        let batches = self
            .store
            .list_completed(&owner.key, limit.clamp(1, 100))
            .await?;
        Ok(RpsBatchHistoryResponse::new(batches))
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
        if batch.active {
            self.store.put(&batch).await?;
        } else {
            self.store.complete(&batch).await?;
        }
        Ok(true)
    }

    pub async fn fail_batch(
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
        batch.ensure_context();
        if batch.id != batch_id.trim() {
            return Ok(false);
        }
        let now = now_string();
        batch.last_error = detail.into();
        batch.last_error_at = now.clone();
        batch.updated_at = now;
        if batch.active {
            batch.active = false;
            batch.revision = next_revision(batch.revision);
        }
        self.store.complete(&batch).await?;
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
            && batch.prints.iter().any(|entry| entry.epc == response.epc)
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
        if batch.active {
            self.store.put(&batch).await?;
        } else {
            self.store.complete(&batch).await?;
        }
        Ok(true)
    }

    pub async fn material_receipt_request(
        &self,
        principal: &Principal,
        request: RpsBatchPrintRequest,
    ) -> Result<
        (
            String,
            MaterialReceiptPrintRequest,
            OwnedRwLockReadGuard<()>,
        ),
        RpsBatchServiceError,
    > {
        let lifecycle_guard = self.lifecycle_lock.clone().read_owned().await;
        let owner = BatchOwner::from_principal(principal);
        let Some(mut batch) = self.store.get(&owner.key).await? else {
            return Err(RpsBatchServiceError::BatchNotActive);
        };
        batch.ensure_context();
        if !batch.active {
            return Err(RpsBatchServiceError::BatchNotActive);
        }
        validate_print_context(&batch, &request)?;
        Ok((
            batch.id.clone(),
            batch.material_receipt_request(request),
            lifecycle_guard,
        ))
    }
}

#[derive(Debug)]
struct BatchOwner<'a> {
    key: String,
    role: &'static str,
    ref_: &'a str,
}

impl<'a> BatchOwner<'a> {
    fn from_principal(principal: &'a Principal) -> Self {
        let role = role_name(principal.role);
        let ref_ = first_non_empty([&principal.ref_, &principal.phone, &principal.display_name]);
        Self {
            key: format!("{role}:{ref_}"),
            role,
            ref_,
        }
    }

    fn into_inactive_batch(self) -> RpsBatchSession {
        RpsBatchSession::inactive(self.key, self.role.to_string(), self.ref_.to_string())
    }
}

fn normalize_start(
    owner: BatchOwner<'_>,
    request: RpsBatchStartRequest,
    now: String,
) -> Result<RpsBatchSession, RpsBatchServiceError> {
    let RpsBatchStartRequest {
        client_batch_id,
        driver_url,
        item_code,
        item_name,
        warehouse,
        printer,
        print_mode,
        quantity_source,
        manual_qty_kg,
        tare_enabled,
        tare_kg,
        width_mm,
        micron,
    } = request;
    let item_code = trim_owned(item_code);
    let warehouse = trim_owned(warehouse);
    if item_code.is_empty() || warehouse.is_empty() {
        return Err(RpsBatchServiceError::InvalidInput(
            "item_code_and_warehouse_required".to_string(),
        ));
    }
    let driver_url = trim_trailing_slashes(driver_url);
    if driver_url.is_empty() {
        return Err(RpsBatchServiceError::InvalidInput(
            "driver_url_required".to_string(),
        ));
    }
    let (width_mm, micron) = normalize_dimensions(width_mm, micron)?;
    let (tare_enabled, tare_kg) = normalize_tare(tare_enabled, tare_kg);

    Ok(RpsBatchSession {
        id: batch_id(client_batch_id, &owner.key),
        batch_code: new_batch_code(),
        revision: 1,
        active: true,
        owner_key: owner.key,
        owner_role: owner.role.to_string(),
        owner_ref: owner.ref_.to_string(),
        driver_url,
        item_name: trimmed_or(item_name, &item_code),
        item_code,
        warehouse,
        printer: lowercase_or(printer, "zebra"),
        print_mode: lowercase_or(print_mode, "rfid"),
        quantity_source: normalize_quantity_source(quantity_source),
        manual_qty_kg: positive_or_zero(manual_qty_kg),
        tare_enabled,
        tare_kg,
        width_mm,
        micron,
        last_error: String::new(),
        last_error_at: String::new(),
        prints: Vec::new(),
        created_at: now.clone(),
        updated_at: now,
    })
}

fn normalize_quantity_source(value: String) -> String {
    let mut value = trim_owned(value);
    if value.eq_ignore_ascii_case("manual") {
        value.make_ascii_lowercase();
        value
    } else {
        "scale".to_string()
    }
}

fn normalize_tare(tare_enabled: bool, tare_kg: f64) -> (bool, f64) {
    let tare_kg = positive_or_zero(tare_kg);
    (tare_enabled || tare_kg > 0.0, tare_kg)
}

fn normalize_dimensions(
    width_mm: Option<f64>,
    micron: Option<f64>,
) -> Result<(Option<f64>, Option<f64>), RpsBatchServiceError> {
    match (width_mm, micron) {
        (None, None) => Ok((None, None)),
        (Some(width_mm), Some(micron))
            if width_mm.is_finite() && width_mm > 0.0 && micron.is_finite() && micron > 0.0 =>
        {
            Ok((Some(width_mm), Some(micron)))
        }
        (Some(_), Some(_)) => Err(RpsBatchServiceError::InvalidInput(
            "width_mm_and_micron_must_be_positive".to_string(),
        )),
        _ => Err(RpsBatchServiceError::InvalidInput(
            "width_mm_and_micron_required_together".to_string(),
        )),
    }
}

fn apply_update(
    batch: &mut RpsBatchSession,
    request: RpsBatchUpdateRequest,
) -> Result<(), RpsBatchServiceError> {
    let RpsBatchUpdateRequest {
        item_code,
        item_name,
        warehouse,
        width_mm,
        micron,
        quantity_source,
        tare_enabled,
        tare_kg,
        ..
    } = request;
    let item_code = trim_owned(item_code);
    let warehouse = trim_owned(warehouse);
    if item_code.is_empty() || warehouse.is_empty() {
        return Err(RpsBatchServiceError::InvalidInput(
            "item_code_and_warehouse_required".to_string(),
        ));
    }
    let (width_mm, micron) = normalize_dimensions(width_mm, micron)?;
    batch.item_name = trimmed_or(item_name, &item_code);
    batch.item_code = item_code;
    batch.warehouse = warehouse;
    batch.width_mm = width_mm;
    batch.micron = micron;
    if let Some(quantity_source) = quantity_source {
        batch.quantity_source = normalize_quantity_source(quantity_source);
    }
    if tare_enabled.is_some() || tare_kg.is_some() {
        let tare_enabled = tare_enabled.unwrap_or(batch.tare_enabled);
        let tare_kg = tare_kg.unwrap_or(if tare_enabled { batch.tare_kg } else { 0.0 });
        (batch.tare_enabled, batch.tare_kg) = normalize_tare(tare_enabled, tare_kg);
    }
    Ok(())
}

fn batch_id(client_batch_id: String, owner_key: &str) -> String {
    let client_batch_id = trim_owned(client_batch_id);
    if !client_batch_id.is_empty() {
        return client_batch_id;
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

fn role_name(role: PrincipalRole) -> &'static str {
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

fn first_non_empty(values: [&str; 3]) -> &str {
    values
        .into_iter()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .unwrap_or("unknown")
}

fn trim_trailing_slashes(value: String) -> String {
    let mut value = trim_owned(value);
    let end = value.trim_end_matches('/').len();
    value.truncate(end);
    value
}

fn lowercase_or(value: String, default: &str) -> String {
    let mut value = trim_owned(value);
    if value.is_empty() {
        default.to_string()
    } else {
        value.make_ascii_lowercase();
        value
    }
}

fn positive_or_zero(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        0.0
    }
}

fn validate_stop_context(
    batch: &RpsBatchSession,
    request: &RpsBatchStopRequest,
) -> Result<(), RpsBatchServiceError> {
    let batch_id = request.batch_id.trim();
    if batch_id.is_empty() || request.expected_revision == 0 {
        return Err(RpsBatchServiceError::InvalidInput(
            "batch_context_required".to_string(),
        ));
    }
    if batch.id.trim() != batch_id {
        return Err(RpsBatchServiceError::BatchContextConflict);
    }
    if batch.active && batch.revision != request.expected_revision {
        return Err(RpsBatchServiceError::BatchContextConflict);
    }
    Ok(())
}

fn validate_update_context(
    batch: &RpsBatchSession,
    request: &RpsBatchUpdateRequest,
) -> Result<(), RpsBatchServiceError> {
    if request.batch_id.trim().is_empty() || request.expected_revision == 0 {
        return Err(RpsBatchServiceError::InvalidInput(
            "batch_context_required".to_string(),
        ));
    }
    if batch.id.trim() != request.batch_id.trim() || batch.revision != request.expected_revision {
        return Err(RpsBatchServiceError::BatchContextConflict);
    }
    Ok(())
}

fn validate_print_context(
    batch: &RpsBatchSession,
    request: &RpsBatchPrintRequest,
) -> Result<(), RpsBatchServiceError> {
    if request.batch_id.trim().is_empty()
        || request.expected_revision == 0
        || request.expected_item_code.trim().is_empty()
        || request.expected_warehouse.trim().is_empty()
    {
        return Err(RpsBatchServiceError::InvalidInput(
            "batch_context_required".to_string(),
        ));
    }
    if batch.id.trim() != request.batch_id.trim()
        || batch.revision != request.expected_revision
        || batch.item_code.trim() != request.expected_item_code.trim()
        || batch.warehouse.trim() != request.expected_warehouse.trim()
    {
        return Err(RpsBatchServiceError::BatchContextConflict);
    }
    Ok(())
}

fn next_revision(current: u64) -> u64 {
    current.saturating_add(1)
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RpsBatchServiceError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("batch not active")]
    BatchNotActive,
    #[error("batch already active")]
    BatchAlreadyActive,
    #[error("batch context conflict")]
    BatchContextConflict,
    #[error("store failed")]
    StoreFailed,
}

impl From<RpsBatchStoreError> for RpsBatchServiceError {
    fn from(_: RpsBatchStoreError) -> Self {
        Self::StoreFailed
    }
}
