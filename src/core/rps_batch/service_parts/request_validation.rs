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
    if !batch
        .length_m
        .is_some_and(|length_m| length_m.is_finite() && length_m > 0.0)
    {
        return Err(RpsBatchServiceError::InvalidInput(
            "length_m_required".to_string(),
        ));
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
