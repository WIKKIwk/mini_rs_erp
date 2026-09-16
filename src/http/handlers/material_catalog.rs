use crate::app::AppState;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::gscale::models::MaterialReceiptPrintRequest;
use crate::core::rps_batch::{RpsBatchStartRequest, RpsBatchUpdateRequest};
use crate::core::werka::models::SupplierItem;

pub(super) const ROLL_MATERIAL_ITEM_GROUP: &str = "Rulon";
const ROLL_MATERIAL_ITEM_GROUP_ALIASES: &[&str] = &[ROLL_MATERIAL_ITEM_GROUP, "Rulon materiallari"];

pub(super) fn roll_material_item_group_roots() -> Vec<String> {
    ROLL_MATERIAL_ITEM_GROUP_ALIASES
        .iter()
        .map(|group| (*group).to_string())
        .collect()
}

pub(super) enum MaterialCatalogError {
    OrderAssignment(String),
    ReadFailed,
    ItemNotFound,
    Forbidden,
    DimensionsRequired,
}

pub(super) async fn normalize_material_batch_item(
    state: &AppState,
    principal: &Principal,
    request: &mut RpsBatchStartRequest,
) -> Result<(), MaterialCatalogError> {
    if let Some(item) = authorized_material_item(state, principal, &request.item_code).await? {
        require_dimensions(state, &item, request.width_mm, request.micron).await?;
        request.item_code = item.code;
        request.item_name = item.name;
    }
    request.order_assignment = receipt_order_assignment(state, principal, &request.order_id, &request.apparatus,
        &request.item_code, &request.warehouse, request.width_mm, request.micron).await?;
    Ok(())
}

pub(super) async fn normalize_material_batch_update_item(
    state: &AppState,
    principal: &Principal,
    request: &mut RpsBatchUpdateRequest,
) -> Result<(), MaterialCatalogError> {
    if let Some(item) = authorized_material_item(state, principal, &request.item_code).await? {
        require_dimensions(state, &item, request.width_mm, request.micron).await?;
        request.item_code = item.code;
        request.item_name = item.name;
    }
    // Older clients omit order fields during batch edits. Never silently erase
    // the server-owned commitment in that case.
    if request.order_id.trim().is_empty() {
        let current = state.rps_batch.state(principal).await.map_err(|_| MaterialCatalogError::ReadFailed)?;
        if current.batch.id == request.batch_id {
            if let Some(link) = current.batch.order_assignment {
                request.order_id = link["order_id"].as_str().unwrap_or_default().into();
                request.apparatus = link["apparatus"].as_str().unwrap_or_default().into();
            }
        }
    }
    request.order_assignment = receipt_order_assignment(state, principal, &request.order_id, &request.apparatus,
        &request.item_code, &request.warehouse, request.width_mm, request.micron).await?;
    Ok(())
}

pub(super) async fn normalize_material_receipt_item(
    state: &AppState,
    principal: &Principal,
    request: &mut MaterialReceiptPrintRequest,
) -> Result<(), MaterialCatalogError> {
    if let Some(item) = authorized_material_item(state, principal, &request.item_code).await? {
        require_dimensions(state, &item, request.width_mm, request.micron).await?;
        request.item_code = item.code;
        request.item_name = item.name;
    }
    request.order_assignment = receipt_order_assignment(state, principal, &request.order_id, &request.apparatus,
        &request.item_code, &request.warehouse, request.width_mm, request.micron).await?;
    Ok(())
}

async fn receipt_order_assignment(
    state: &AppState, principal: &Principal, order_id: &str, apparatus: &str,
    item_code: &str, warehouse: &str, width_mm: Option<f64>, micron: Option<f64>,
) -> Result<Option<serde_json::Value>, MaterialCatalogError> {
    super::admin::validate_receipt_order_assignment(state, principal, order_id, apparatus, item_code, warehouse, width_mm, micron)
        .await.map_err(|(_, axum::Json(error))| MaterialCatalogError::OrderAssignment(error.error))
}

async fn authorized_material_item(
    state: &AppState,
    principal: &Principal,
    item_code: &str,
) -> Result<Option<SupplierItem>, MaterialCatalogError> {
    if principal.role == PrincipalRole::TayyorlovMasteri {
        // GScale/Tarozi katalogi backend scope'idan kelgan itemlargina RPS va
        // print oqimlariga kiradi. Preparation store yo'q konfiguratsiyada
        // avvalgi test/dev fallback saqlanadi.
        let Some(store) = state.preparation.as_ref() else {
            return Ok(None);
        };
        let item = store
            .assigned_rulon_item(&principal.ref_, item_code)
            .await
            .map_err(|_| MaterialCatalogError::ReadFailed)?
            .ok_or(MaterialCatalogError::Forbidden)?;
        return Ok(Some(item));
    }
    if principal.role != PrincipalRole::MaterialTaminotchi {
        return Ok(None);
    }
    let item_code = item_code.trim().to_string();
    let item = state
        .admin
        .items_by_codes(std::slice::from_ref(&item_code))
        .await
        .map_err(|_| MaterialCatalogError::ReadFailed)?
        .into_iter()
        .find(|item| item.code.trim().eq_ignore_ascii_case(&item_code))
        .ok_or(MaterialCatalogError::ItemNotFound)?;
    let assigned_groups = state
        .admin
        .principal_assigned_item_group_scope(principal)
        .await
        .map_err(|_| MaterialCatalogError::ReadFailed)?;
    if !assigned_groups
        .iter()
        .any(|group| group.trim().eq_ignore_ascii_case(item.item_group.trim()))
    {
        return Err(MaterialCatalogError::Forbidden);
    }
    Ok(Some(item))
}

async fn require_dimensions(
    state: &AppState,
    item: &SupplierItem,
    width_mm: Option<f64>,
    micron: Option<f64>,
) -> Result<(), MaterialCatalogError> {
    let dimension_groups = state
        .admin
        .item_group_scope(roll_material_item_group_roots())
        .await
        .map_err(|_| MaterialCatalogError::ReadFailed)?;
    if requires_material_dimensions(item, &dimension_groups)
        && !matches!(
            (width_mm, micron),
            (Some(width_mm), Some(micron))
                if width_mm.is_finite()
                    && width_mm > 0.0
                    && micron.is_finite()
                    && micron > 0.0
        )
    {
        return Err(MaterialCatalogError::DimensionsRequired);
    }
    Ok(())
}

pub(super) fn requires_material_dimensions(
    item: &SupplierItem,
    dimension_groups: &[String],
) -> bool {
    let group = item.item_group.trim();
    dimension_groups
        .iter()
        .any(|candidate| candidate.trim().eq_ignore_ascii_case(group))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(group: &str) -> SupplierItem {
        SupplierItem {
            code: "ITEM".to_string(),
            name: "Item".to_string(),
            uom: "Kg".to_string(),
            warehouse: String::new(),
            item_group: group.to_string(),
            customer_names: Vec::new(),
        }
    }

    #[test]
    fn material_dimensions_follow_only_rulon_group_scope() {
        let dimension_groups = vec!["Rulon".to_string(), "Rulon eni".to_string()];

        assert!(requires_material_dimensions(
            &item("Rulon"),
            &dimension_groups
        ));
        assert!(requires_material_dimensions(
            &item("Rulon eni"),
            &dimension_groups
        ));
        assert!(!requires_material_dimensions(
            &item("Homashyo"),
            &dimension_groups
        ));
        assert!(!requires_material_dimensions(
            &item("Kraska"),
            &dimension_groups
        ));
    }
}
