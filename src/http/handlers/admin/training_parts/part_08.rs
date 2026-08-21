
fn training_store(state: &AppState) -> Result<&PostgresTrainingWorkspaceStore, AdminError> {
    state
        .training_workspace
        .as_ref()
        .ok_or_else(|| server_error("training workspace unavailable"))
}

fn training_calculate_error(error: CalculateOrderError) -> AdminError {
    match error {
        CalculateOrderError::InvalidInput(detail) => bad_request(detail),
        CalculateOrderError::StoreFailed => server_error("calculate order save failed"),
    }
}

pub(super) fn training_workspace_error(error: TrainingWorkspaceError) -> AdminError {
    match error {
        TrainingWorkspaceError::StoreFailed => server_error("training workspace store failed"),
        TrainingWorkspaceError::MapNotFound => not_found("training_map_not_found"),
        TrainingWorkspaceError::DuplicateOrderNumber => conflict("training_order_number_exists"),
        TrainingWorkspaceError::DuplicateRawMaterialAssignment => {
            conflict("training_material_assignment_exists")
        }
        TrainingWorkspaceError::InvalidInput(detail)
        | TrainingWorkspaceError::InvalidMap(detail) => bad_request(detail),
    }
}

fn image_extension(mime: &str) -> Option<&'static str> {
    match mime {
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

fn clean_file_name(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !matches!(ch, '/' | '\\' | '\0' | '\r' | '\n'))
        .collect::<String>()
        .trim()
        .to_string()
}

fn safe_image_id(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
}

fn query_value(uri: &Uri, key: &str) -> Option<String> {
    uri.query()?.split('&').find_map(|pair| {
        let (raw_key, raw_value) = pair.split_once('=')?;
        (raw_key == key).then(|| raw_value.trim().to_string())
    })
}

fn unix_micros() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_micros())
        .unwrap_or_default()
}

include!("../training_inline_tests.rs");
