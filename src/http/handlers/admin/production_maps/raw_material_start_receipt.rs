use super::*;
use crate::core::inventory_movements::{
    InventoryActor, InventoryAsset, InventoryAssetKind, InventoryAssetQuery,
    InventoryDeliveryReceipt, InventoryLocationKind, InventoryRelocationCreate,
};
use crate::core::production_map::OrderControlState;

#[derive(serde::Deserialize)]
pub struct ReceiptQuery {
    order_id: String,
    apparatus: String,
    barcode: String,
}

#[derive(serde::Deserialize)]
struct ReceiptInput {
    #[serde(flatten)]
    query: ReceiptQuery,
    delivered_by_role: PrincipalRole,
    delivered_by_ref: String,
    idempotency_key: String,
}

struct ReceiptContext {
    asset: InventoryAsset,
    location_id: String,
    already_at_apparatus: bool,
    deliverers: Vec<Principal>,
}

/// An operator records physical delivery to their apparatus. The authenticated
/// operator remains the audit actor; the selected deliverer is an attribution.
pub async fn raw_material_start_receipt(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<std::collections::HashMap<String, String>>,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal =
        authorize_capability(&state, &headers, Capability::ApparatusQueueManage).await?;
    if !matches!(method, Method::GET | Method::POST) {
        return Err(method_not_allowed());
    }
    let input = if method == Method::POST {
        Some(parse_json::<ReceiptInput>(&body)?)
    } else {
        None
    };
    let request = if let Some(input) = &input {
        &input.query
    } else {
        &ReceiptQuery {
            order_id: query.get("order_id").cloned().unwrap_or_default(),
            apparatus: query.get("apparatus").cloned().unwrap_or_default(),
            barcode: query.get("barcode").cloned().unwrap_or_default(),
        }
    };
    let _guard = state.production_maps.queue_action_guard().await;
    let context = receipt_context(&state, &principal, request).await?;
    if let Some(input) = input {
        if input.idempotency_key.trim().is_empty() {
            return Err(bad_request("inventory_idempotency_key_required"));
        }
        let deliverer = context
            .deliverers
            .iter()
            .find(|candidate| {
                candidate.role == input.delivered_by_role
                    && candidate.ref_ == input.delivered_by_ref.trim()
            })
            .cloned()
            .ok_or_else(|| conflict("material_deliverer_not_allowed"))?;
        let mut actor = InventoryActor::new(
            principal.clone(),
            false,
            [context.asset.custody_warehouse.clone()],
        );
        actor.delivery_receipt = Some(InventoryDeliveryReceipt {
            order_id: input.query.order_id.trim().to_string(),
            apparatus_id: input.query.apparatus.trim().to_string(),
            asset_ref: context.asset.asset_ref.clone(),
            source_location_id: context.asset.physical_location.id.clone(),
            destination_location_id: context.location_id.clone(),
            delivered_by: deliverer,
        });
        state
            .inventory_movements
            .relocate(
                &actor,
                InventoryRelocationCreate {
                    asset_kind: InventoryAssetKind::RawMaterial,
                    asset_ref: context.asset.asset_ref.clone(),
                    physical_location_id: context.location_id,
                    note: format!(
                        "Olib keldi: {}. Qabul qildi: {}.",
                        actor
                            .delivery_receipt
                            .as_ref()
                            .unwrap()
                            .delivered_by
                            .display_name,
                        principal.display_name
                    ),
                    idempotency_key: input.idempotency_key,
                },
            )
            .await
            .map_err(|error| match error {
                crate::core::inventory_movements::InventoryMovementError::AssetUnavailable => {
                    conflict("raw_material_stock_unavailable")
                }
                crate::core::inventory_movements::InventoryMovementError::IdempotencyConflict => {
                    conflict("inventory_idempotency_conflict")
                }
                _ => server_error("material delivery receipt failed"),
            })?;
        state.warehouse_events.notify_updated(
            &context.asset.custody_warehouse,
            "material_delivery_received",
        );
        state.production_maps.notify_live();
        return Ok(json_response(
            serde_json::json!({ "ok": true, "barcode": input.query.barcode.trim(), "apparatus": input.query.apparatus.trim() }),
        ));
    }
    Ok(json_response(serde_json::json!({
        "already_at_apparatus": context.already_at_apparatus,
        "deliverers": context.deliverers.iter().map(|user| serde_json::json!({
            "role": user.role, "ref": user.ref_, "name": user.display_name,
        })).collect::<Vec<_>>(),
    })))
}

async fn receipt_context(
    state: &AppState,
    principal: &Principal,
    query: &ReceiptQuery,
) -> Result<ReceiptContext, AdminError> {
    let order_id = query.order_id.trim();
    let apparatus = query.apparatus.trim();
    let barcode = query.barcode.trim();
    if order_id.is_empty()
        || barcode.is_empty()
        || !queue_state::is_canonical_apparatus_id(apparatus)
    {
        return Err(bad_request("raw_material_invalid_input"));
    }
    if principal.role == PrincipalRole::Aparatchi {
        let assigned = state.admin.principal_assigned_apparatus(principal).await;
        if !assigned.iter().any(|id| id == apparatus) {
            return Err(production_map_error(
                ProductionMapError::ApparatusNotAssigned,
            ));
        }
    } else if !state
        .admin
        .principal_has_capability(principal, Capability::AdminAccess)
        .await
    {
        return Err(forbidden());
    }
    match state
        .production_maps
        .order_control_state(order_id)
        .await
        .map_err(production_map_error)?
        .state
    {
        OrderControlState::Frozen => return Err(conflict("order_frozen")),
        OrderControlState::FreezeRequested => return Err(conflict("order_freeze_requested")),
        _ => {}
    }
    let controls = state
        .production_maps
        .queue_action_controls_for_apparatus(apparatus)
        .await
        .map_err(production_map_error)?;
    let control = controls
        .get(apparatus)
        .and_then(|orders| orders.get(order_id))
        .ok_or_else(|| conflict("queue_action_not_allowed"))?;
    if !control
        .allowed_actions
        .contains(&queue_state::ApparatusQueueAction::Start)
        || control.interaction.start_materials_mode
            != crate::core::production_map::ApparatusQueueStartMaterialsMode::ScanRequired
    {
        return Err(conflict("queue_action_not_allowed"));
    }
    let assignments = state
        .production_maps
        .raw_material_assignments_for_order(order_id)
        .await
        .map_err(production_map_error)?;
    let assignment = assignments
        .into_iter()
        .find(|assignment| {
            assignment.apparatus_id.as_str() == apparatus
                && assignment.barcode.eq_ignore_ascii_case(barcode)
        })
        .ok_or_else(|| conflict("raw_material_mismatch"))?;
    let stock = state
        .gscale
        .raw_material_stock_by_barcode(barcode)
        .await
        .map_err(|_| server_error("raw material stock lookup failed"))?;
    match super::raw_materials::raw_material_execution_status(state, &assignment, stock.as_ref())
        .await?
    {
        "compatible" => {}
        "needs_cutting" => return Err(conflict("raw_material_needs_cutting")),
        "width_mismatch" => return Err(conflict("raw_material_width_mismatch")),
        _ => return Err(conflict("raw_material_stock_unavailable")),
    }
    // Resolve only the already-authorized, assigned barcode; do not expose
    // general warehouse inventory to the operator.
    let assets = state
        .inventory_movements
        .assets(
            &InventoryActor::new(principal.clone(), true, []),
            InventoryAssetQuery {
                query: barcode.to_string(),
                asset_kind: Some(InventoryAssetKind::RawMaterial),
                limit: 50,
                ..Default::default()
            },
        )
        .await
        .map_err(|_| server_error("material inventory lookup failed"))?;
    let asset = assets
        .into_iter()
        .find(|asset| asset.identifier.eq_ignore_ascii_case(barcode))
        .ok_or_else(|| conflict("raw_material_stock_unavailable"))?;
    if asset.status != "available" || asset.qty <= 0.0 || !asset.transfer_id.is_empty() {
        return Err(conflict("raw_material_stock_unavailable"));
    }
    let locations = state
        .inventory_movements
        .locations()
        .await
        .map_err(|_| server_error("inventory locations failed"))?;
    let mut destinations = locations
        .into_iter()
        .filter(|location| {
            location.active
                && location.kind == InventoryLocationKind::State
                && location
                    .apparatus
                    .iter()
                    .any(|candidate| candidate.id == apparatus)
        })
        .collect::<Vec<_>>();
    destinations.sort_by(|a, b| a.id.cmp(&b.id));
    let already_at_apparatus = destinations
        .iter()
        .any(|location| location.id == asset.physical_location.id);
    let location_id = destinations
        .into_iter()
        .next()
        .ok_or_else(|| conflict("material_delivery_location_missing"))?
        .id;
    let assignments = state
        .warehouses
        .warehouse_assignments(&asset.custody_warehouse)
        .await
        .map_err(|_| server_error("material deliverer scope failed"))?;
    let mut deliverers = Vec::new();
    for assignment in assignments {
        if assignment.principal_ref.trim().is_empty() {
            continue;
        }
        let candidate = Principal {
            role: assignment.principal_role,
            ref_: assignment.principal_ref.trim().to_string(),
            display_name: assignment.display_name,
            legal_name: String::new(),
            phone: String::new(),
            avatar_url: String::new(),
        };
        if !state
            .admin
            .principal_is_active(&candidate)
            .await
            .map_err(|_| server_error("material deliverer state failed"))?
        {
            continue;
        }
        if !state
            .admin
            .principal_has_capability(&candidate, Capability::InventoryMovementManage)
            .await
        {
            continue;
        }
        let exists = if candidate.role == PrincipalRole::Aparatchi {
            !state
                .workers
                .workers_by_ids(&[candidate.ref_.clone()])
                .await
                .map_err(|_| server_error("material deliverer lookup failed"))?
                .is_empty()
        } else if matches!(
            candidate.role,
            PrincipalRole::Werka | PrincipalRole::MaterialTaminotchi
        ) {
            state
                .admin
                .user_list_entry_for_principal(&candidate.role, &candidate.ref_)
                .await
                .map_err(|_| server_error("material deliverer lookup failed"))?
                .is_some_and(|user| !user.blocked && user.status == "active")
        } else {
            !state
                .system_users
                .users_by_ids(&[candidate.ref_.clone()])
                .await
                .map_err(|_| server_error("material deliverer lookup failed"))?
                .iter()
                .all(|user| user.role != candidate.role)
        };
        if exists
            && !deliverers
                .iter()
                .any(|user: &Principal| user.role == candidate.role && user.ref_ == candidate.ref_)
        {
            deliverers.push(candidate);
        }
    }
    deliverers.sort_by(|a, b| {
        a.display_name
            .cmp(&b.display_name)
            .then_with(|| a.ref_.cmp(&b.ref_))
    });
    Ok(ReceiptContext {
        asset,
        location_id,
        already_at_apparatus,
        deliverers,
    })
}
