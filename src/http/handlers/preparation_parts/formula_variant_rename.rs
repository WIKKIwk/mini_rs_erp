pub async fn formula_variant_rename(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(target): Query<FormulaVariantTarget>,
    Json(input): Json<FormulaVariantRename>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?.rename_formula_variant(&actor.ref_, target, &input.name).await.map(Json).map_err(error)
}

pub async fn formula_variant_reuse(
    State(state): State<AppState>, headers: HeaderMap,
    Query(query): Query<FormulaVariantOrderContext>, Json(input): Json<FormulaVariantReuse>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?.reuse_formula_variant_in_order(&actor.ref_, input, &query.order_id).await.map(Json).map_err(error)
}

pub async fn formula_variant_remove_from_order(
    State(state): State<AppState>, headers: HeaderMap,
    Query(query): Query<FormulaVariantOrderRemove>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?.remove_formula_variant_from_order(&actor.ref_, query).await.map(Json).map_err(error)
}

#[derive(Debug, Deserialize)]
pub struct OrderMaterialsQuery {
    pub order_id: String,
}

pub async fn formula_upsert(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FormulaVariantQuery>,
    Json(input): Json<FormulaUpsert>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?
        .upsert_formula_in_order(&actor, input, &query.variant_id, &query.order_id)
        .await
        .map(Json)
        .map_err(error)
}

pub async fn formula_shares_update(
    State(state): State<AppState>, headers: HeaderMap, Query(query): Query<FormulaVariantQuery>, Json(input): Json<FormulaSharesUpdate>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?.update_formula_shares_in_order(&actor.ref_, input, false, &query.variant_id, &query.order_id).await.map(Json).map_err(error)
}

pub async fn saved_formula_shares_update(
    State(state): State<AppState>, headers: HeaderMap, Query(query): Query<FormulaVariantQuery>, Json(input): Json<FormulaSharesUpdate>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?.update_formula_variant_shares(&actor.ref_, input, true, &query.variant_id).await.map(Json).map_err(error)
}

pub async fn formula_show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FormulaQuery>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    if !query.order_id.trim().is_empty() {
        return store(&state)?.list_order_formula_components(
            &actor.ref_, query.product_code.trim(), query.material_id.trim(),
            query.variant_id.trim(), query.order_id.trim(),
        ).await.map(Json).map_err(error);
    }
    store(&state)?
        .list_formulas_variant(&actor.ref_, &query.product_code, &query.material_id, &query.variant_id)
        .await
        .map(Json)
        .map_err(error)
}

pub async fn formula_orders(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?
        .formula_orders(&actor.ref_)
        .await
        .map(Json)
        .map_err(error)
}

pub async fn formula_sets(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?.formula_sets(&actor.ref_).await.map(Json).map_err(error)
}

#[derive(Debug, Default, Deserialize)]
pub struct FormulaCatalogQuery {
    #[serde(default)]
    pub product_code: String,
    #[serde(default)]
    pub material_id: String,
    #[serde(default)]
    pub order_id: String,
}

pub async fn formula_catalog(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FormulaCatalogQuery>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    if !query.product_code.trim().is_empty() || !query.material_id.trim().is_empty()
        || !query.order_id.trim().is_empty() {
        return store(&state)?.available_formula_variants(
            &actor.ref_, &query.product_code, &query.material_id, &query.order_id,
        ).await.map(Json).map_err(error);
    }
    store(&state)?.formula_catalog(&actor.ref_).await.map(Json).map_err(error)
}

pub async fn rename_formula_set(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<FormulaSetRename>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?.rename_formula_set(&actor.ref_, input).await.map(Json).map_err(error)
}

pub async fn saved_formula_show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FormulaQuery>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?.list_saved_formulas_variant(&actor.ref_, &query.product_code, &query.material_id, &query.variant_id)
        .await.map(Json).map_err(error)
}

pub async fn saved_formula_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FormulaVariantQuery>,
    Json(input): Json<FormulaUpsert>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?.update_saved_formula_variant(&actor, input, &query.variant_id).await.map(Json).map_err(error)
}

pub async fn saved_formula_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FormulaDeleteQuery>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?.delete_saved_formula_variant(&actor.ref_, &query.product_code, &query.name, &query.material_id, &query.variant_id)
        .await.map(Json).map_err(error)
}

pub async fn formula_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FormulaDeleteQuery>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?
        .delete_formula_in_order(
            &actor.ref_,
            &query.product_code,
            &query.name,
            &query.material_id,
            &query.variant_id,
            &query.order_id,
        )
        .await
        .map(Json)
        .map_err(error)
}

pub async fn order_materials(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<OrderMaterialsQuery>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    let result = store(&state)?
        .order_materials(&query.order_id)
        .await
        .map_err(error)?;
    // Faqat o'ziga biriktirilgan homashyolar qaytadi (fail-closed).
    let assigned: std::collections::BTreeSet<String> = store(&state)?
        .list_responsibilities(&actor.ref_)
        .await
        .map_err(error)?
        .get("materials")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|m| m.get("material_id")?.as_str())
                .map(|s| s.to_lowercase())
                .collect()
        })
        .unwrap_or_default();
    let mut filtered: Vec<Value> = result
        .get("materials")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter(|m| {
                    m.get("material_id")
                        .and_then(|s| s.as_str())
                        .is_some_and(|s| assigned.contains(&s.to_lowercase()))
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    order_estimates::enrich(&state, query.order_id.trim(), &mut filtered).await;
    Ok(Json(
        serde_json::json!({"order_id": result["order_id"], "materials": filtered}),
    ))
}

#[derive(Debug, Deserialize)]
pub struct ResponsibilityQuery {
    pub principal_ref: String,
}

pub async fn responsibilities_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ResponsibilityQuery>,
) -> Result<Json<Value>, ApiError> {
    let _admin = authorize_admin(&state, &headers).await?;
    store(&state)?
        .list_responsibilities(&query.principal_ref)
        .await
        .map(Json)
        .map_err(error)
}

pub async fn responsibilities_assign(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<MaterialResponsibilityAssign>,
) -> Result<Json<Value>, ApiError> {
    let _admin = authorize_admin(&state, &headers).await?;
    store(&state)?
        .assign_responsibility(input)
        .await
        .map(Json)
        .map_err(error)
}

pub async fn responsibilities_unassign(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ResponsibilityDeleteQuery>,
) -> Result<Json<Value>, ApiError> {
    let _admin = authorize_admin(&state, &headers).await?;
    store(&state)?
        .unassign_responsibility(MaterialResponsibilityDelete {
            principal_ref: query.principal_ref,
            material_id: query.material_id,
        })
        .await
        .map(Json)
        .map_err(error)
}

#[derive(Debug, Deserialize)]
pub struct ResponsibilityDeleteQuery {
    pub principal_ref: String,
    pub material_id: String,
}

/// Bola ombor ochish: faqat masterga eksklyuziv biriktirilgan ota ombor ostiga.
/// Yaratilgan bola avtomatik shu masterga biriktiriladi — snapshot,
/// kirim/sarf uni darhol ko'radi.
pub async fn create_warehouse(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<PreparationWarehouseCreate>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    let created = store(&state)?
        .create_child_warehouse(&actor, input)
        .await
        .map_err(error)?;
    state.warehouse_events.notify_updated(
        created["warehouse"].as_str().unwrap_or_default(),
        "warehouse_assignment",
    );
    Ok(Json(created))
}

pub async fn rename_warehouse(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<PreparationWarehouseRename>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    let old = input.warehouse.clone();
    let result = store(&state)?
        .rename_child_warehouse(&actor.ref_, input)
        .await
        .map_err(error)?;
    state.warehouse_events.notify_updated(&old, "warehouse_renamed");
    state.warehouse_events.notify_updated(
        result["warehouse"].as_str().unwrap_or_default(),
        "warehouse_renamed",
    );
    Ok(Json(result))
}

pub async fn delete_warehouse(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(input): Query<PreparationWarehouseDelete>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    let result = store(&state)?
        .delete_child_warehouse(&actor.ref_, &input.warehouse)
        .await
        .map_err(error)?;
    state.warehouse_events.notify_updated(
        &input.warehouse,
        "warehouse_deleted",
    );
    Ok(Json(result))
}
