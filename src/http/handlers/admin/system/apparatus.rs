use super::*;

pub async fn apparatus_groups(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    if !matches!(method, Method::GET | Method::PUT) {
        return Err(method_not_allowed());
    }
    match method {
        Method::GET => {
            require_capability(&state, &principal, Capability::ProductionMapManage).await?;
            state
                .apparatus_groups
                .groups()
                .await
                .map(json_response)
                .map_err(apparatus_group_error)
        }
        Method::PUT => {
            require_capability(&state, &principal, Capability::ProductionMapManage).await?;
            let input: ApparatusGroupUpsert = parse_json(&body)?;
            state
                .apparatus_groups
                .upsert_group(input)
                .await
                .map(json_response)
                .map_err(apparatus_group_error)
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn apparatus(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<ItemQuery>,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::CatalogItemRead,
            Capability::ApparatusQueueRead,
            Capability::RawMaterialAssign,
        ],
    )
    .await?;
    if !matches!(method, Method::GET | Method::POST) {
        return Err(method_not_allowed());
    }
    match method {
        Method::GET => {
            let limit = optional_search_limit(query.limit.as_deref(), 50, 500);
            state
                .apparatus_groups
                .apparatus(query.q.as_deref().unwrap_or(""), limit)
                .await
                .map(|items| {
                    items
                        .into_iter()
                        .map(|name| AdminApparatusResponse { name })
                        .collect::<Vec<_>>()
                })
                .map(json_response)
                .map_err(apparatus_group_error)
        }
        Method::POST => {
            require_capability(&state, &principal, Capability::ProductionMapManage).await?;
            let input: ApparatusUpsert = parse_json(&body)?;
            let name = state
                .apparatus_groups
                .upsert_apparatus(input)
                .await
                .map_err(apparatus_group_error)?;
            Ok(json_response(AdminApparatusMutationResponse::new(name)))
        }
        _ => Err(method_not_allowed()),
    }
}

#[derive(Serialize)]
struct AdminApparatusResponse {
    name: String,
}

#[derive(Serialize)]
struct AdminApparatusMutationResponse {
    name: String,
    // Compatibility fields for already released clients.
    warehouse: String,
    company: String,
    is_group: bool,
    parent_warehouse: String,
}

impl AdminApparatusMutationResponse {
    fn new(name: String) -> Self {
        Self {
            warehouse: name.clone(),
            name,
            company: String::new(),
            is_group: false,
            parent_warehouse: "aparat - A".to_string(),
        }
    }
}

fn apparatus_group_error(error: ApparatusGroupError) -> AdminError {
    match error {
        ApparatusGroupError::MissingName => bad_request("group name is required"),
        ApparatusGroupError::MissingApparatus => bad_request("apparatus is required"),
        ApparatusGroupError::InvalidApparatus => bad_request("apparatus is invalid"),
        ApparatusGroupError::StoreFailed => server_error("apparatus group store failed"),
    }
}

pub(super) fn is_legacy_apparatus_parent(parent: &str) -> bool {
    matches!(
        parent.trim().to_lowercase().as_str(),
        "aparat" | "aparat - a" | "apparat" | "apparat - a"
    )
}

pub(super) fn legacy_apparatus_warehouse(
    name: String,
) -> crate::core::admin::models::AdminWarehouse {
    crate::core::admin::models::AdminWarehouse {
        warehouse: name,
        company: String::new(),
        is_group: false,
        parent_warehouse: "aparat - A".to_string(),
    }
}
