
fn training_progress_batch_from_payload(
    payload: serde_json::Value,
) -> Result<OrderProgressBatch, TrainingWorkspaceError> {
    let batch = serde_json::from_value::<OrderProgressBatch>(payload).map_err(|error| {
        tracing::warn!(%error, "invalid persisted training progress batch");
        TrainingWorkspaceError::StoreFailed
    })?;
    for apparatus in [
        batch.apparatus.as_str(),
        batch.current_apparatus.as_str(),
        batch.next_apparatus.as_str(),
        batch.used_by_apparatus.as_str(),
        batch.processed_by_apparatus.as_str(),
    ] {
        if !apparatus.trim().is_empty() && canonical_training_apparatus(apparatus).is_err() {
            return Err(TrainingWorkspaceError::StoreFailed);
        }
    }
    Ok(batch)
}

fn is_production_progress_qr(value: &str) -> bool {
    let value = value.trim().as_bytes();
    value.len() == 24
        && value[..4].eq_ignore_ascii_case(b"4001")
        && value[4..].iter().all(u8::is_ascii_hexdigit)
}

fn training_calculate_error(error: CalculateOrderError) -> TrainingWorkspaceError {
    match error {
        CalculateOrderError::InvalidInput(detail) => TrainingWorkspaceError::InvalidInput(detail),
        CalculateOrderError::StoreFailed => TrainingWorkspaceError::StoreFailed,
    }
}

fn unix_micros() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_micros())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{
        TRAINING_VIRTUAL_INPUT_BOSMA, canonical_training_apparatus, is_production_progress_qr,
        training_virtual_input_id,
    };

    #[test]
    fn recognizes_only_production_progress_qr_payloads() {
        assert!(is_production_progress_qr("400118904D9F447100000F96"));
        assert!(is_production_progress_qr("400118904d9f447100000f96"));
        assert!(!is_production_progress_qr(
            "TRAINING-INPUT:training-zakaz-0005"
        ));
        assert!(!is_production_progress_qr("4001🚫000000000000000"));
    }

    #[test]
    fn training_store_accepts_canonical_ids_but_not_renamed_titles() {
        let id = canonical_training_apparatus("apparatus:training:lam-001")
            .expect("canonical training apparatus");
        assert_eq!(id.as_str(), "apparatus:training:lam-001");
        assert!(canonical_training_apparatus("Renamed laminatsiya").is_err());
    }

    #[test]
    fn virtual_training_input_is_not_canonical_or_production_fallback() {
        assert!(canonical_training_apparatus(TRAINING_VIRTUAL_INPUT_BOSMA).is_err());
        assert_eq!(
            training_virtual_input_id(TRAINING_VIRTUAL_INPUT_BOSMA).expect("virtual input"),
            TRAINING_VIRTUAL_INPUT_BOSMA
        );
        assert!(!is_production_progress_qr("TRAINING-INPUT:training-1001"));
    }
}
