//! Opt-in read-only MCP core. No route, listener, credential store or production grants.
//! A future transport MUST verify OAuth and resolve a live principal before each call.
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::time::Duration;

pub mod erp;
#[cfg(test)]
mod tests;

pub const TOOLS: [&str; 4] = [
    "erp_summary",
    "erp_order_status",
    "erp_wip",
    "erp_warehouse",
];
const MAX_RESPONSE: usize = 131_072;

/// Only created by trusted authentication code, never deserialized from tool arguments.
/// Resolver must check signature, issuer, audience/resource, expiry, revocation and live
/// account status. Scopes are intersected with current ERP capabilities by `ReadPort`.
pub struct VerifiedGrant {
    pub principal: crate::core::auth::models::Principal,
    pub deployment: String,
    pub scopes: BTreeSet<String>,
}
#[async_trait]
pub trait GrantResolver: Send + Sync {
    async fn resolve(&self, credential: &str) -> Result<VerifiedGrant, ()>;
}
pub struct DenyAll;
#[async_trait]
impl GrantResolver for DenyAll {
    async fn resolve(&self, _: &str) -> Result<VerifiedGrant, ()> {
        Err(())
    }
}
#[async_trait]
pub trait ReadPort: Send + Sync {
    /// Check CURRENT permissions on every request; never trust grant scopes as ERP roles.
    async fn permitted(&self, grant: &VerifiedGrant, tool: &str) -> bool;
    async fn read(&self, tool: &str, args: &Args) -> Result<ReadResult, ()>;
}
pub struct ReadResult {
    pub data: Value,
    pub partial: bool,
}
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Args {
    pub order_id: Option<String>,
    pub warehouse: Option<String>,
    pub limit: Option<usize>,
}
fn valid_identifier(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && s.trim() == s && !s.chars().any(char::is_control)
}
impl Args {
    fn parse(tool: &str, value: Value) -> Result<Self, ()> {
        let a: Self = serde_json::from_value(value).map_err(|_| ())?;
        if a.limit.is_some_and(|n| n == 0 || n > 100)
            || a.order_id.as_deref().is_some_and(|s| !valid_identifier(s))
            || a.warehouse.as_deref().is_some_and(|s| !valid_identifier(s))
        {
            return Err(());
        }
        let valid = match tool {
            "erp_summary" => a.order_id.is_none() && a.warehouse.is_none(),
            "erp_order_status" => {
                a.order_id.is_some() && a.warehouse.is_none() && a.limit.is_none()
            }
            "erp_wip" => a.order_id.is_some() && a.warehouse.is_none(),
            "erp_warehouse" => a.warehouse.is_some() && a.order_id.is_none(),
            _ => false,
        };
        if valid { Ok(a) } else { Err(()) }
    }
    pub fn limit(&self) -> usize {
        self.limit.unwrap_or(25)
    }
}

pub struct Server<R, P> {
    enabled: bool,
    deployment: String,
    resolver: R,
    port: P,
}
impl<R: GrantResolver, P: ReadPort> Server<R, P> {
    pub fn disabled(deployment: String, resolver: R, port: P) -> Self {
        Self {
            enabled: false,
            deployment,
            resolver,
            port,
        }
    }
    /// Explicit opt-in for a trusted host; this alone does not provision any grant.
    pub fn enable(&mut self) {
        self.enabled = true;
    }
    pub async fn handle(&self, credential: &str, request: Value) -> Value {
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        if request.get("jsonrpc") != Some(&json!("2.0"))
            || !request.is_object()
            || !(id.is_null() || id.is_string() || id.is_number())
        {
            return error(Value::Null, -32600, "Invalid request");
        }
        let result =
            tokio::time::timeout(Duration::from_secs(5), self.dispatch(credential, &request)).await;
        match result {
            Ok(Ok(value)) => json!({"jsonrpc":"2.0", "id":id, "result":value}),
            Ok(Err((code, msg))) => error(id, code, msg),
            Err(_) => error(id, -32001, "Read timed out"),
        }
    }
    async fn dispatch(&self, credential: &str, r: &Value) -> Result<Value, (i32, &'static str)> {
        if !self.enabled {
            return Err((-32003, "MCP disabled"));
        }
        let grant = self
            .resolver
            .resolve(credential)
            .await
            .map_err(|_| (-32001, "Unauthorized"))?;
        if self.deployment.is_empty() || grant.deployment != self.deployment {
            return Err((-32001, "Unauthorized"));
        }
        let method = r
            .get("method")
            .and_then(Value::as_str)
            .ok_or((-32600, "Invalid request"))?;
        match method {
            "initialize" => Ok(
                json!({"protocolVersion":"2025-03-26", "capabilities":{"tools":{"listChanged":false}}, "serverInfo":{"name":"accord-readonly-prototype", "version":"0.1.0"}}),
            ),
            "ping" => Ok(json!({})),
            "tools/list" => {
                let mut visible = Vec::new();
                for tool in TOOLS {
                    if grant.scopes.contains(tool) && self.port.permitted(&grant, tool).await {
                        visible.push(tool_definition(tool));
                    }
                }
                Ok(json!({"tools":visible}))
            }
            "tools/call" => {
                let p = r
                    .get("params")
                    .and_then(Value::as_object)
                    .ok_or((-32602, "Invalid parameters"))?;
                if p.keys().any(|k| k != "name" && k != "arguments") {
                    return Err((-32602, "Invalid parameters"));
                }
                let tool = p
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or((-32602, "Invalid parameters"))?;
                if !TOOLS.contains(&tool) {
                    return Err((-32602, "Unknown tool"));
                }
                if !grant.scopes.contains(tool) || !self.port.permitted(&grant, tool).await {
                    return Err((-32001, "Forbidden"));
                }
                let args = Args::parse(tool, p.get("arguments").cloned().unwrap_or(json!({})))
                    .map_err(|_| (-32602, "Invalid parameters"))?;
                let read = self
                    .port
                    .read(tool, &args)
                    .await
                    .map_err(|_| (-32002, "Read unavailable"))?;
                let body = json!({"data": read.data, "metadata": {"retrieved_at_unix":time::OffsetDateTime::now_utc().unix_timestamp(), "source":"ERP domain services", "partial":read.partial, "limit":args.limit(), "snapshot_consistent":false, "source_freshness":"unknown"}});
                let text =
                    serde_json::to_string(&body).map_err(|_| (-32002, "Read unavailable"))?;
                if text.len() > MAX_RESPONSE {
                    return Err((-32002, "Response too large; narrow the request"));
                }
                Ok(json!({"content":[{"type":"text","text":text}], "isError":false}))
            }
            _ => Err((-32601, "Method not found")),
        }
    }
}
fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
fn tool_definition(name: &str) -> Value {
    let (properties, required, description) = match name {
        "erp_order_status" => (
            json!({"order_id":{"type":"string","minLength":1,"maxLength":128}}),
            json!(["order_id"]),
            "Read one production order status",
        ),
        "erp_wip" => (
            json!({"order_id":{"type":"string","minLength":1,"maxLength":128},"limit":{"type":"integer","minimum":1,"maximum":100,"default":25}}),
            json!(["order_id"]),
            "Read bounded active WIP for one order; partial results possible",
        ),
        "erp_warehouse" => (
            json!({"warehouse":{"type":"string","minLength":1,"maxLength":128},"limit":{"type":"integer","minimum":1,"maximum":100,"default":25}}),
            json!(["warehouse"]),
            "Read available finished-goods stock for one warehouse, grouped by order/item/unit; quantities retain original units",
        ),
        _ => (
            json!({"limit":{"type":"integer","minimum":1,"maximum":100,"default":25}}),
            json!([]),
            "Read bounded heterogeneous per-warehouse operational counts, not physical quantities or company-wide financial totals",
        ),
    };
    json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}})
}
