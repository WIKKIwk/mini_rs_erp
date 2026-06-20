use axum::Router;
use axum::routing::{any, get};

use crate::app::AppState;
use crate::http::handlers::admin;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/mobile/admin/settings", any(admin::settings))
        .route(
            "/v1/mobile/admin/apparatus-groups",
            any(admin::apparatus_groups),
        )
        .route("/v1/mobile/admin/capabilities", any(admin::capabilities))
        .route("/v1/mobile/admin/roles", any(admin::roles))
        .route("/v1/mobile/admin/workers", any(admin::workers))
        .route("/v1/mobile/admin/workers/detail", any(admin::worker_detail))
        .route(
            "/v1/mobile/admin/workers/code/regenerate",
            any(admin::worker_code_regenerate),
        )
        .route("/v1/mobile/admin/worker-groups", any(admin::worker_groups))
        .route(
            "/v1/mobile/admin/production-maps",
            any(admin::production_maps),
        )
        .route(
            "/v1/mobile/admin/production-maps/run",
            any(admin::production_map_run),
        )
        .route(
            "/v1/mobile/admin/production-maps/with-order",
            any(admin::production_map_save_with_order),
        )
        .route(
            "/v1/mobile/admin/production-maps/move",
            any(admin::production_map_move),
        )
        .route(
            "/v1/mobile/admin/production-maps/move-batch",
            any(admin::production_map_move_batch),
        )
        .route(
            "/v1/mobile/admin/production-maps/sequence",
            any(admin::production_map_sequence),
        )
        .route(
            "/v1/mobile/admin/production-maps/queue-policies",
            any(admin::production_map_queue_policies),
        )
        .route(
            "/v1/mobile/admin/production-maps/live",
            any(admin::production_map_live),
        )
        .route(
            "/v1/mobile/admin/production-maps/completed-orders",
            any(admin::production_map_completed_orders),
        )
        .route(
            "/v1/mobile/admin/production-maps/completion-requests",
            any(admin::production_map_completion_requests),
        )
        .route(
            "/v1/mobile/admin/production-maps/completion-requests/decision",
            any(admin::production_map_completion_request_decision),
        )
        .route(
            "/v1/mobile/admin/production-maps/completion-request-decisions",
            any(admin::production_map_completion_request_decisions),
        )
        .route(
            "/v1/mobile/admin/production-maps/closed-orders",
            any(admin::production_map_closed_orders),
        )
        .route(
            "/v1/mobile/admin/production-maps/queue-action",
            any(admin::production_map_queue_action),
        )
        .route(
            "/v1/mobile/admin/production-maps/progress-qr/lookup",
            any(admin::production_map_progress_qr_lookup),
        )
        .route(
            "/v1/mobile/admin/raw-material-rules",
            any(admin::raw_material_rules),
        )
        .route(
            "/v1/mobile/admin/raw-material-assignments/lookup",
            any(admin::raw_material_assignment_lookup),
        )
        .route(
            "/v1/mobile/admin/raw-material-assignments",
            any(admin::raw_material_assignments),
        )
        .route(
            "/v1/mobile/admin/raw-material-stock",
            any(admin::raw_material_stock),
        )
        .route(
            "/v1/mobile/admin/role-assignments",
            any(admin::role_assignments),
        )
        .route("/v1/mobile/admin/suppliers", any(admin::suppliers))
        .route("/v1/mobile/admin/users/list", any(admin::user_list))
        .route("/v1/mobile/admin/suppliers/list", any(admin::supplier_list))
        .route(
            "/v1/mobile/admin/suppliers/summary",
            any(admin::supplier_summary),
        )
        .route(
            "/v1/mobile/admin/suppliers/detail",
            any(admin::supplier_detail),
        )
        .route(
            "/v1/mobile/admin/suppliers/inactive",
            any(admin::inactive_suppliers),
        )
        .route(
            "/v1/mobile/admin/suppliers/items/assigned",
            any(admin::assigned_supplier_items),
        )
        .route(
            "/v1/mobile/admin/suppliers/status",
            any(admin::supplier_status),
        )
        .route(
            "/v1/mobile/admin/suppliers/phone",
            any(admin::supplier_phone),
        )
        .route(
            "/v1/mobile/admin/suppliers/items",
            any(admin::supplier_items),
        )
        .route(
            "/v1/mobile/admin/suppliers/items/add",
            any(admin::supplier_item_add),
        )
        .route(
            "/v1/mobile/admin/suppliers/items/remove",
            any(admin::supplier_item_remove),
        )
        .route(
            "/v1/mobile/admin/suppliers/code/regenerate",
            any(admin::supplier_code_regenerate),
        )
        .route(
            "/v1/mobile/admin/suppliers/remove",
            any(admin::supplier_remove),
        )
        .route(
            "/v1/mobile/admin/suppliers/restore",
            any(admin::supplier_restore),
        )
        .route("/v1/mobile/admin/customers", any(admin::customers))
        .route("/v1/mobile/admin/customers/list", any(admin::customer_list))
        .route(
            "/v1/mobile/admin/customers/detail",
            any(admin::customer_detail),
        )
        .route(
            "/v1/mobile/admin/customers/phone",
            any(admin::customer_phone),
        )
        .route(
            "/v1/mobile/admin/customers/code/regenerate",
            any(admin::customer_code_regenerate),
        )
        .route(
            "/v1/mobile/admin/customers/items/add",
            any(admin::customer_item_add),
        )
        .route(
            "/v1/mobile/admin/customers/items/remove",
            any(admin::customer_item_remove),
        )
        .route(
            "/v1/mobile/admin/customers/remove",
            any(admin::customer_remove),
        )
        .route("/v1/mobile/admin/items", any(admin::items))
        .route("/v1/mobile/admin/apparatus", any(admin::apparatus_create))
        .route("/v1/mobile/admin/warehouses", any(admin::warehouses))
        .route(
            "/v1/mobile/admin/warehouses/live",
            get(admin::warehouse_live),
        )
        .route(
            "/v1/mobile/admin/warehouses/summary",
            any(admin::warehouse_summaries),
        )
        .route(
            "/v1/mobile/admin/warehouses/assignments",
            any(admin::warehouse_assignments),
        )
        .route(
            "/v1/mobile/admin/items/bulk-move-group",
            any(admin::items_bulk_move_group),
        )
        .route(
            "/v1/mobile/admin/item-groups/tree",
            any(admin::item_group_tree),
        )
        .route("/v1/mobile/admin/item-groups", any(admin::item_groups))
        .route("/v1/mobile/admin/activity", any(admin::activity))
        .route(
            "/v1/mobile/admin/werka/code/regenerate",
            any(admin::werka_code_regenerate),
        )
}
