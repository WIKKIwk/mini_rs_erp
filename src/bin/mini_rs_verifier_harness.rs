use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderName, HeaderValue, Method, Request};
use mini_rs_erp::app::AppState;
use mini_rs_erp::config::AppConfig;
use mini_rs_erp::core::auth::models::{Principal, PrincipalRole};
use mini_rs_erp::core::session::manager::SessionManager;
use mini_rs_erp::core::werka::models::{
    CustomerDirectoryEntry, CustomerItemOption, DispatchRecord, SupplierDirectoryEntry,
    SupplierItem, WerkaArchiveResponse, WerkaArchiveSummary, WerkaHomeData, WerkaHomeSummary,
    WerkaStatusBreakdownEntry,
};
use mini_rs_erp::core::werka::ports::{WerkaHomeLookup, WerkaPortError};
use mini_rs_erp::core::werka::service::WerkaService;
use mini_rs_erp::http::router::build_router;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tower::ServiceExt;

#[path = "mini_rs_verifier_harness/supplier_lookup.rs"]
mod supplier_lookup;
use supplier_lookup::VerifierSupplierLookup;

#[derive(Debug, Deserialize)]
struct ProbeRequest {
    #[serde(default = "default_method")]
    method: String,
    uri: String,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    body: Option<Value>,
    #[serde(default)]
    raw_body: Option<String>,
    #[serde(default)]
    fixture: Option<String>,
}

struct VerifierRouters {
    providers: axum::Router,
    isolated: axum::Router,
}

#[derive(Debug, Serialize)]
struct ProbeResponse {
    status: u16,
    headers: BTreeMap<String, String>,
    body: Value,
}

#[derive(Debug, Serialize)]
struct ProtocolError<'a> {
    error: &'a str,
}

fn default_method() -> String {
    "GET".to_string()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = verifier_workspace()?;
    std::fs::create_dir_all(workspace.join("data"))?;
    std::env::set_current_dir(&workspace)?;

    let mut isolated_state = AppState::verification(verifier_config(&workspace));
    isolated_state.sessions = SessionManager::memory(Some(60 * 60));
    let tokens = role_tokens(&isolated_state).await?;

    let mut provider_state = isolated_state.clone();
    provider_state.werka = WerkaService::new()
        .with_lookup(Arc::new(VerifierWerkaLookup))
        .with_supplier_read_lookup(Arc::new(VerifierSupplierLookup))
        .with_supplier_purchase_receipt_lookup(Arc::new(VerifierSupplierLookup))
        .with_supplier_item_lookup(Arc::new(VerifierSupplierLookup));
    let routers = VerifierRouters {
        providers: build_router(provider_state),
        isolated: build_router(isolated_state),
    };

    write_json(&serde_json::json!({
        "ready": true,
        "protocol": 1,
        "roles": tokens.keys().collect::<Vec<_>>(),
    }))?;

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request = match serde_json::from_str::<ProbeRequest>(&line) {
            Ok(request) => request,
            Err(_) => {
                write_json(&ProtocolError {
                    error: "invalid verifier request",
                })?;
                continue;
            }
        };
        match probe(&routers, &tokens, request).await {
            Ok(response) => write_json(&response)?,
            Err(error) => write_json(&ProtocolError { error: &error })?,
        }
    }
    Ok(())
}

async fn probe(
    routers: &VerifierRouters,
    tokens: &BTreeMap<String, String>,
    probe: ProbeRequest,
) -> Result<ProbeResponse, String> {
    let router = match probe.fixture.as_deref().unwrap_or("providers") {
        "providers" => routers.providers.clone(),
        "isolated" => routers.isolated.clone(),
        _ => return Err("unknown verifier fixture".to_string()),
    };
    let method = Method::from_bytes(probe.method.trim().as_bytes())
        .map_err(|_| "invalid method".to_string())?;
    let mut builder = Request::builder().method(method).uri(probe.uri);
    for (name, value) in probe.headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| "invalid header name".to_string())?;
        let value =
            HeaderValue::from_str(&value).map_err(|_| "invalid header value".to_string())?;
        builder = builder.header(name, value);
    }
    if let Some(role) = probe.role {
        let token = tokens
            .get(role.trim())
            .ok_or_else(|| "unknown verifier role".to_string())?;
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let body = match (probe.raw_body, probe.body) {
        (Some(raw), _) => Body::from(raw),
        (None, Some(json)) => {
            builder = builder.header("content-type", "application/json");
            Body::from(serde_json::to_vec(&json).map_err(|_| "invalid JSON body".to_string())?)
        }
        (None, None) => Body::empty(),
    };
    let response = router
        .oneshot(
            builder
                .body(body)
                .map_err(|_| "invalid request".to_string())?,
        )
        .await
        .expect("Axum router service is infallible");
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect();
    let bytes = to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .map_err(|_| "response body failed".to_string())?;
    let body = if bytes.is_empty() {
        Value::Null
    } else if let Ok(json) = serde_json::from_slice(&bytes) {
        json
    } else {
        Value::String(String::from_utf8_lossy(&bytes).into_owned())
    };
    Ok(ProbeResponse {
        status,
        headers,
        body,
    })
}

async fn role_tokens(
    state: &AppState,
) -> Result<BTreeMap<String, String>, Box<dyn std::error::Error>> {
    let roles = [
        ("supplier", PrincipalRole::Supplier),
        ("werka", PrincipalRole::Werka),
        ("customer", PrincipalRole::Customer),
        ("aparatchi", PrincipalRole::Aparatchi),
        ("qolipchi", PrincipalRole::Qolipchi),
        ("boyoqchi", PrincipalRole::Boyoqchi),
        ("material_taminotchi", PrincipalRole::MaterialTaminotchi),
        ("admin", PrincipalRole::Admin),
    ];
    let mut tokens = BTreeMap::new();
    for (name, role) in roles {
        let principal_ref = if name == "supplier" {
            "SUP-001".to_string()
        } else {
            format!("verify-{name}")
        };
        let token = state
            .sessions
            .create(Principal {
                role,
                display_name: format!("Verifier {name}"),
                legal_name: format!("Verifier {name}"),
                ref_: principal_ref,
                phone: "+998000000000".to_string(),
                avatar_url: String::new(),
            })
            .await?;
        tokens.insert(name.to_string(), token);
    }
    Ok(tokens)
}

fn verifier_workspace() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = std::env::var_os("MINI_ERP_VERIFIER_TMP")
        .map(PathBuf::from)
        .ok_or("MINI_ERP_VERIFIER_TMP is required")?;
    if !path.is_absolute() {
        return Err("MINI_ERP_VERIFIER_TMP must be absolute".into());
    }
    Ok(path)
}

fn verifier_config(workspace: &Path) -> AppConfig {
    let data = workspace.join("data");
    AppConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static verifier address"),
        default_target_warehouse: String::new(),
        http_timeout: Duration::from_secs(2),
        session_store_path: data.join("sessions.json"),
        profile_store_path: data.join("profiles.json"),
        push_token_store_path: data.join("push_tokens.json"),
        session_ttl_seconds: Some(60 * 60),
        supplier_prefix: "10".to_string(),
        werka_prefix: "20".to_string(),
        werka_code: "20ABCDEF1234".to_string(),
        werka_name: "Werka".to_string(),
        werka_phone: "+99888862440".to_string(),
        material_taminotchi_code: String::new(),
        material_taminotchi_name: "Material taminotchisi".to_string(),
        material_taminotchi_phone: String::new(),
        admin_phone: "+998880000000".to_string(),
        admin_name: "Admin".to_string(),
        admin_code: "19621978".to_string(),
    }
}

fn write_json(value: &impl Serialize) -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, value)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    Ok(())
}

struct VerifierWerkaLookup;

#[async_trait]
impl WerkaHomeLookup for VerifierWerkaLookup {
    async fn werka_summary(&self) -> Result<WerkaHomeSummary, WerkaPortError> {
        Ok(WerkaHomeSummary {
            pending_count: 2,
            confirmed_count: 3,
            returned_count: 1,
        })
    }

    async fn werka_home(&self, pending_limit: usize) -> Result<WerkaHomeData, WerkaPortError> {
        if pending_limit != 20 {
            return Err(WerkaPortError::LookupFailed);
        }
        Ok(WerkaHomeData {
            summary: self.werka_summary().await?,
            pending_items: Vec::new(),
        })
    }

    async fn werka_pending(&self, limit: usize) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        if limit != 0 {
            return Err(WerkaPortError::LookupFailed);
        }
        Ok(vec![DispatchRecord {
            id: "PR-001".to_string(),
            supplier_name: "Supplier".to_string(),
            item_code: "ITEM-001".to_string(),
            item_name: "Item".to_string(),
            uom: "Kg".to_string(),
            sent_qty: 10.0,
            accepted_qty: 0.0,
            status: "pending".to_string(),
            created_label: "2026-01-16".to_string(),
            ..DispatchRecord::default()
        }])
    }

    async fn werka_history(&self) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        Ok(vec![DispatchRecord {
            id: "supplier_ack:COMM-001".to_string(),
            supplier_name: "Supplier".to_string(),
            item_code: "ITEM-001".to_string(),
            item_name: "Item".to_string(),
            uom: "Kg".to_string(),
            sent_qty: 10.0,
            accepted_qty: 10.0,
            event_type: "supplier_ack".to_string(),
            status: "accepted".to_string(),
            created_label: "2026-01-16".to_string(),
            ..DispatchRecord::default()
        }])
    }

    async fn werka_status_breakdown(
        &self,
        kind: &str,
    ) -> Result<Vec<WerkaStatusBreakdownEntry>, WerkaPortError> {
        verifier_condition(kind == "returned")?;
        Ok(vec![WerkaStatusBreakdownEntry {
            supplier_ref: "SUP-001".to_string(),
            supplier_name: "Supplier".to_string(),
            receipt_count: 1,
            total_sent_qty: 10.0,
            total_accepted_qty: 8.0,
            total_returned_qty: 2.0,
            uom: "Kg".to_string(),
        }])
    }

    async fn werka_status_details(
        &self,
        kind: &str,
        supplier_ref: &str,
    ) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        verifier_condition(kind == "pending" && supplier_ref == "SUP-001")?;
        Ok(vec![DispatchRecord {
            id: "PR-001".to_string(),
            supplier_ref: "SUP-001".to_string(),
            supplier_name: "Supplier".to_string(),
            item_code: "ITEM-001".to_string(),
            item_name: "Item".to_string(),
            uom: "Kg".to_string(),
            sent_qty: 10.0,
            accepted_qty: 0.0,
            status: "pending".to_string(),
            created_label: "2026-01-16".to_string(),
            ..DispatchRecord::default()
        }])
    }

    async fn werka_archive(
        &self,
        kind: &str,
        period: &str,
        from: Option<time::Date>,
        to: Option<time::Date>,
    ) -> Result<WerkaArchiveResponse, WerkaPortError> {
        verifier_condition(kind == "sent" && period == "monthly")?;
        Ok(WerkaArchiveResponse {
            kind: kind.to_string(),
            period: period.to_string(),
            from: from.map(|date| date.to_string()).unwrap_or_default(),
            to: to.map(|date| date.to_string()).unwrap_or_default(),
            summary: WerkaArchiveSummary {
                record_count: 1,
                totals_by_uom: Vec::new(),
            },
            items: vec![DispatchRecord {
                id: "DN-001".to_string(),
                record_type: "delivery_note".to_string(),
                supplier_name: "Customer".to_string(),
                item_code: "ITEM-001".to_string(),
                item_name: "Item".to_string(),
                uom: "Kg".to_string(),
                sent_qty: 12.0,
                accepted_qty: 10.0,
                status: "partial".to_string(),
                created_label: "2026-01-16".to_string(),
                ..DispatchRecord::default()
            }],
        })
    }

    async fn werka_suppliers(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierDirectoryEntry>, WerkaPortError> {
        verifier_condition(
            (query == "Ali" && limit == 200 && offset == 3)
                || (query.is_empty() && limit == 200 && offset == 0),
        )?;
        Ok(vec![SupplierDirectoryEntry {
            ref_: "SUP-001".to_string(),
            name: "Ali".to_string(),
            phone: "+998901111111".to_string(),
        }])
    }

    async fn werka_customers(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<CustomerDirectoryEntry>, WerkaPortError> {
        verifier_condition(
            (query == "Ali" && limit == 200 && offset == 3)
                || (query.is_empty() && limit == 200 && offset == 0),
        )?;
        Ok(vec![CustomerDirectoryEntry {
            ref_: "CUST-001".to_string(),
            name: "Ali Market".to_string(),
            phone: "+998902222222".to_string(),
        }])
    }

    async fn werka_supplier_items(
        &self,
        supplier_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        verifier_condition(
            (supplier_ref == "SUP-001" && query == "milk" && limit == 200 && offset == 3)
                || (supplier_ref.is_empty() && query.is_empty() && limit == 100 && offset == 0),
        )?;
        Ok(vec![verifier_supplier_item("ITEM-001", "Supplier Milk")])
    }

    async fn werka_customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        verifier_condition(
            (customer_ref == "CUST-001" && query == "milk" && limit == 200 && offset == 3)
                || (customer_ref.is_empty() && query.is_empty() && limit == 100 && offset == 0),
        )?;
        Ok(vec![verifier_supplier_item("ITEM-002", "Customer Milk")])
    }

    async fn werka_customer_item_options(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<CustomerItemOption>, WerkaPortError> {
        verifier_condition(
            (query == "milk" && limit == 200 && offset == 3)
                || (query.is_empty() && limit == 200 && offset == 0),
        )?;
        Ok(vec![CustomerItemOption {
            customer_ref: "CUST-001".to_string(),
            customer_name: "Ali Market".to_string(),
            customer_phone: "+998901111111".to_string(),
            item_code: "ITEM-003".to_string(),
            item_name: "Option Milk".to_string(),
            uom: "Kg".to_string(),
            warehouse: "Stores - A".to_string(),
        }])
    }
}

fn verifier_condition(condition: bool) -> Result<(), WerkaPortError> {
    if condition {
        Ok(())
    } else {
        Err(WerkaPortError::LookupFailed)
    }
}

fn verifier_supplier_item(code: &str, name: &str) -> SupplierItem {
    SupplierItem {
        code: code.to_string(),
        name: name.to_string(),
        uom: "Kg".to_string(),
        warehouse: "Stores - A".to_string(),
        item_group: String::new(),
        customer_names: Vec::new(),
    }
}
