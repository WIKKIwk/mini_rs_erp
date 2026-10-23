use super::*;
use mini_rs_erp::core::calculate_orders::{CalculateOrderImage, CalculateOrderTemplate};
use mini_rs_erp::core::pending_orders::{
    PendingOrder, PendingOrderCompletion, PendingOrderError, PendingOrderStore,
};

const PROJECTION: &str = "orders_list_v1";
const QUERY: &str = "view=orders&projection=orders_list_v1";

#[derive(Default)]
struct Intake(tokio::sync::Mutex<Vec<PendingOrder>>);
#[async_trait::async_trait]
impl PendingOrderStore for Intake {
    async fn list(&self) -> Result<Vec<PendingOrder>, PendingOrderError> {
        Ok(self.0.lock().await.clone())
    }
    async fn get(&self, id: &str) -> Result<PendingOrder, PendingOrderError> {
        self.0
            .lock()
            .await
            .iter()
            .find(|order| order.id == id)
            .cloned()
            .ok_or(PendingOrderError::NotFound)
    }
    async fn create(
        &self,
        _: PendingOrder,
        _: CalculateOrderImage,
    ) -> Result<PendingOrder, PendingOrderError> {
        unreachable!()
    }
    async fn image(&self, _: &str) -> Result<CalculateOrderImage, PendingOrderError> {
        unreachable!()
    }
    async fn complete(
        &self,
        _: &str,
        _: &str,
        _: PendingOrderCompletion,
        _: ProductionMapSaved,
    ) -> Result<(PendingOrderCompletion, bool), PendingOrderError> {
        unreachable!()
    }
}

fn pending(id: &str) -> PendingOrder {
    PendingOrder {
        id: id.into(),
        telegram_user_id: "private-owner".into(),
        manager_name: "manager".into(),
        created_at: 10,
        completion: None,
        template: CalculateOrderTemplate {
            order_number: id.trim_start_matches("zakaz-").into(),
            product: "Pending product".into(),
            customer: "Pending customer".into(),
            image_id: "pending-image".into(),
            kg: 120.0,
            source_map_id: "large-detail-only".repeat(1000),
            ..Default::default()
        },
    }
}

async fn response(f: &Fixture, token: &str, uri: &str) -> (StatusCode, Value) {
    let response = f
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 10 * 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        },
    )
}

#[tokio::test]
async fn orders_projection_preserves_paging_labels_but_omits_full_authority_and_pending_templates()
{
    let _guard = FIXTURE_LOCK.lock().await;
    let mut f = Fixture::new().await;
    let intake = Arc::new(Intake::default());
    intake
        .0
        .lock()
        .await
        .extend([pending("zakaz-9000"), pending("zakaz-000")]);
    f.state.pending_orders = Some(intake);
    f.router = build_router(f.state.clone());
    let token = admin(&f, "orders-list").await;
    let full = page(&f, &token, "view=orders").await;
    let slim = page(&f, &token, QUERY).await;
    for key in ["order_ids", "total", "offset", "epoch", "rev"] {
        assert_eq!(slim[key], full[key], "{key}");
    }
    assert_eq!(slim["projection"], PROJECTION);
    assert_eq!(slim["pending_ids"], json!(["zakaz-9000"]));
    assert_eq!(
        slim["pending_rows"]["zakaz-9000"],
        json!({"id":"zakaz-9000","code":"9000","title":"Pending product","customer_name":"Pending customer","image_id":"pending-image","kg":120.0})
    );
    assert_eq!(slim["rows"].as_object().unwrap().len(), 19);
    for id in slim["order_ids"].as_array().unwrap() {
        let id = id.as_str().unwrap();
        let saved = full["maps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|saved| saved["map"]["id"] == id)
            .unwrap();
        assert_eq!(slim["rows"][id]["code"], saved["map"]["code"]);
        assert_eq!(slim["rows"][id]["title"], saved["map"]["title"]);
        assert_eq!(slim["rows"][id]["display_state"], "pending");
    }
    for key in [
        "maps",
        "program",
        "pending_orders",
        "calculation_templates",
        "queue_action_controls",
        "queue_states",
        "stage_states",
        "order_controls",
        "order_statuses",
        "queue_policies",
        "sequence_versions",
    ] {
        assert!(slim.get(key).is_none(), "{key}");
    }
    assert!(!slim.to_string().contains("private-owner"));
    assert!(!slim.to_string().contains("large-detail-only"));
    assert_ne!(slim["scope"], full["scope"]);
    assert_ne!(slim["version"], full["version"]);
    assert_eq!(
        response(
            &f,
            &token,
            &format!(
                "/v1/mobile/admin/production-maps/order-page?{QUERY}&if_version={}",
                slim["version"].as_str().unwrap()
            )
        )
        .await
        .0,
        StatusCode::NOT_MODIFIED
    );
    assert_eq!(
        response(
            &f,
            &token,
            &format!(
                "/v1/mobile/admin/production-maps/order-page?{QUERY}&cursor={}",
                full["next_cursor"].as_str().unwrap()
            )
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let other = admin(&f, "other-orders-admin").await;
    assert_eq!(
        response(
            &f,
            &other,
            &format!(
                "/v1/mobile/admin/production-maps/order-page?{QUERY}&cursor={}",
                slim["next_cursor"].as_str().unwrap()
            )
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let pending_detail =
        response(&f, &token, "/v1/mobile/admin/pending-orders?id=zakaz-9000").await;
    assert_eq!(pending_detail.0, StatusCode::OK);
    assert!(
        pending_detail.1["template"]["source_map_id"]
            .as_str()
            .unwrap()
            .len()
            > 10000
    );
    let mut all = prefix(&f, &token, QUERY, 4).await;
    assert_eq!(
        all.iter()
            .map(|page| page["rows"].as_object().unwrap().len()
                + page["pending_rows"].as_object().unwrap().len())
            .sum::<usize>(),
        62
    );
    assert!(all.remove(1)["pending_ids"].as_array().unwrap().is_empty());
    let search = page(&f, &token, &format!("{QUERY}&search=manager")).await;
    assert_eq!(search["pending_ids"], json!(["zakaz-9000"]));
    assert_eq!(search["rows"], json!({}));
}

#[tokio::test]
async fn orders_projection_rejects_cross_view_scope_and_worker_access_and_detail_is_single_order() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    let token = admin(&f, "orders-detail").await;
    for query in [
        "view=orders&projection=sequence_list_v1",
        "view=closed&projection=orders_list_v1",
        "view=sequence&projection=orders_list_v1",
    ] {
        assert_eq!(
            response(
                &f,
                &token,
                &format!("/v1/mobile/admin/production-maps/order-page?{query}")
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        response(
            &f,
            &f.token,
            &format!("/v1/mobile/admin/production-maps/order-page?{QUERY}")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, detail) = f.get_as(&token, "order_id=zakaz-000&detail=true").await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["projection"], "order_detail_v1");
    assert_eq!(detail["order_id"], "zakaz-000");
    assert_eq!(detail["apparatus"], "");
    assert_eq!(detail["maps"].as_array().unwrap().len(), 1);
    assert_eq!(detail["maps"][0]["map"]["id"], "zakaz-000");
    assert_eq!(
        serde_json::from_str::<Value>(detail["scope"].as_str().unwrap()).unwrap(),
        json!(["admin", "orders-detail", "order_detail_v1", "", "zakaz-000"])
    );
    assert!(detail["calculation_templates"].is_object());
    assert_eq!(
        f.get_as(&token, "order_id=zakaz-000").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        f.get_as(&token, "order_id=absent&detail=true").await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        f.get("order_id=zakaz-000&detail=true").await.0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn orders_identity_diagnostic_keeps_scope_and_data_while_default_compression_stays_gzip() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    let token = admin(&f, "orders-encoding").await;
    let mut payloads = Vec::new();
    for (suffix, compressed) in [("", true), ("&compression=identity", false)] {
        let response = f
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/v1/mobile/admin/production-maps/order-page?{QUERY}{suffix}"
                    ))
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::ACCEPT_ENCODING, "gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_ENCODING).is_some(),
            compressed
        );
        let bytes = to_bytes(response.into_body(), 10 * 1024 * 1024)
            .await
            .unwrap();
        let data = if compressed {
            use std::io::Read;
            let mut data = Vec::new();
            flate2::read::GzDecoder::new(bytes.as_ref())
                .read_to_end(&mut data)
                .unwrap();
            data
        } else {
            bytes.to_vec()
        };
        payloads.push(serde_json::from_slice::<Value>(&data).unwrap());
    }
    assert_eq!(payloads[0], payloads[1]);
}

fn subscription(version: &Value) -> String {
    format!("protocol=page_delta_v1&pages={}",urlencoding::encode(&json!([{"id":"orders","view":"orders","projection":PROJECTION,"length":40,"version":version}]).to_string()))
}

#[tokio::test]
async fn orders_live_colour_membership_and_intake_deltas_match_http_and_reconnect() {
    let _guard = FIXTURE_LOCK.lock().await;
    let mut f = Fixture::new().await;
    let intake = Arc::new(Intake::default());
    f.state.pending_orders = Some(intake.clone());
    f.router = build_router(f.state.clone());
    let token = admin(&f, "orders-live").await;
    let mut pages = prefix(&f, &token, QUERY, 2).await;
    let (address, server) = start(&f).await;
    let (mut socket, upgrade) = connect(address, &token, &subscription(&pages[0]["version"])).await;
    assert!(upgrade.starts_with("HTTP/1.1 101"), "{upgrade}");
    assert_eq!(ws_json(&mut socket).await["resync"], false);
    f.store
        .put_apparatus_queue_states(CUT, BTreeMap::from([("zakaz-000".into(), "paused".into())]))
        .await
        .unwrap();
    f.state.production_maps.notify_live();
    let delta = ws_json(&mut socket).await;
    assert_eq!(delta["type"], "page_window_delta");
    assert!(!delta.to_string().contains("queue_action_controls"));
    assert!(!delta.to_string().contains("\"maps\""));
    apply(&mut pages, &delta);
    assert_eq!(pages[0]["rows"]["zakaz-000"]["display_state"], "paused");
    assert_eq!(
        pages[0]["rows"]["zakaz-000"]["watermarks"],
        json!([{"apparatus_id":CUT,"kind":"paused"}])
    );
    assert_eq!(pages, prefix(&f, &token, QUERY, 2).await);
    intake.0.lock().await.push(pending("zakaz-9000"));
    f.state.production_maps.notify_live();
    let delta = ws_json(&mut socket).await;
    assert!(!delta.to_string().contains("large-detail-only"));
    apply(&mut pages, &delta);
    assert_eq!(pages, prefix(&f, &token, QUERY, 2).await);
    assert_eq!(pages[0]["pending_ids"], json!(["zakaz-9000"]));
    drop(socket);
    let (mut resumed, _) = connect(address, &token, &subscription(&pages[0]["version"])).await;
    assert_eq!(ws_json(&mut resumed).await["resync"], false);
    drop(resumed);
    server.abort();
}

#[tokio::test]
async fn orders_summary_wire_size_is_smaller_than_legacy_full_page() {
    use std::io::Write;
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    let token = admin(&f, "orders-size").await;
    let full = page(&f, &token, "view=orders").await;
    let slim = page(&f, &token, QUERY).await;
    let sizes = |value: &Value| {
        let raw = serde_json::to_vec(value).unwrap();
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&raw).unwrap();
        (raw.len(), encoder.finish().unwrap().len())
    };
    let (full_raw, full_gzip) = sizes(&full);
    let (slim_raw, slim_gzip) = sizes(&slim);
    eprintln!(
        "orders_list_v1 fixture=61_orders page=20 full_raw={full_raw} slim_raw={slim_raw} full_gzip={full_gzip} slim_gzip={slim_gzip}"
    );
    assert!(slim_raw < full_raw / 2);
    assert!(slim_gzip < full_gzip);
}
