//! LOCAL SYNTHETIC FIXTURE ONLY. Opens no socket and never constructs AppState.
use async_trait::async_trait;
use mini_rs_erp::{
    core::auth::models::{Principal, PrincipalRole},
    mcp_readonly::{Args, GrantResolver, ReadPort, ReadResult, Server, TOOLS, VerifiedGrant},
};
use serde_json::{Value, json};
use std::io::{self, BufRead, Read, Write};
struct LocalFixture;
#[async_trait]
impl GrantResolver for LocalFixture {
    async fn resolve(&self, credential: &str) -> Result<VerifiedGrant, ()> {
        if credential != "in-process-synthetic-fixture" {
            return Err(());
        }
        Ok(VerifiedGrant {
            principal: Principal {
                role: PrincipalRole::Admin,
                display_name: "Synthetic fixture".into(),
                legal_name: String::new(),
                ref_: "fixture".into(),
                phone: String::new(),
                avatar_url: String::new(),
            },
            deployment: "synthetic-fixture".into(),
            scopes: TOOLS.iter().map(|s| s.to_string()).collect(),
        })
    }
}
#[async_trait]
impl ReadPort for LocalFixture {
    async fn permitted(&self, _: &VerifiedGrant, _: &str) -> bool {
        true
    }
    async fn read(&self, tool: &str, args: &Args) -> Result<ReadResult, ()> {
        let data = match tool {
            "erp_summary" => {
                json!([{"warehouse":"DEMO-WH","product_count":2,"reserved_count":1,"unit":"count"}])
            }
            "erp_order_status" => {
                json!({"order_id":args.order_id,"order_status":"DEMO_IN_PROGRESS"})
            }
            "erp_wip" => json!([{"order_id":args.order_id,"quantity":12.5,"unit":"kg"}]),
            "erp_warehouse" => {
                json!([{"warehouse":args.warehouse,"item_code":"DEMO-ITEM","quantity":40,"unit":"kg"}])
            }
            _ => return Err(()),
        };
        Ok(ReadResult {
            data: json!({"synthetic_fixture":true,"rows":data}),
            partial: true,
        })
    }
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().nth(1).as_deref() != Some("--synthetic-fixture") {
        eprintln!("Disabled. Use --synthetic-fixture for local fake-data stdio only.");
        return Ok(());
    }
    let mut server = Server::disabled("synthetic-fixture".into(), LocalFixture, LocalFixture);
    server.enable();
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut output = io::stdout().lock();
    loop {
        let mut bytes = Vec::new();
        let n = input.by_ref().take(32769).read_until(b'\n', &mut bytes)?;
        if n == 0 {
            break;
        }
        if n > 32768 {
            eprintln!("Request too large; closing fixture.");
            break;
        }
        let request: Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => {
                writeln!(
                    output,
                    "{}",
                    json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}})
                )?;
                output.flush()?;
                continue;
            }
        };
        // JSON-RPC notifications must never receive responses.
        if request.get("id").is_none() && request.get("method").is_some() {
            continue;
        }
        let response = server.handle("in-process-synthetic-fixture", request).await;
        writeln!(output, "{}", response)?;
        output.flush()?;
    }
    Ok(())
}
