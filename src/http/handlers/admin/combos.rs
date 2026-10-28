use super::*;
use crate::core::calculate_orders::CalculateOrderTemplate;
use crate::core::combo::ComboError;

#[derive(Default, Deserialize)]
pub struct ComboQuery {
    #[serde(default)]
    item_code: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveComboRequest {
    name: String,
    #[serde(default)]
    combo_id: Option<String>,
    template: CalculateOrderTemplate,
}

pub(super) fn combo_error(error: ComboError) -> AdminError {
    match error {
        ComboError::InvalidInput(message) => {
            let mut response = AdminErrorResponse::new("combo_invalid");
            response.message = Some(message);
            (StatusCode::BAD_REQUEST, Json(response))
        }
        ComboError::NameConflict => conflict("combo_name_conflict"),
        ComboError::NotFound => not_found("combo_not_found"),
        ComboError::ReferenceMismatch => conflict("combo_reference_mismatch"),
        ComboError::StoreFailed => server_error("combo_store_failed"),
    }
}

pub(crate) async fn validate_combo_template(
    state: &AppState,
    template: &CalculateOrderTemplate,
) -> Result<(), AdminError> {
    crate::core::combo::validate_reference(template).map_err(combo_error)?;
    match &state.combos {
        Some(store) => store.validate(template).await.map_err(combo_error),
        None if template.combo.is_none() => Ok(()),
        None => Err(server_error("combo_store_unavailable")),
    }
}

/// Map-only routes have no Calculate template. Check the catalog identity even
/// when a caller omits the combo tag, rather than treating a combo SKU as stock.
pub(crate) async fn validate_combo_map_catalog_identity(
    state: &AppState,
    map: &crate::core::production_map::ProductionMapDefinition,
) -> Result<(), AdminError> {
    if map.combo.is_some() {
        return Ok(());
    }
    validate_combo_template(
        state,
        &CalculateOrderTemplate {
            item_code: map.product_code.clone(),
            products: map.products.clone(),
            ..Default::default()
        },
    )
    .await
}

pub(crate) fn validate_combo_map_update(
    map: &crate::core::production_map::ProductionMapDefinition,
    previous: Option<&crate::core::production_map::ProductionMapDefinition>,
) -> Result<(), AdminError> {
    if map.combo.is_some() || previous.is_some_and(|map| map.combo.is_some()) {
        let Some(previous) = previous else {
            return Err(bad_request("combo_template_required"));
        };
        if map.combo != previous.combo
            || map.products != previous.products
            || map.product_code != previous.product_code
            || map.title != previous.title
            || map.width_mm != previous.width_mm
            || map.print_val_size_mm != previous.print_val_size_mm
            || map.roll_count != previous.roll_count
            || map.image_id != previous.image_id
        {
            return Err(conflict("combo_reference_mismatch"));
        }
    }
    Ok(())
}

pub async fn combos(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<ComboQuery>,
    body: Bytes,
) -> Result<Response, AdminError> {
    match method {
        Method::POST => {
            let principal = authorize_any_capability(
                &state,
                &headers,
                &[
                    Capability::AdminAccess,
                    Capability::ProductionMapManage,
                    Capability::ProductionOrderCreate,
                ],
            )
            .await?;
            require_capability(&state, &principal, Capability::CatalogItemCreate).await?;
            let request: SaveComboRequest = parse_json(&body)?;
            if let Some(combo) = &request.template.combo {
                if request.combo_id.as_deref() != Some(combo.id.as_str()) {
                    return Err(conflict("combo_reference_mismatch"));
                }
            }
            // Validate every component/customer before creating the catalog item.
            let mut components = request.template.clone();
            components.combo = None;
            components.item_code = components
                .products
                .first()
                .map(|product| product.item_code.clone())
                .unwrap_or_default();
            production_maps::validate_multi_product_customer(&state, &components).await?;
            let store = state
                .combos
                .as_ref()
                .ok_or_else(|| server_error("combo_store_unavailable"))?;
            let combo = store
                .save(
                    &request.name,
                    request.combo_id.as_deref(),
                    &request.template,
                )
                .await
                .map_err(combo_error)?;
            state.production_maps.notify_live();
            Ok(json_response(serde_json::json!({"ok":true,"combo":combo})))
        }
        Method::GET => {
            let principal = authorize_any_capability(
                &state,
                &headers,
                &[
                    Capability::AdminAccess,
                    Capability::ProductionMapManage,
                    Capability::QolipManage,
                    Capability::ProductionMapRead,
                ],
            )
            .await?;
            let full = state
                .admin
                .principal_has_capability(&principal, Capability::AdminAccess)
                .await
                || state
                    .admin
                    .principal_has_capability(&principal, Capability::ProductionMapManage)
                    .await;
            if !full {
                // Match the Qolip catalog's no-mold identity visibility: a mold
                // warehouse assignment is necessary even before tooling exists.
                let warehouses = state
                    .qolip
                    .warehouses_for_principal(&principal, false)
                    .await
                    .map_err(|_| server_error("combo_scope_failed"))?;
                if warehouses.is_empty() {
                    return Err(forbidden());
                }
            }
            let code = query.item_code.trim();
            if code.is_empty() {
                return Err(bad_request("combo_item_code_required"));
            }
            let store = state
                .combos
                .as_ref()
                .ok_or_else(|| server_error("combo_store_unavailable"))?;
            let entry = store
                .get(code)
                .await
                .map_err(combo_error)?
                .ok_or_else(|| not_found("combo_not_found"))?;
            let template = if full {
                serde_json::to_value(&entry.template)
                    .map_err(|_| server_error("combo_store_failed"))?
            } else {
                tooling_template(&entry)
            };
            Ok(json_response(
                serde_json::json!({"ok":true,"combo":entry.combo,"template":template}),
            ))
        }
        _ => Err(method_not_allowed()),
    }
}

/// The warehouse catalog exposes product identities; combo details add only the
/// physical layout needed to prepare tooling, not the private order template.
fn tooling_template(entry: &crate::core::combo::ComboCatalogEntry) -> serde_json::Value {
    serde_json::json!({
        "combo":entry.combo,"products":entry.template.products,
        "item_code":entry.combo.item_code,"name":entry.combo.name,"product":entry.combo.name,
        "frame_product_size_mm":entry.template.frame_product_size_mm,
        "frame_count":entry.template.frame_count,"width_mm":entry.template.width_mm,
        "edge_allowance_mm":entry.template.edge_allowance_mm,
        "print_val_size_mm":entry.template.print_val_size_mm,
        "production_options":entry.template.production_options,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combo_map_only_updates_preserve_typed_identity_and_physical_layout() {
        let original: crate::core::production_map::ProductionMapDefinition = serde_json::from_value(serde_json::json!({
            "id":"zakaz-combo", "product_code":"COMBO-R1", "title":"Combo", "width_mm":575,
            "combo":{"id":"master","item_code":"COMBO-R1","name":"Combo","revision":1,"layout_fingerprint":"fp"},
            "products":crate::core::order_layout::tests::products(),"nodes":[],"edges":[]
        })).unwrap();
        let mut altered = original.clone();
        altered.combo = None;
        assert!(validate_combo_map_update(&altered, Some(&original)).is_err());
        altered = original.clone();
        altered.products.swap(0, 1);
        assert!(validate_combo_map_update(&altered, Some(&original)).is_err());
        altered = original.clone();
        altered.roll_count = Some(10);
        assert!(validate_combo_map_update(&altered, Some(&original)).is_err());
        assert!(validate_combo_map_update(&original, None).is_err());
        altered = original.clone();
        altered.order_kg = Some(900.0);
        assert!(validate_combo_map_update(&altered, Some(&original)).is_ok());
    }

    #[test]
    fn combo_tooling_details_expose_layout_without_private_order_fields() {
        let entry = crate::core::combo::ComboCatalogEntry {
            combo: crate::core::combo::ComboReference {
                id: "combo-1".into(),
                item_code: "COMBO-R1".into(),
                name: "Combo".into(),
                revision: 1,
                layout_fingerprint: "fingerprint".into(),
            },
            template: CalculateOrderTemplate {
                products: crate::core::order_layout::tests::products(),
                width_mm: 575.0,
                kg: 300.0,
                customer: "Private Customer".into(),
                customer_ref: "PRIVATE-REF".into(),
                note: "Private note".into(),
                image_id: "private-photo".into(),
                ..Default::default()
            },
        };
        let payload = tooling_template(&entry);
        assert_eq!(payload["products"].as_array().unwrap().len(), 4);
        assert_eq!(payload["width_mm"], 575.0);
        assert_eq!(payload["combo"]["item_code"], "COMBO-R1");
        for key in [
            "customer",
            "customer_ref",
            "note",
            "kg",
            "image_id",
            "layers",
            "source_map_id",
        ] {
            assert!(
                payload.get(key).is_none(),
                "private {key} leaked to warehouse reader"
            );
        }
    }
}
