use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use axum::http::StatusCode;
use tower::ServiceExt;

use crate::core::admin::service::AdminService;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::authz::RoleAssignmentUpsert;
use crate::core::calculate_orders::CalculateOrderTemplate;
use crate::core::mini_orders::{MiniOrderError, MiniOrderSink, OrderMaterial};
use crate::core::production_map::{
    MemoryProductionMapStore, ProductionMapDefinition, ProductionMapService,
    ProductionMapStorePort, TestCanonicalApparatusResolver,
};
use crate::core::werka::models::SupplierItem;
use crate::http::router::build_router;

use super::support::*;

#[derive(Default)]
struct OrderMaterials(AtomicUsize);

#[async_trait]
impl MiniOrderSink for OrderMaterials {
    async fn order_materials(&self, order_id: &str) -> Result<Vec<OrderMaterial>, MiniOrderError> {
        self.0.fetch_add(1, Ordering::Relaxed);
        let rows = match order_id {
            "zakaz-9001" => vec![
                ("HIDDEN", "Hidden material", "Kraska", vec![20.0]),
                ("ORDER-PE", "PE oq", "Rulon", vec![30.0]),
                ("ROLL-1000", "PET", "Rulon eni", vec![12.0, 20.0]),
            ],
            "zakaz-9002" => vec![("ORDER-PE", "PE oq", "Rulon", vec![50.0])],
            _ => Vec::new(),
        };
        Ok(rows
            .into_iter()
            .map(|(code, name, group, microns)| OrderMaterial {
                item: SupplierItem {
                    code: code.into(),
                    name: name.into(),
                    item_group: group.into(),
                    uom: "Kg".into(),
                    ..Default::default()
                },
                microns,
            })
            .collect())
    }

    async fn save_order(
        &self,
        _: &ProductionMapDefinition,
        _: &CalculateOrderTemplate,
    ) -> Result<(), MiniOrderError> {
        unreachable!("catalog reads must not write orders")
    }
}

async fn fixture() -> (crate::app::AppState, Arc<OrderMaterials>, Principal) {
    let mut state = test_state();
    state.admin =
        AdminService::new(&state.config).with_read_port(Arc::new(FakeAdminCatalogReadPort));
    state
        .admin
        .upsert_role_assignment(RoleAssignmentUpsert {
            principal_role: PrincipalRole::MaterialTaminotchi,
            principal_ref: "order-material-test".into(),
            role_id: "material_taminotchi".into(),
            assigned_apparatus: Vec::new(),
            assigned_item_groups: vec!["Rulon".into()],
        })
        .await
        .unwrap();
    let sink = Arc::new(OrderMaterials::default());
    state.production_orders = sink.clone();
    let store = Arc::new(MemoryProductionMapStore::new());
    for id in ["zakaz-9001", "zakaz-9002"] {
        store
            .put_map(
                serde_json::from_value(serde_json::json!({
                    "id": id, "product_code": "PRODUCT", "title": "Order",
                }))
                .unwrap(),
            )
            .await
            .unwrap();
    }
    state.production_maps =
        ProductionMapService::new(store, Arc::new(TestCanonicalApparatusResolver::standard()));
    let principal = Principal {
        role: PrincipalRole::MaterialTaminotchi,
        ref_: "order-material-test".into(),
        display_name: "Materialchi".into(),
        legal_name: String::new(),
        phone: String::new(),
        avatar_url: String::new(),
    };
    (state, sink, principal)
}

#[tokio::test]
async fn picker_reads_only_selected_order_materials_before_search_and_pagination() {
    let (state, sink, principal) = fixture().await;
    let token = state.sessions.create(principal).await.unwrap();
    let router = build_router(state);
    for (query, expected) in [
        ("order_id=zakaz-9001", vec!["ORDER-PE", "ROLL-1000"]),
        ("order_id=zakaz-9001&limit=1&offset=1", vec!["ROLL-1000"]),
        ("order_id=zakaz-9001&q=PET&limit=1", vec!["ROLL-1000"]),
        ("order_id=zakaz-9001&group=Kraska", vec![]),
        ("order_id=zakaz-9002", vec!["ORDER-PE"]),
        ("order_id=zakaz-missing", vec![]),
        ("", vec!["ROLL-1000"]),
    ] {
        let before = sink.0.load(Ordering::Relaxed);
        let response = router
            .clone()
            .oneshot(request(
                "GET",
                &format!("/v1/mobile/gscale/items?{query}"),
                &token,
                "",
            ))
            .await
            .unwrap();
        let status = response.status();
        let body = json_body(response).await;
        assert_eq!(status, StatusCode::OK, "{query}: {body}");
        let items = body.as_array().unwrap();
        assert_eq!(
            items
                .iter()
                .map(|item| item["code"].as_str().unwrap())
                .collect::<Vec<_>>(),
            expected,
            "{query}"
        );
        assert_eq!(
            sink.0.load(Ordering::Relaxed) - before,
            usize::from(!query.is_empty())
        );
        for item in items {
            if query.starts_with("order_id=zakaz-9001") && item["code"] == "ROLL-1000" {
                assert_eq!(item["order_microns"], serde_json::json!([12.0, 20.0]));
                assert_eq!(item["requires_dimensions"], true);
            }
            if query == "order_id=zakaz-9002" {
                assert_eq!(item["order_microns"], serde_json::json!([50.0]));
            }
        }
    }
}

#[tokio::test]
async fn receipt_rejects_materials_and_microns_outside_selected_order() {
    let (state, _, principal) = fixture().await;
    assign_warehouse_to_principal(&state, principal.role, &principal.ref_, "Stores - A").await;
    let token = state.sessions.create(principal).await.unwrap();
    let router = build_router(state);
    for (order, micron, expected) in [
        ("zakaz-9002", 35.0, "Homashyo order qavatlariga mos emas"),
        ("zakaz-9001", 99.0, "Mikron order qavatlariga mos emas"),
    ] {
        let body = serde_json::json!({
            "order_id": order, "item_code": "ROLL-1000", "warehouse": "Stores - A",
            "width_mm": 1000.0, "micron": micron,
        });
        let response = router
            .clone()
            .oneshot(request(
                "POST",
                "/v1/mobile/gscale/material-receipt/print",
                &token,
                &body.to_string(),
            ))
            .await
            .unwrap();
        let status = response.status();
        let body = json_body(response).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"], "order_material_assignment_invalid");
        assert_eq!(body["detail"], expected);
    }
}
