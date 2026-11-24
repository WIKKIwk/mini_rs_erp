pub async fn item_group_tree(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::CatalogItemGroupRead,
            Capability::GscaleCatalogRead,
        ],
    )
    .await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let groups = state
        .admin
        .item_group_tree()
        .await
        .map_err(|_| server_error("admin item group tree failed"))?;
    if principal.role == PrincipalRole::MaterialTaminotchi {
        return Ok(json_response(
            scoped_item_group_tree(&state, &principal, groups).await?,
        ));
    }
    Ok(json_response(groups))
}

include!("../customers_mutations.rs");
