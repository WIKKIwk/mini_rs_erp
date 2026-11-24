use super::*;

fn assert_code(error: ProductionMapError, expected: &str) {
    let (_, response) = production_map_error(error);
    assert_eq!(response.0.error, expected);
}

#[test]
fn validation_errors_use_stable_codes_instead_of_display_text() {
    assert_code(ProductionMapError::MissingId, "map_id_required");
    assert_code(
        ProductionMapError::DuplicateNode("node-1".to_string()),
        "duplicate_node_id",
    );
    assert_code(
        ProductionMapError::InvalidFormulaExpression("x".to_string()),
        "invalid_formula_expression",
    );
    assert_code(
        ProductionMapError::QueueSequenceOrderNotFound("missing".to_string()),
        "queue_sequence_order_not_found",
    );
    assert_code(
        ProductionMapError::QueueSequenceApparatusMismatch("zakaz-1".to_string()),
        "queue_sequence_apparatus_mismatch",
    );
    assert_code(ProductionMapError::Cycle, "production_map_cycle");
}

#[test]
fn capacity_errors_keep_their_specific_http_and_api_identity() {
    let (status, response) = production_map_error(ProductionMapError::CapacityConflict);
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(response.0.error, "capacity_conflict");

    let (status, response) =
        production_map_error(ProductionMapError::ScheduleReservationNotFound);
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(response.0.error, "schedule_reservation_not_found");
}
