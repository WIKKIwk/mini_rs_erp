use super::*;
use crate::core::authz::Capability;
use crate::core::warehouses::WarehouseAssignmentDeleteRequest;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct InventoryLiveServer {
    address: std::net::SocketAddr,
    task: tokio::task::JoinHandle<()>,
    client: reqwest::Client,
}

impl InventoryLiveServer {
    async fn start(state: AppState) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, build_router(state)).await.unwrap();
        });
        Self {
            address,
            task,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
        }
    }

    async fn open(&self, token: &str, version: Option<&str>) -> reqwest::Response {
        let subscriptions = json!([{
            "id":"window", "kind":"assets",
            "query":{
                "warehouse_id":"warehouse:material", "limit":2,
                "include_order_assignment":true, "current_user_states_only":false,
            },
            "version":version.unwrap_or_default(),
        }]);
        // The native/web client supplies URL subscriptions and a query token.
        // Exercise that exact path, including typed JSON limits and flags.
        self.client
            .get(format!(
                "http://{}/v1/mobile/admin/inventory/live",
                self.address
            ))
            .query(&[
                ("token", token),
                ("subscriptions", &subscriptions.to_string()),
            ])
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
            .send()
            .await
            .unwrap()
    }

    async fn socket(&self, token: &str, version: Option<&str>) -> reqwest::Upgraded {
        let response = self.open(token, version).await;
        assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
        response.upgrade().await.unwrap()
    }
}

impl Drop for InventoryLiveServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn message(socket: &mut reqwest::Upgraded) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let opcode = socket.read_u8().await.unwrap() & 0x0f;
            let byte = socket.read_u8().await.unwrap();
            assert_eq!(byte & 0x80, 0, "server frames must be unmasked");
            let length = match byte & 0x7f {
                126 => socket.read_u16().await.unwrap() as usize,
                127 => socket.read_u64().await.unwrap() as usize,
                n => n as usize,
            };
            assert!(length < 128 * 1024, "bounded inventory window frame");
            let mut payload = vec![0; length];
            socket.read_exact(&mut payload).await.unwrap();
            if opcode == 1 {
                return serde_json::from_slice(&payload).unwrap();
            }
            if opcode == 9 {
                assert!(length <= 125);
                let mut pong = vec![0x8a, 0x80 | length as u8, 0, 0, 0, 0];
                pong.extend(payload);
                socket.write_all(&pong).await.unwrap();
            }
            assert_ne!(opcode, 8, "socket closed before inventory decision");
        }
    })
    .await
    .expect("inventory live response")
}

async fn close(socket: &mut reqwest::Upgraded) {
    socket.write_all(&[0x88, 0x80, 0, 0, 0, 0]).await.unwrap();
    let mut tail = Vec::new();
    tokio::time::timeout(Duration::from_secs(2), socket.read_to_end(&mut tail))
        .await
        .expect("closed inventory subscriber released")
        .unwrap();
}

fn location(warehouse_id: &str, name: &str) -> InventoryLocation {
    InventoryLocation {
        id: format!("inventory_location:{warehouse_id}"),
        kind: InventoryLocationKind::Warehouse,
        name: name.into(),
        warehouse_id: warehouse_id.into(),
        factory_location_id: String::new(),
        active: true,
        apparatus: vec![],
    }
}

fn asset(id: &str, name: &str, location: &InventoryLocation) -> InventoryAsset {
    InventoryAsset {
        kind: InventoryAssetKind::RawMaterial,
        asset_ref: id.into(),
        custody_warehouse_id: location.warehouse_id.clone(),
        custody_warehouse: location.name.clone(),
        item_code: id.into(),
        item_name: name.into(),
        identifier: id.into(),
        qty: 10.0,
        uom: "kg".into(),
        status: "available".into(),
        physical_location: InventoryLocationRef::from(location),
        transfer_id: String::new(),
        placement_version: 1,
    }
}

#[tokio::test]
async fn inventory_live_material_scope_deltas_resume_and_revocation() {
    let mut state = test_state();
    let store = Arc::new(MemoryInventoryMovementStore::new());
    let material = location("warehouse:material", "Material ombor");
    let other = location("warehouse:other", "Other ombor");
    let factory = InventoryLocation {
        id: "inventory_location:state:bosma".into(),
        kind: InventoryLocationKind::State,
        name: "Bosma oldi".into(),
        warehouse_id: String::new(),
        factory_location_id: "state_bosma".into(),
        active: true,
        apparatus: vec![],
    };
    store
        .seed_locations(vec![material.clone(), other.clone(), factory.clone()])
        .await;
    let mut first = asset("raw:a", "A", &material);
    let second = asset("raw:b", "B", &material);
    let third = asset("raw:c", "C", &material);
    store
        .seed_assets(vec![
            first.clone(),
            second.clone(),
            third.clone(),
            asset("raw:private", "AAA private", &other),
        ])
        .await;
    state.inventory_movements = InventoryMovementService::new(store.clone());
    assign_warehouse_to_principal(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "inventory-live-material",
        &material.name,
    )
    .await;
    let token = session_for(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "inventory-live-material",
    )
    .await;
    let principal = state.sessions.get(&token).await.unwrap();
    assert!(
        !state
            .admin
            .principal_has_capability(&principal, Capability::CatalogItemRead)
            .await
    );
    let server = InventoryLiveServer::start(state.clone()).await;
    let mut socket = server.socket(&token, None).await;
    let snapshot = message(&mut socket).await;
    assert_eq!(snapshot["event"], "inventory.snapshot");
    assert_eq!(snapshot["rows"].as_array().unwrap().len(), 2);
    assert_eq!(snapshot["rows"][0]["asset_ref"], "raw:a");
    assert_eq!(snapshot["rows"][1]["asset_ref"], "raw:b");
    assert!(
        snapshot["rows"][0]
            .get("order_assignment")
            .unwrap()
            .is_null()
    );

    // Moving the first row out must remove it and pull row 3 into the capped
    // window, while the unchanged second row is represented only by its key.
    first.physical_location = InventoryLocationRef::from(&factory);
    first.placement_version += 1;
    store.seed_assets(vec![first]).await;
    state
        .warehouse_events
        .notify_updated(&material.name, "inventory_relocated");
    let delta = message(&mut socket).await;
    assert_eq!(delta["event"], "inventory.delta");
    assert_eq!(delta["epoch"], snapshot["epoch"]);
    assert_eq!(delta["base_revision"], snapshot["revision"]);
    assert_eq!(delta["revision"], 1);
    assert_eq!(delta["removed"], json!(["raw_material:raw:a"]));
    assert_eq!(delta["upserts"].as_array().unwrap().len(), 1);
    assert_eq!(delta["upserts"][0]["asset_ref"], "raw:c");
    assert_eq!(
        delta["order"],
        json!(["raw_material:raw:b", "raw_material:raw:c"])
    );
    close(&mut socket).await;

    // A cached current version is validated without downloading the rows.
    let mut resumed = server.socket(&token, delta["version"].as_str()).await;
    let ready = message(&mut resumed).await;
    assert_eq!(ready["event"], "inventory.ready");
    assert_eq!(ready["version"], delta["version"]);
    assert!(ready.get("rows").is_none());

    state
        .warehouses
        .unassign_warehouse(WarehouseAssignmentDeleteRequest {
            assignment_kind: "warehouse".into(),
            warehouse: material.name.clone(),
            warehouse_name: Some(material.name.clone()),
            apparatus_id: None,
            principal_role: PrincipalRole::MaterialTaminotchi,
            principal_ref: "inventory-live-material".into(),
        })
        .await
        .unwrap();
    state
        .warehouse_events
        .notify_updated(&material.name, "warehouse_assignment_removed");
    let reset = message(&mut resumed).await;
    assert_eq!(reset["event"], "inventory.reset");
    assert!(reset.get("rows").is_none());
    assert!(reset.get("upserts").is_none());
}
