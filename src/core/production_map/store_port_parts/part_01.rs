
pub type StoreResult<T> = Result<T, ProductionMapError>;
pub type ApparatusSequenceMap = BTreeMap<String, Vec<String>>;
pub type QueueStateMap = BTreeMap<String, String>;
pub type ApparatusQueueStateMap = BTreeMap<String, QueueStateMap>;
pub type OrderLogMap = BTreeMap<String, Vec<ProductionOrderLogEntry>>;
pub type OrderControlMap = BTreeMap<String, OrderControlRecord>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawMaterialStockTransitionKind {
    InUse,
    Consumed,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawMaterialStockTransition {
    pub kind: RawMaterialStockTransitionKind,
    pub barcodes: Vec<String>,
    pub order_id: String,
}

impl RawMaterialStockTransition {
    pub fn new(
        kind: RawMaterialStockTransitionKind,
        barcodes: Vec<String>,
        order_id: &str,
    ) -> Self {
        Self {
            kind,
            barcodes: barcodes
                .into_iter()
                .map(|barcode| barcode.trim().to_string())
                .filter(|barcode| !barcode.is_empty())
                .collect(),
            order_id: order_id.trim().to_string(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.barcodes.is_empty() || self.order_id.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueueActionProgressWriteResult {
    pub raw_material_stock_warehouses: Vec<String>,
    pub raw_material_stock_committed: bool,
}

#[derive(Debug, Clone)]
pub struct QueueActionProgressWrite {
    pub apparatus: String,
    /// An alternative-assignment map replacement that must commit with the
    /// queue/session/progress/material write. PostgreSQL applies this through
    /// the same database transaction; test stores apply it at the same store
    /// boundary rather than exposing a caller-managed rollback.
    pub map_update: Option<ProductionMapDefinition>,
    pub states: QueueStateMap,
    /// Persisted sequence replacements that belong to the same queue write.
    /// PostgreSQL applies these in the same transaction as the queue state,
    /// event, session, and control transition.
    pub sequence_updates: BTreeMap<String, Vec<String>>,
    pub event: ApparatusQueueActionEvent,
    pub session: Option<OrderRunSession>,
    pub progress_event: Option<OrderProgressEvent>,
    pub progress_batch: Option<OrderProgressBatch>,
    pub progress_batches: Vec<OrderProgressBatch>,
    pub progress_batch_updates: Vec<OrderProgressBatch>,
    pub opening_wip_batch_updates: Vec<OpeningWipBatch>,
    pub raw_material_stock_transitions: Vec<RawMaterialStockTransition>,
    pub qolip_checkouts: Vec<QolipCheckout>,
    pub returned_paint_report: Option<ReturnedPaintRequest>,
    pub order_control_update: Option<OrderControlRecord>,
    pub schedule_reservation_status: Option<ApparatusScheduleStatus>,
}

pub struct ProductionMapApparatusTransferWrite {
    pub record: ProductionMapApparatusTransferRecord,
    pub updated_map: ProductionMapDefinition,
    pub from_sequence: Vec<String>,
    pub to_sequence: Vec<String>,
    pub from_states: QueueStateMap,
    pub to_states: QueueStateMap,
    pub target_apparatus_id: String,
    pub session: OrderRunSession,
    pub progress_batch: OrderProgressBatch,
    pub progress_batch_updates: Vec<OrderProgressBatch>,
    pub raw_material_assignments: Vec<RawMaterialAssignment>,
}

pub(crate) fn validate_queue_progress_write(write: &QueueActionProgressWrite) -> StoreResult<()> {
    require_live_apparatus(&write.apparatus)?;
    require_queue_event_apparatus(&write.event)?;
    for apparatus in write.sequence_updates.keys() {
        require_live_apparatus(apparatus)?;
    }
    if let Some(session) = &write.session {
        require_live_apparatus(&session.apparatus)?;
    }
    if let Some(event) = &write.progress_event {
        require_live_apparatus(&event.apparatus)?;
    }
    for batch in write
        .progress_batch
        .iter()
        .chain(write.progress_batches.iter())
        .chain(write.progress_batch_updates.iter())
    {
        require_progress_batch_apparatus(batch)?;
    }
    for batch in &write.opening_wip_batch_updates {
        if batch.order_id.trim().is_empty() || batch.batch_id.trim().is_empty() {
            return Err(ProductionMapError::StoreFailed);
        }
        for apparatus in [
            batch.used_by_apparatus.as_str(),
            batch.processed_by_apparatus.as_str(),
        ] {
            if !apparatus.trim().is_empty() {
                require_live_apparatus(apparatus)?;
            }
        }
    }
    if let Some(report) = &write.returned_paint_report {
        require_live_apparatus(&report.apparatus)?;
    }
    if let Some(record) = &write.order_control_update
        && let Some(request) = &record.freeze_request
        && !request.target_apparatus.trim().is_empty()
    {
        require_live_apparatus(&request.target_apparatus)?;
    }
    Ok(())
}

fn require_live_apparatus(value: &str) -> StoreResult<ApparatusId> {
    ApparatusId::new(value.trim().to_string()).map_err(|_| ProductionMapError::StoreFailed)
}

fn require_queue_event_apparatus(event: &ApparatusQueueActionEvent) -> StoreResult<()> {
    require_live_apparatus(&event.apparatus)?;
    for apparatus in &event.assigned_apparatus {
        require_live_apparatus(apparatus)?;
    }
    Ok(())
}

fn require_progress_batch_apparatus(batch: &OrderProgressBatch) -> StoreResult<()> {
    require_live_apparatus(&batch.apparatus)?;
    if !batch.current_apparatus_key.trim().is_empty() {
        require_live_apparatus(&batch.current_apparatus_key)?;
    }
    for apparatus in [
        batch.current_apparatus.as_str(),
        batch.next_apparatus.as_str(),
        batch.used_by_apparatus.as_str(),
        batch.processed_by_apparatus.as_str(),
    ] {
        if !apparatus.trim().is_empty() && !is_warehouse_processing_marker(apparatus) {
            require_live_apparatus(apparatus)?;
        }
    }
    Ok(())
}

fn is_warehouse_processing_marker(value: &str) -> bool {
    value.trim().to_ascii_lowercase().starts_with("warehouse:")
}
