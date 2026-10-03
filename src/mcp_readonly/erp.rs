//! Deliberately admin-only first slice. Narrower row/warehouse/apparatus policies
//! must be implemented before permitting non-admin principals.
use super::*;
use crate::{
    app::AppState,
    core::{authz::Capability, production_map::WipProgressBatchQuery},
};

pub struct ErpReadPort {
    pub(super) admin: crate::core::admin::service::AdminService,
    pub(super) production_maps: crate::core::production_map::ProductionMapService,
    pub(super) warehouses: crate::core::warehouses::WarehouseService,
}
impl From<&AppState> for ErpReadPort {
    fn from(state: &AppState) -> Self {
        Self {
            admin: state.admin.clone(),
            production_maps: state.production_maps.clone(),
            warehouses: state.warehouses.clone(),
        }
    }
}

pub(super) fn native_admin(principal: &crate::core::auth::models::Principal) -> bool {
    principal.role == crate::core::auth::models::PrincipalRole::Admin
}
#[async_trait]
impl ReadPort for ErpReadPort {
    async fn permitted(&self, grant: &VerifiedGrant, tool: &str) -> bool {
        // Restrict native role too: custom AdminAccess must not bypass existing warehouse scoping.
        if !native_admin(&grant.principal) {
            return false;
        }
        // Also enforce live custom-role assignments; native role alone never grants access.
        if !self
            .admin
            .principal_has_capability(&grant.principal, Capability::AdminAccess)
            .await
        {
            return false;
        }
        let capability = match tool {
            "erp_summary" | "erp_warehouse" => Capability::CatalogItemRead,
            "erp_order_status" | "erp_wip" => Capability::ApparatusQueueRead,
            _ => return false,
        };
        self.admin
            .principal_has_capability(&grant.principal, capability)
            .await
    }
    async fn read(&self, tool: &str, args: &Args) -> Result<ReadResult, ()> {
        let limit = args.limit();
        match tool {
            "erp_summary" => {
                let rows = self
                    .warehouses
                    .warehouse_summaries("", limit + 1)
                    .await
                    .map_err(|_| ())?;
                let partial = rows.len() > limit;
                let data: Vec<Value> = rows.into_iter().take(limit).map(|r| json!({"warehouse":r.warehouse,"product_count":r.product_count,"reserved_count":r.reserved_count,"unit":"count"})).collect();
                Ok(ReadResult {
                    data: json!(data),
                    partial,
                })
            }
            "erp_order_status" => {
                let order_id = args.order_id.as_deref().ok_or(())?;
                let r = self
                    .production_maps
                    .order_status_detail(order_id)
                    .await
                    .map_err(|_| ())?;
                // Explicit field whitelist; never serialize entire domain objects.
                Ok(ReadResult {
                    data: json!({"order_id":order_id,"order_status":r.order_status,"work_status":r.work_status,"flow_status":r.flow_status,"stock_status":r.stock_status,"completed_with_issue_count":r.completed_with_issue_count,"unit":"count"}),
                    partial: false,
                })
            }
            "erp_wip" => {
                let order_id = args.order_id.as_deref().ok_or(())?;
                let q = WipProgressBatchQuery::new("", "", "", None, false, order_id, limit + 1);
                let rows = self
                    .production_maps
                    .wip_progress_batches(q)
                    .await
                    .map_err(|_| ())?;
                let data: Vec<Value> = rows.into_iter().filter(|r| r.order_id == order_id).take(limit).map(|r| json!({"batch_id":r.batch_id,"order_id":r.order_id,"quantity":r.produced_qty,"unit":r.uom,"wip_status":r.wip_status,"current_location":r.current_location,"completed_at_unix":r.completed_at_unix})).collect();
                // Domain service may prefetch/filter only 500 records; cannot prove completeness.
                Ok(ReadResult {
                    data: json!(data),
                    partial: true,
                })
            }
            "erp_warehouse" => {
                let warehouse = args.warehouse.as_deref().ok_or(())?;
                let rows = self
                    .warehouses
                    .warehouse_stock_items(warehouse, "", limit + 1, 0)
                    .await
                    .map_err(|_| ())?;
                if rows
                    .iter()
                    .any(|r| !r.warehouse.eq_ignore_ascii_case(warehouse))
                {
                    return Err(());
                }
                let partial = rows.len() > limit;
                let data: Vec<Value> = rows.into_iter().take(limit).map(|r| json!({"item_code":r.code,"order_id":r.order_id,"warehouse":r.warehouse,"quantity":r.on_hand_qty,"unit":r.uom,"package_count":r.package_count})).collect();
                Ok(ReadResult {
                    data: json!(data),
                    partial,
                })
            }
            _ => Err(()),
        }
    }
}
