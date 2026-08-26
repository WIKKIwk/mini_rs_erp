use super::*;

pub async fn apparatus_collections(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = super::apparatus::authorize_apparatus(&state, &headers).await?;
    match method {
        Method::GET => state
            .apparatus_collections
            .list()
            .await
            .map(json_response)
            .map_err(apparatus_collection_error),
        Method::POST => {
            require_capability(&state, &principal, Capability::ProductionMapManage).await?;
            let input: ApparatusCollectionCreate = parse_json(&body)?;
            state
                .apparatus_collections
                .create(input)
                .await
                .map(json_response)
                .map_err(apparatus_collection_error)
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn apparatus_collection(
    State(state): State<AppState>,
    Path(id): Path<String>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = super::apparatus::authorize_apparatus(&state, &headers).await?;
    require_capability(&state, &principal, Capability::ProductionMapManage).await?;
    match method {
        Method::PUT => {
            let input: ApparatusCollectionUpdate = parse_json(&body)?;
            state
                .apparatus_collections
                .update(&id, input)
                .await
                .map(json_response)
                .map_err(apparatus_collection_error)
        }
        Method::DELETE => {
            let input: ApparatusCollectionDelete = parse_json(&body)?;
            state
                .apparatus_collections
                .delete(&id, input)
                .await
                .map_err(apparatus_collection_error)?;
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        _ => Err(method_not_allowed()),
    }
}

fn apparatus_collection_error(error: ApparatusCollectionError) -> AdminError {
    match error {
        ApparatusCollectionError::MissingName => {
            bad_request("apparatus collection name is required")
        }
        ApparatusCollectionError::NameTooLong => {
            bad_request("apparatus collection name is too long")
        }
        ApparatusCollectionError::TooManyApparatus => {
            bad_request("apparatus collection has too many apparatus")
        }
        ApparatusCollectionError::InvalidApparatus => bad_request("apparatus id is invalid"),
        ApparatusCollectionError::DuplicateName => {
            conflict("apparatus collection name already exists")
        }
        ApparatusCollectionError::NotFound => not_found("apparatus collection not found"),
        ApparatusCollectionError::InvalidRevision => {
            bad_request("apparatus collection revision is invalid")
        }
        ApparatusCollectionError::RevisionConflict => {
            conflict("apparatus collection revision conflict")
        }
        ApparatusCollectionError::StoreFailed => server_error("apparatus collection store failed"),
    }
}
