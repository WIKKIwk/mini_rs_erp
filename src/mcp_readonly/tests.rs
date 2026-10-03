use super::*;
use crate::core::auth::models::{Principal, PrincipalRole};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
struct FixtureResolver;
#[async_trait]
impl GrantResolver for FixtureResolver {
    async fn resolve(&self, c: &str) -> Result<VerifiedGrant, ()> {
        if c != "LOCAL-FIXTURE-NOT-A-REAL-TOKEN" && c != "FIXTURE-NO-SCOPES" {
            return Err(());
        }
        Ok(VerifiedGrant {
            principal: Principal {
                role: PrincipalRole::Admin,
                display_name: "Fixture".into(),
                legal_name: String::new(),
                ref_: "fixture".into(),
                phone: String::new(),
                avatar_url: String::new(),
            },
            deployment: "fixture-only".into(),
            scopes: if c == "FIXTURE-NO-SCOPES" {
                BTreeSet::new()
            } else {
                TOOLS.iter().map(|s| s.to_string()).collect()
            },
        })
    }
}
struct FixturePort {
    allowed: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
    fail: bool,
}
#[async_trait]
impl ReadPort for FixturePort {
    async fn permitted(&self, _: &VerifiedGrant, _: &str) -> bool {
        self.allowed.load(Ordering::SeqCst)
    }
    async fn read(&self, _: &str, _: &Args) -> Result<ReadResult, ()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            Err(())
        } else {
            Ok(ReadResult {
                data: json!({"quantity":3,"unit":"kg"}),
                partial: true,
            })
        }
    }
}
fn call(name: &str, args: Value) -> Value {
    json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}})
}
fn fixture() -> (
    Server<FixtureResolver, FixturePort>,
    Arc<AtomicBool>,
    Arc<AtomicUsize>,
) {
    let allowed = Arc::new(AtomicBool::new(true));
    let calls = Arc::new(AtomicUsize::new(0));
    (
        Server::disabled(
            "fixture-only".into(),
            FixtureResolver,
            FixturePort {
                allowed: allowed.clone(),
                calls: calls.clone(),
                fail: false,
            },
        ),
        allowed,
        calls,
    )
}
#[tokio::test]
async fn defaults_disabled_and_no_reads() {
    let (s, _, calls) = fixture();
    assert_eq!(
        s.handle(
            "LOCAL-FIXTURE-NOT-A-REAL-TOKEN",
            call("erp_summary", json!({}))
        )
        .await["error"]["message"],
        "MCP disabled"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn fixture_rejects_unrecognized_token_and_wrong_deployment() {
    let (mut s, _, calls) = fixture();
    s.enable();
    assert!(
        s.handle("mobile-admin-token", call("erp_summary", json!({})))
            .await
            .get("error")
            .is_some()
    );
    s.deployment = "other".into();
    assert!(
        s.handle(
            "LOCAL-FIXTURE-NOT-A-REAL-TOKEN",
            call("erp_summary", json!({}))
        )
        .await
        .get("error")
        .is_some()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn live_permissions_rechecked_and_metadata_present() {
    let (mut s, allowed, calls) = fixture();
    s.enable();
    let r = s
        .handle(
            "LOCAL-FIXTURE-NOT-A-REAL-TOKEN",
            call("erp_wip", json!({"order_id":"o1"})),
        )
        .await;
    let body: Value =
        serde_json::from_str(r["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(body["metadata"]["partial"], true);
    assert_eq!(body["data"]["unit"], "kg");
    assert!(body["metadata"]["retrieved_at_unix"].is_number());
    allowed.store(false, Ordering::SeqCst);
    assert!(
        s.handle(
            "LOCAL-FIXTURE-NOT-A-REAL-TOKEN",
            call("erp_wip", json!({"order_id":"o1"}))
        )
        .await
        .get("error")
        .is_some()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn arguments_writes_and_unknown_methods_fail_closed() {
    let (mut s, _, calls) = fixture();
    s.enable();
    for request in [
        call("erp_summary", json!({"sql":"DROP TABLE x"})),
        call("erp_wip", json!({})),
        call("erp_warehouse", json!({"warehouse":"x","limit":101})),
        call("erp_order_status", json!({"order_id":" "})),
        call("erp_delete", json!({})),
        json!({"jsonrpc":"2.0","id":1,"method":"resources/read"}),
    ] {
        assert!(
            s.handle("LOCAL-FIXTURE-NOT-A-REAL-TOKEN", request)
                .await
                .get("error")
                .is_some()
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn list_is_filtered_and_read_errors_are_generic() {
    let (mut s, allowed, _) = fixture();
    s.enable();
    let request = json!({"jsonrpc":"2.0","id":1,"method":"tools/list"});
    assert_eq!(
        s.handle("LOCAL-FIXTURE-NOT-A-REAL-TOKEN", request.clone())
            .await["result"]["tools"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    allowed.store(false, Ordering::SeqCst);
    assert_eq!(
        s.handle("LOCAL-FIXTURE-NOT-A-REAL-TOKEN", request).await["result"]["tools"],
        json!([])
    );
    allowed.store(true, Ordering::SeqCst);
    s.port.fail = true;
    assert_eq!(
        s.handle(
            "LOCAL-FIXTURE-NOT-A-REAL-TOKEN",
            call("erp_summary", json!({}))
        )
        .await["error"]["message"],
        "Read unavailable"
    );
}
#[test]
fn bounds_and_no_cross_tool_fields() {
    for n in [0, 101] {
        assert!(Args::parse("erp_summary", json!({"limit":n})).is_err());
    }
    assert!(Args::parse("erp_summary", json!({"limit":100})).is_ok());
    assert!(Args::parse("erp_order_status", json!({"order_id":"a","warehouse":"b"})).is_err());
    assert!(Args::parse("erp_order_status", json!({"order_id":"a\nb"})).is_err());
}

#[tokio::test]
async fn scope_denial_prevents_service_read() {
    let (mut s, _, calls) = fixture();
    s.enable();
    assert!(
        s.handle("FIXTURE-NO-SCOPES", call("erp_summary", json!({})))
            .await
            .get("error")
            .is_some()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn native_non_admin_is_ineligible_even_with_external_scopes() {
    let mut grant = FixtureResolver
        .resolve("LOCAL-FIXTURE-NOT-A-REAL-TOKEN")
        .await
        .unwrap();
    assert!(erp::native_admin(&grant.principal));
    grant.principal.role = PrincipalRole::Werka;
    assert!(!erp::native_admin(&grant.principal));
    grant.principal.role = PrincipalRole::MaterialTaminotchi;
    assert!(!erp::native_admin(&grant.principal));
}

#[tokio::test]
async fn actual_domain_port_uses_isolated_stores_and_live_capabilities() {
    use crate::core::{
        admin::service::AdminService,
        authz::{
            Capability, MemoryRoleDefinitionStore, RoleAssignment, RoleDefinition,
            RoleDefinitionStorePort, capability_code,
        },
        production_map::{
            MemoryProductionMapStore, ProductionMapService, TestCanonicalApparatusResolver,
        },
        warehouses::{MemoryWarehouseStore, WarehouseService, WarehouseStockItem},
    };
    // No AppState/from_env, live DB, disk stores or network services are constructed.
    let config = crate::config::AppConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        default_target_warehouse: String::new(),
        http_timeout: Duration::from_secs(1),
        session_store_path: Default::default(),
        profile_store_path: Default::default(),
        push_token_store_path: Default::default(),
        session_ttl_seconds: None,
        supplier_prefix: String::new(),
        werka_prefix: String::new(),
        werka_code: String::new(),
        werka_name: String::new(),
        werka_phone: String::new(),
        material_taminotchi_code: String::new(),
        material_taminotchi_name: String::new(),
        material_taminotchi_phone: String::new(),
        admin_phone: String::new(),
        admin_name: String::new(),
        admin_code: String::new(),
    };
    let roles = Arc::new(MemoryRoleDefinitionStore::new());
    let warehouse_store = Arc::new(MemoryWarehouseStore::new());
    warehouse_store
        .set_stock_items(vec![WarehouseStockItem {
            code: "DEMO".into(),
            name: "Not exposed".into(),
            uom: "kg".into(),
            warehouse: "MAIN".into(),
            order_id: "ORDER-1".into(),
            item_group: "Not exposed".into(),
            on_hand_qty: 12.5,
            package_count: 2,
        }])
        .await;
    let resolver = Arc::new(TestCanonicalApparatusResolver::default());
    let port = erp::ErpReadPort {
        admin: AdminService::new(&config).with_role_store(roles.clone()),
        production_maps: ProductionMapService::new(
            Arc::new(MemoryProductionMapStore::new()),
            resolver.clone(),
        ),
        warehouses: WarehouseService::new(warehouse_store, resolver),
    };
    let mut grant = FixtureResolver
        .resolve("LOCAL-FIXTURE-NOT-A-REAL-TOKEN")
        .await
        .unwrap();
    assert!(port.permitted(&grant, "erp_warehouse").await);
    let data = port
        .read(
            "erp_warehouse",
            &Args::parse("erp_warehouse", json!({"warehouse":"main"})).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(data.data[0]["order_id"], "ORDER-1");
    assert_eq!(data.data[0]["unit"], "kg");
    assert_eq!(data.data[0]["quantity"], 12.5);
    assert!(data.data[0].get("name").is_none());
    assert!(data.data[0].get("item_group").is_none());
    assert!(
        port.read(
            "erp_order_status",
            &Args::parse("erp_order_status", json!({"order_id":"absent"})).unwrap()
        )
        .await
        .is_err()
    );
    let wip = port
        .read(
            "erp_wip",
            &Args::parse("erp_wip", json!({"order_id":"absent"})).unwrap(),
        )
        .await
        .unwrap();
    assert!(wip.partial);
    assert_eq!(wip.data, json!([]));
    roles
        .put_role_definition(RoleDefinition {
            id: "mcp-limited".into(),
            label: "Limited fixture".into(),
            base_role: None,
            capability_codes: vec![capability_code(Capability::AdminAccess).unwrap().into()],
            system: false,
        })
        .await
        .unwrap();
    roles
        .put_role_assignment(RoleAssignment {
            principal_role: PrincipalRole::Admin,
            principal_ref: grant.principal.ref_.clone(),
            role_id: "mcp-limited".into(),
            assigned_apparatus: vec![],
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
    // Current custom assignment removes read permission even though the old grant still has its scope.
    assert!(!port.permitted(&grant, "erp_warehouse").await);
    grant.principal.role = PrincipalRole::Werka;
    roles
        .put_role_definition(RoleDefinition {
            id: "mcp-elevated".into(),
            label: "Elevated fixture".into(),
            base_role: None,
            capability_codes: vec![
                capability_code(Capability::AdminAccess).unwrap().into(),
                capability_code(Capability::CatalogItemRead).unwrap().into(),
            ],
            system: false,
        })
        .await
        .unwrap();
    roles
        .put_role_assignment(RoleAssignment {
            principal_role: PrincipalRole::Werka,
            principal_ref: grant.principal.ref_.clone(),
            role_id: "mcp-elevated".into(),
            assigned_apparatus: vec![],
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
    assert!(!port.permitted(&grant, "erp_warehouse").await);
}
