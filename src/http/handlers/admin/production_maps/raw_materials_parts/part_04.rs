
fn raw_material_assignment_quantities(
    assignment: &RawMaterialAssignment,
    stock: Option<&RawMaterialStockEntry>,
) -> (f64, f64, f64) {
    let Some(stock) = stock else {
        return (0.0, 0.0, 0.0);
    };
    let qty = if stock.qty.is_finite() && stock.qty > 0.0 {
        stock.qty
    } else {
        0.0
    };
    let belongs_to_order = stock
        .reserved_order_id
        .trim()
        .eq_ignore_ascii_case(assignment.order_id.trim());
    let received = belongs_to_order
        && matches!(
            stock.status.trim().to_ascii_lowercase().as_str(),
            "in_use" | "consumed"
        );
    let consumed = belongs_to_order && stock.status.trim().eq_ignore_ascii_case("consumed");
    let received_qty = if received { qty } else { 0.0 };
    let consumed_qty = if consumed { qty } else { 0.0 };
    (
        received_qty,
        consumed_qty,
        (received_qty - consumed_qty).max(0.0),
    )
}

#[cfg(test)]
mod raw_material_assignment_quantity_tests {
    use super::*;

    fn assignment() -> RawMaterialAssignment {
        RawMaterialAssignment {
            order_id: "zakaz-1".to_string(),
            apparatus_id: crate::core::apparatus_standard::ApparatusId::new(
                "apparatus:test:pechat-1",
            )
            .unwrap(),
            apparatus: "Pechat".to_string(),
            barcode: "ROLL-1000".to_string(),
            item_code: "RULON".to_string(),
            item_name: "Rulon".to_string(),
            item_group: "Rulon".to_string(),
            assigned_by_role: "aparatchi".to_string(),
            assigned_by_ref: "worker-1".to_string(),
            assigned_by_display_name: "Worker".to_string(),
            assigned_at: String::new(),
        }
    }

    fn stock(status: &str, order_id: &str) -> RawMaterialStockEntry {
        RawMaterialStockEntry {
            qty: 1_000.0,
            status: status.to_string(),
            reserved_order_id: order_id.to_string(),
            ..RawMaterialStockEntry::default()
        }
    }

    #[test]
    fn quantity_ledger_preserves_received_consumed_remaining_invariant() {
        let assignment = assignment();
        assert_eq!(
            raw_material_assignment_quantities(&assignment, Some(&stock("available", ""))),
            (0.0, 0.0, 0.0)
        );
        assert_eq!(
            raw_material_assignment_quantities(&assignment, Some(&stock("in_use", "zakaz-1"))),
            (1_000.0, 0.0, 1_000.0)
        );
        assert_eq!(
            raw_material_assignment_quantities(&assignment, Some(&stock("consumed", "zakaz-1"))),
            (1_000.0, 1_000.0, 0.0)
        );
        assert_eq!(
            raw_material_assignment_quantities(&assignment, Some(&stock("in_use", "zakaz-2"))),
            (0.0, 0.0, 0.0)
        );
    }

    #[test]
    fn staged_placement_matches_canonical_id_not_display_name() {
        let placement = RawMaterialStatePlacement {
            barcode: "ROLL-1000".to_string(),
            location_id: "location:state:pechat".to_string(),
            location_name: "Pechat oldi".to_string(),
            apparatus_ids: vec!["apparatus:catalog:pechat-001".to_string()],
            apparatus: vec!["7 ta rangli pechat - A".to_string()],
        };

        assert!(state_placement_matches_apparatus(
            &placement,
            "apparatus:catalog:pechat-001"
        ));
        assert!(!state_placement_matches_apparatus(
            &placement,
            "7 ta rangli pechat - A"
        ));
    }
}

pub async fn raw_material_stock(
    State(state): State<AppState>,
    Query(query): Query<ItemQuery>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::CatalogItemRead,
            Capability::RawMaterialAssign,
        ],
    )
    .await?;
    match method {
        Method::GET => {
            let limit = optional_search_limit(query.limit.as_deref(), 200, 500);
            let warehouse = query.warehouse.as_deref().unwrap_or("");
            if principal.role == PrincipalRole::MaterialTaminotchi {
                return material_scoped_raw_material_stock(&state, &principal, warehouse, limit)
                    .await
                    .map(json_response);
            }
            state
                .gscale
                .raw_material_stock(warehouse, limit)
                .await
                .map(json_response)
                .map_err(|_| server_error("raw material stock fetch failed"))
        }
        Method::PUT => update_material_scoped_raw_material_stock(&state, &principal, &body).await,
        _ => Err(method_not_allowed()),
    }
}

#[derive(Debug, serde::Deserialize)]
struct RawMaterialStockUpdateRequest {
    #[serde(default)]
    barcode: String,
    #[serde(default)]
    item_code: String,
    #[serde(default)]
    qty: f64,
}

async fn update_material_scoped_raw_material_stock(
    state: &AppState,
    principal: &Principal,
    body: &[u8],
) -> Result<Response, AdminError> {
    if principal.role != PrincipalRole::MaterialTaminotchi {
        return Err(forbidden());
    }
    require_capability(state, principal, Capability::RawMaterialAssign).await?;
    let request: RawMaterialStockUpdateRequest = parse_json(body)?;
    let barcode = request.barcode.trim();
    let item_code = request.item_code.trim();
    if barcode.is_empty() || item_code.is_empty() {
        return Err(bad_request("raw_material_stock_update_invalid"));
    }
    let current = state
        .gscale
        .raw_material_stock_by_barcode(barcode)
        .await
        .map_err(|_| server_error("raw material stock fetch failed"))?
        .ok_or_else(|| not_found("raw_material_stock_not_found"))?;
    let warehouses = material_warehouse_scope(state, principal).await?;
    if !warehouse_in_scope(&warehouses, &current.warehouse) {
        return Err(forbidden());
    }
    let has_assignment = state
        .production_maps
        .raw_material_assignments()
        .await
        .map_err(production_map_error)?
        .iter()
        .any(|assignment| assignment.barcode.trim().eq_ignore_ascii_case(barcode));
    if has_assignment
        || !current.status.trim().eq_ignore_ascii_case("available")
        || !current.reserved_order_id.trim().is_empty()
    {
        return Err(raw_material_stock_locked_error());
    }

    let items = state
        .admin
        .items_by_codes(&[item_code.to_string()])
        .await
        .map_err(|_| server_error("raw material item fetch failed"))?;
    let selected_item = items
        .into_iter()
        .find(|item| item.code.trim().eq_ignore_ascii_case(item_code))
        .ok_or_else(|| bad_request("raw_material_item_not_found"))?;
    if selected_item.uom.trim().is_empty()
        || !selected_item
            .uom
            .trim()
            .eq_ignore_ascii_case(current.uom.trim())
    {
        return Err(bad_request("raw_material_uom_mismatch"));
    }
    let assigned_groups = state
        .admin
        .principal_assigned_item_group_scope(principal)
        .await
        .map_err(|_| server_error("item group scope fetch failed"))?;
    if selected_item.item_group.trim().is_empty()
        || !assigned_groups.iter().any(|group| {
            group
                .trim()
                .eq_ignore_ascii_case(selected_item.item_group.trim())
        })
    {
        return Err(bad_request(
            "item group is not assigned to material taminotchi",
        ));
    }
    let actor = queue_action_actor(principal);
    let item_name = selected_item.name.trim();
    let updated = state
        .gscale
        .update_raw_material_stock(RawMaterialStockUpdateInput {
            barcode: barcode.to_string(),
            item_code: selected_item.code.trim().to_string(),
            item_name: if item_name.is_empty() {
                item_code.to_string()
            } else {
                item_name.to_string()
            },
            qty: request.qty,
            actor_role: actor.role,
            actor_ref: actor.ref_,
            actor_display_name: actor.display_name,
        })
        .await
        .map_err(raw_material_stock_update_error)?;
    state
        .warehouse_events
        .notify_updated(&updated.warehouse, "raw_material_stock_corrected");
    Ok(json_response(updated))
}

fn raw_material_stock_update_error(error: crate::core::gscale::GscaleServiceError) -> AdminError {
    match error {
        crate::core::gscale::GscaleServiceError::InvalidInput(detail)
            if detail == "raw_material_stock_not_found" =>
        {
            not_found(detail)
        }
        crate::core::gscale::GscaleServiceError::InvalidInput(detail)
            if detail == "raw_material_stock_locked" =>
        {
            raw_material_stock_locked_error()
        }
        crate::core::gscale::GscaleServiceError::InvalidInput(detail) => bad_request(detail),
        _ => server_error("raw material stock update failed"),
    }
}

pub(super) fn raw_material_stock_locked_error() -> AdminError {
    (
        StatusCode::CONFLICT,
        Json(AdminErrorResponse::new("raw_material_stock_locked")),
    )
}

include!("../raw_materials_history.rs");
