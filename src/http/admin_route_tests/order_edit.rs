use super::*;
use crate::core::order_edit::{OrderEditError, OrderEditSource};
use crate::core::production_map::{ProductionMapDefinition, QueueActionActor};

struct EditSink {
    source: Mutex<OrderEditSource>,
    blocked: std::sync::atomic::AtomicBool,
    saves: AtomicUsize,
}

#[async_trait]
impl MiniOrderSink for EditSink {
    async fn save_order(
        &self,
        _: &ProductionMapDefinition,
        _: &CalculateOrderTemplate,
    ) -> Result<(), MiniOrderError> {
        panic!("edit must never create an order");
    }
    async fn order_edit_source(&self, id: &str) -> Result<OrderEditSource, OrderEditError> {
        if self.blocked.load(Ordering::SeqCst) {
            return Err(OrderEditError::Locked("Buyurtmada harakat bor"));
        }
        let source = self.source.lock().await;
        if source.map.id != id {
            return Err(OrderEditError::NotFound);
        }
        Ok(source.clone())
    }
    async fn save_order_edit(
        &self,
        original: &OrderEditSource,
        map: &ProductionMapDefinition,
        template: &CalculateOrderTemplate,
        actor: &QueueActionActor,
    ) -> Result<OrderEditSource, OrderEditError> {
        let mut source = self.source.lock().await;
        if source.revision != original.revision {
            return Err(OrderEditError::Conflict);
        }
        assert_eq!(actor.role, "admin");
        *source = OrderEditSource {
            map: map.clone(),
            template: template.clone(),
            revision: source.revision + 1,
        };
        self.saves.fetch_add(1, Ordering::SeqCst);
        Ok(source.clone())
    }
}

#[tokio::test]
async fn order_edit_route_authorization_stale_form_and_existing_order_update() {
    let mut state = test_state();
    let template = CalculateOrderTemplate {
        id: "quick-1".into(),
        name: "Order".into(),
        product: "Product".into(),
        item_code: "item".into(),
        order_number: "1234".into(),
        source_map_id: "zakaz-1234".into(),
        status: "rulon".into(),
        kg: 500.0,
        frame_product_size_mm: 300.0,
        frame_count: 2.0,
        roll_count: Some(6),
        layers: vec![crate::core::formula::LayerInput::new("pet", "12")],
        ..Default::default()
    };
    let result = crate::core::formula::calculate_with_material_catalog(
        crate::core::formula::CalculateRequest {
            kg: Some(500.0),
            frame_product_size_mm: Some(300.0),
            frame_count: Some(2.0),
            roll_count: Some(6),
            edge_allowance_mm: Some(template.edge_allowance_mm),
            waste_percent: Some(template.waste_percent),
            layers: template.layers.clone(),
            ..Default::default()
        },
        &state.calculate_materials.list().await.unwrap(),
    )
    .unwrap();
    let map: ProductionMapDefinition = serde_json::from_value(serde_json::json!({
        "id":"zakaz-1234", "product_code":"item", "title":"Product", "code":"1234", "order_number":"1234",
        "width_mm":result.width_mm, "order_kg":500.0, "roll_count":6, "base_length":result.results[0].rounded_length,
        "nodes":[{"id":"start","kind":"start","title":"Start"},
            {"id":"cut","kind":"apparatus","title":"Cut","apparatus_id":"apparatus:default:asset-011", "rezka_kadr_count":2},
            {"id":"end","kind":"end","title":"End"}],
        "edges":[{"from":"start","to":"cut"},{"from":"cut","to":"end"}]
    })).unwrap();
    // Use the actual canonical cutter id from this test fixture, not its label.
    let mut map = map;
    let cutter = state
        .apparatus
        .list_runtime_configurations()
        .await
        .unwrap()
        .into_iter()
        .find(|a| {
            a.runtime.execution_profile.operation
                == crate::core::apparatus_standard::ExecutionOperation::Cut
        })
        .unwrap();
    map.nodes[1].apparatus_id = cutter.runtime.apparatus_id.to_string();
    state.production_maps.upsert_map(map.clone()).await.unwrap();
    let sink = Arc::new(EditSink {
        source: Mutex::new(OrderEditSource {
            map: map.clone(),
            template: template.clone(),
            revision: 0,
        }),
        blocked: std::sync::atomic::AtomicBool::new(false),
        saves: AtomicUsize::new(0),
    });
    state.production_orders = sink.clone();
    let token = session(&state, PrincipalRole::Admin).await;
    let worker = session(&state, PrincipalRole::Aparatchi).await;
    let app = build_router(state.clone());
    let path = "/v1/mobile/admin/production-maps/order-edit?id=zakaz-1234";
    let original = sink.source.lock().await.clone();
    let mut changed = template.clone();
    changed.kg = 600.0;
    let body = serde_json::json!({"original": original, "template":changed}).to_string();
    for method in ["GET", "PUT"] {
        let response = app
            .clone()
            .oneshot(request_with_body(method, path, &worker, &body))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let response = app
            .clone()
            .oneshot(request_with_body(method, path, "invalid", &body))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let response = app
        .clone()
        .oneshot(request_with_body("GET", path, &token, ""))
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{}",
        json_body(response).await
    );
    sink.blocked.store(true, Ordering::SeqCst);
    let response = app
        .clone()
        .oneshot(request_with_body("PUT", path, &token, &body))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(response).await["error"], "Buyurtmada harakat bor");
    sink.blocked.store(false, Ordering::SeqCst);
    let response = app
        .clone()
        .oneshot(request_with_body("PUT", path, &token, &body))
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{}",
        json_body(response).await
    );
    let saved = sink.source.lock().await.clone();
    assert_eq!(saved.map.id, map.id);
    assert_eq!(saved.map.order_number, map.order_number);
    assert_eq!(saved.map.order_kg, Some(600.0));
    assert_eq!(saved.template.kg, 600.0);
    assert!(saved.map.base_length > map.base_length);
    assert_eq!(saved.map.edges, map.edges);
    assert_eq!(saved.revision, 1);
    let response = app
        .clone()
        .oneshot(request_with_body("PUT", path, &token, &body))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(response).await["error"],
        OrderEditError::Conflict.to_string()
    );
    assert_eq!(sink.saves.load(Ordering::SeqCst), 1);
    assert!(state.calculate_orders.list_all().await.unwrap().is_empty());
    let mut bypass = map.clone();
    bypass.order_kg = Some(700.0);
    assert_eq!(
        state.production_maps.upsert_map(bypass).await.unwrap_err(),
        crate::core::production_map::ProductionMapError::OpenedOrderCalculationLocked
    );
}
