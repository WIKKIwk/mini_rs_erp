use super::*;
use crate::core::order_edit::{OrderEditError, OrderEditSource};
use crate::core::production_map::{ProductionMapDefinition, QueueActionActor};

struct EditSink {
    source: Mutex<OrderEditSource>,
    blocked: std::sync::atomic::AtomicBool,
    saves: AtomicUsize,
    storage_failure: std::sync::atomic::AtomicBool,
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
    async fn order_calculation(&self, id: &str) -> Result<CalculateOrderTemplate, OrderEditError> {
        let source = self.source.lock().await;
        if source.map.id != id {
            return Err(OrderEditError::NotFound);
        }
        Ok(source.template.clone())
    }
    async fn save_order_edit(
        &self,
        original: &OrderEditSource,
        map: &ProductionMapDefinition,
        template: &CalculateOrderTemplate,
        actor: &QueueActionActor,
    ) -> Result<OrderEditSource, OrderEditError> {
        if self.storage_failure.load(Ordering::SeqCst) {
            return Err(OrderEditError::Storage {
                code: "order_edit_database_permission",
                message: "Serverning baza hisobi tarix yozuvlarini himoyalash amalini bajara olmadi. Administrator baza sozlamalarini tekshirishi kerak.".into(),
            });
        }
        let mut source = self.source.lock().await;
        if source.revision != original.revision {
            return Err(OrderEditError::Conflict);
        }
        assert_eq!(actor.role, "admin");
        *source = OrderEditSource {
            map: map.clone(),
            template: template.clone(),
            revision: source.revision + 1,
            restrictions: source.restrictions.clone(),
        };
        self.saves.fetch_add(1, Ordering::SeqCst);
        Ok(source.clone())
    }
}

#[tokio::test]
async fn order_summary_reads_saved_calculation_when_editing_is_blocked() {
    let mut state = test_state();
    let template = CalculateOrderTemplate {
        source_map_id: "zakaz-1234".into(),
        order_number: "1234".into(),
        kg: 400.0,
        frame_count: 3.0,
        layers: vec![
            crate::core::formula::LayerInput::new("PET", "12"),
            crate::core::formula::LayerInput::new("PE", "40"),
        ],
        ..Default::default()
    };
    let sink = Arc::new(EditSink {
        source: Mutex::new(OrderEditSource {
            map: serde_json::from_value(serde_json::json!({
                "id": "zakaz-1234", "product_code": "item", "title": "Product",
                "order_number": "1234"
            })).unwrap(),
            template: template.clone(),
            revision: 7,
            restrictions: Default::default(),
        }),
        blocked: std::sync::atomic::AtomicBool::new(true),
        saves: AtomicUsize::new(0),
        storage_failure: std::sync::atomic::AtomicBool::new(false),
    });
    state.production_orders = sink.clone();
    let token = session(&state, PrincipalRole::Admin).await;
    let worker = session(&state, PrincipalRole::Aparatchi).await;
    let app = build_router(state);
    let path = "/v1/mobile/admin/production-maps/order-summary?id=zakaz-1234";
    for (credential, expected) in [
        (worker.as_str(), StatusCode::FORBIDDEN),
        ("invalid", StatusCode::UNAUTHORIZED),
    ] {
        let response = app.clone().oneshot(request_with_body("GET", path, credential, "")).await.unwrap();
        assert_eq!(response.status(), expected);
    }
    let response = app.clone().oneshot(request_with_body("GET", path, &token, "")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["template"], serde_json::to_value(template).unwrap());
    let response = app.clone().oneshot(request_with_body("PUT", path, &token, "{}")).await.unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    let response = app.oneshot(request_with_body("GET", "/v1/mobile/admin/production-maps/order-summary?id=missing", &token, "")).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(sink.saves.load(Ordering::SeqCst), 0);
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
        layers: vec![crate::core::formula::LayerInput::new("pet", "12"), crate::core::formula::LayerInput::new("pe", "40")],
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
            restrictions: Default::default(),
        }),
        blocked: std::sync::atomic::AtomicBool::new(false),
        saves: AtomicUsize::new(0),
        storage_failure: std::sync::atomic::AtomicBool::new(false),
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
    sink.storage_failure.store(true, Ordering::SeqCst);
    let response = app.clone().oneshot(request_with_body("PUT", path, &token, &body)).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let failure = json_body(response).await;
    assert_eq!(failure["error"], "order_edit_database_permission");
    assert!(failure["message"].as_str().unwrap().contains("tarix yozuvlarini himoyalash"));
    assert_eq!(sink.saves.load(Ordering::SeqCst), 0);
    sink.storage_failure.store(false, Ordering::SeqCst);
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

    // Material may be attached after GET. Forging/omitting the echoed policy
    // cannot authorize an edit to a now-protected layer.
    sink.source.lock().await.restrictions =
        crate::core::order_edit::OrderEditRestrictions::for_materials(
            &saved.template, &[vec!["PET".into()]],
        );
    let response = app.clone().oneshot(request_with_body("GET", path, &token, "")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["restrictions"]["locked_layer_indices"], serde_json::json!([0]));
    let mut forbidden = saved.template.clone();
    forbidden.layers[0].micron = "35".into();
    let response = app.clone().oneshot(request_with_body("PUT", path, &token,
        &serde_json::json!({"original": saved, "template": forbidden}).to_string())).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(json_body(response).await["error"].as_str().unwrap().contains("1-qatlam"));
    assert_eq!(sink.saves.load(Ordering::SeqCst), 1);
    let mut allowed = saved.template.clone();
    allowed.kg = 650.0;
    allowed.layers[1].micron = "50".into();
    let response = app.clone().oneshot(request_with_body("PUT", path, &token,
        &serde_json::json!({"original": saved, "template": allowed}).to_string())).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK, "{}", json_body(response).await);
    assert_eq!(sink.saves.load(Ordering::SeqCst), 2);
    let changed_layer = sink.source.lock().await.clone();
    assert_eq!(changed_layer.template.layers[0], saved.template.layers[0]);
    assert_eq!(changed_layer.template.layers[1].micron, "50");
    assert_ne!(changed_layer.map.base_length, saved.map.base_length);

    let mut bypass = map.clone();
    bypass.order_kg = Some(700.0);
    assert_eq!(
        state.production_maps.upsert_map(bypass).await.unwrap_err(),
        crate::core::production_map::ProductionMapError::OpenedOrderCalculationLocked
    );
}
