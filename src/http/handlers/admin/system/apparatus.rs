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

pub async fn apparatus_create(
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
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    require_capability(&state, &principal, Capability::ProductionMapManage).await?;
    let input: ApparatusUpsert = parse_json(&body)?;
    let name = state
        .apparatus_groups
        .upsert_apparatus(input)
        .await
        .map_err(apparatus_group_error)?;
    Ok(json_response(apparatus_warehouse(name)))
}

fn apparatus_group_error(error: ApparatusGroupError) -> AdminError {
    match error {
        ApparatusGroupError::MissingName => bad_request("group name is required"),
        ApparatusGroupError::MissingApparatus => bad_request("apparatus is required"),
        ApparatusGroupError::StoreFailed => server_error("apparatus group store failed"),
    }
}

pub(super) fn is_apparat_parent(parent: &str) -> bool {
    matches!(
        parent.trim().to_lowercase().as_str(),
        "aparat" | "aparat - a" | "apparat" | "apparat - a"
    )
}

pub(super) fn apparatus_warehouse(name: String) -> crate::core::admin::models::AdminWarehouse {
    crate::core::admin::models::AdminWarehouse {
        warehouse: name,
        company: String::new(),
        is_group: false,
        parent_warehouse: "aparat - A".to_string(),
    }
}
