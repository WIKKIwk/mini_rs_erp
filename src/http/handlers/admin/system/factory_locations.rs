use super::*;

pub async fn factory_locations(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    authorize_capability(&state, &headers, Capability::FactoryLocationManage).await?;
    match method {
        Method::GET => state
            .factory_locations
            .list()
            .await
            .map(json_response)
            .map_err(factory_location_error),
        Method::POST => {
            let input: FactoryLocationCreate = parse_json(&body)?;
            state
                .factory_locations
                .create(input)
                .await
                .map(json_response)
                .map_err(factory_location_error)
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn factory_location(
    State(state): State<AppState>,
    Path(id): Path<String>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    authorize_capability(&state, &headers, Capability::FactoryLocationManage).await?;
    if method != Method::PUT {
        return Err(method_not_allowed());
    }
    let input: FactoryLocationUpdate = parse_json(&body)?;
    state
        .factory_locations
        .update(&id, input)
        .await
        .map(json_response)
        .map_err(factory_location_error)
}

pub async fn factory_location_apparatus(
    State(state): State<AppState>,
    Path(id): Path<String>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    authorize_capability(&state, &headers, Capability::FactoryLocationManage).await?;
    if method != Method::PUT {
        return Err(method_not_allowed());
    }
    let input: FactoryLocationApparatusReplace = parse_json(&body)?;
    state
        .factory_locations
        .replace_apparatus(&id, input)
        .await
        .map(json_response)
        .map_err(factory_location_error)
}

fn factory_location_error(error: FactoryLocationError) -> AdminError {
    match error {
        FactoryLocationError::MissingName => bad_request("state name is required"),
        FactoryLocationError::MissingUpdate => bad_request("state update is required"),
        FactoryLocationError::InvalidApparatus => bad_request("apparatus id is invalid"),
        FactoryLocationError::DuplicateName => conflict("state name already exists"),
        FactoryLocationError::NotFound => not_found("state not found"),
        FactoryLocationError::StoreFailed => server_error("factory location store failed"),
    }
}
