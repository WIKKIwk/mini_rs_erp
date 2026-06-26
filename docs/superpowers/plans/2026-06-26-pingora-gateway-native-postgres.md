# Pingora Gateway Native PostgreSQL Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Rust/Pingora public gateway and migrate Fedora runtime from Docker PostgreSQL to native PostgreSQL.

**Architecture:** `mini_rs_gateway` is a new Rust binary in this repo. It reverse-proxies HTTP and WebSocket requests to `mini_rs_erp` on localhost, while `mini_rs_erp` moves to native PostgreSQL on localhost. Cloudflare Tunnel stays available as rollback until the new path passes verification.

**Tech Stack:** Rust 2024, Pingora `0.8.1`, Axum backend, native PostgreSQL, systemd, firewalld, Cloudflare DNS/proxy.

---

## File Structure

- Modify `Cargo.toml`: add Pingora dependencies and declare `mini_rs_gateway` binary.
- Create `src/bin/mini_rs_gateway.rs`: Pingora reverse proxy binary.
- Create `src/gateway_config.rs`: environment parsing for gateway bind/upstream/TLS/timeout settings.
- Modify `src/lib.rs`: expose `gateway_config`.
- Create `src/gateway_config_tests.rs`: config parsing tests.
- Modify `.env.example`: document gateway env.
- Create `deploy/systemd/mini-rs-gateway.service`: Fedora user service template.
- Create `deploy/systemd/mini-rs-erp-native-postgres.env.example`: native Postgres env template.
- Create `docs/deploy/pingora-native-postgres-fedora.md`: deploy and rollback runbook.

## Task 1: Gateway Configuration Module

**Files:**
- Create: `src/gateway_config.rs`
- Create: `src/gateway_config_tests.rs`
- Modify: `src/lib.rs`
- Modify: `Cargo.toml`

- [ ] **Step 1: Add test module declaration**

Add this to `src/lib.rs`:

```rust
pub mod gateway_config;

#[cfg(test)]
mod gateway_config_tests;
```

- [ ] **Step 2: Write config tests**

Create `src/gateway_config_tests.rs`:

```rust
use std::net::SocketAddr;
use std::time::Duration;

use crate::gateway_config::GatewayConfig;

#[test]
fn gateway_config_uses_defaults() {
    let config = GatewayConfig::from_pairs(std::iter::empty::<(&str, &str)>()).unwrap();

    assert_eq!(
        config.bind_addr,
        "127.0.0.1:18080".parse::<SocketAddr>().unwrap()
    );
    assert_eq!(config.upstream_addr, "127.0.0.1:18081");
    assert_eq!(config.upstream_host, "127.0.0.1");
    assert_eq!(config.health_path, "/healthz");
    assert_eq!(config.connect_timeout, Duration::from_millis(1000));
    assert_eq!(config.upstream_timeout, Duration::from_millis(30000));
    assert!(config.tls_cert_path.is_none());
    assert!(config.tls_key_path.is_none());
}

#[test]
fn gateway_config_reads_env_pairs() {
    let config = GatewayConfig::from_pairs([
        ("MINI_RS_GATEWAY_ADDR", "0.0.0.0:8443"),
        ("MINI_RS_GATEWAY_UPSTREAM", "127.0.0.1:18081"),
        ("MINI_RS_GATEWAY_UPSTREAM_HOST", "mini-rs-erp-test.wspace.sbs"),
        ("MINI_RS_GATEWAY_HEALTH_PATH", "/ready"),
        ("MINI_RS_GATEWAY_CONNECT_TIMEOUT_MS", "1500"),
        ("MINI_RS_GATEWAY_UPSTREAM_TIMEOUT_MS", "45000"),
        ("MINI_RS_GATEWAY_TLS_CERT_PATH", "/etc/ssl/cert.pem"),
        ("MINI_RS_GATEWAY_TLS_KEY_PATH", "/etc/ssl/key.pem"),
    ])
    .unwrap();

    assert_eq!(
        config.bind_addr,
        "0.0.0.0:8443".parse::<SocketAddr>().unwrap()
    );
    assert_eq!(config.upstream_addr, "127.0.0.1:18081");
    assert_eq!(config.upstream_host, "mini-rs-erp-test.wspace.sbs");
    assert_eq!(config.health_path, "/ready");
    assert_eq!(config.connect_timeout, Duration::from_millis(1500));
    assert_eq!(config.upstream_timeout, Duration::from_millis(45000));
    assert_eq!(config.tls_cert_path.as_deref(), Some("/etc/ssl/cert.pem"));
    assert_eq!(config.tls_key_path.as_deref(), Some("/etc/ssl/key.pem"));
}

#[test]
fn gateway_config_rejects_invalid_bind_addr() {
    let error = GatewayConfig::from_pairs([("MINI_RS_GATEWAY_ADDR", "bad addr")]).unwrap_err();
    assert!(error.contains("MINI_RS_GATEWAY_ADDR"));
}
```

- [ ] **Step 3: Run failing tests**

Run:

```bash
cargo test --locked gateway_config
```

Expected: fails because `gateway_config` module is missing.

- [ ] **Step 4: Implement config module**

Create `src/gateway_config.rs`:

```rust
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayConfig {
    pub bind_addr: SocketAddr,
    pub upstream_addr: String,
    pub upstream_host: String,
    pub health_path: String,
    pub connect_timeout: Duration,
    pub upstream_timeout: Duration,
    pub tls_cert_path: Option<String>,
    pub tls_key_path: Option<String>,
}

impl GatewayConfig {
    pub fn from_env() -> Result<Self, String> {
        Self::from_pairs(std::env::vars())
    }

    pub fn from_pairs<I, K, V>(pairs: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        let values = pairs
            .into_iter()
            .map(|(key, value)| (key.as_ref().to_string(), value.as_ref().to_string()))
            .collect::<BTreeMap<_, _>>();

        let bind_raw = value_or(&values, "MINI_RS_GATEWAY_ADDR", "127.0.0.1:18080");
        let bind_addr = bind_raw
            .parse::<SocketAddr>()
            .map_err(|_| format!("invalid MINI_RS_GATEWAY_ADDR: {bind_raw}"))?;
        let upstream_addr = value_or(&values, "MINI_RS_GATEWAY_UPSTREAM", "127.0.0.1:18081");
        let upstream_host = value_or(&values, "MINI_RS_GATEWAY_UPSTREAM_HOST", "127.0.0.1");
        let health_path = value_or(&values, "MINI_RS_GATEWAY_HEALTH_PATH", "/healthz");
        let connect_timeout = Duration::from_millis(parse_millis(
            &values,
            "MINI_RS_GATEWAY_CONNECT_TIMEOUT_MS",
            1000,
        )?);
        let upstream_timeout = Duration::from_millis(parse_millis(
            &values,
            "MINI_RS_GATEWAY_UPSTREAM_TIMEOUT_MS",
            30000,
        )?);

        Ok(Self {
            bind_addr,
            upstream_addr,
            upstream_host,
            health_path,
            connect_timeout,
            upstream_timeout,
            tls_cert_path: optional_value(&values, "MINI_RS_GATEWAY_TLS_CERT_PATH"),
            tls_key_path: optional_value(&values, "MINI_RS_GATEWAY_TLS_KEY_PATH"),
        })
    }
}

fn value_or(values: &BTreeMap<String, String>, key: &str, fallback: &str) -> String {
    values
        .get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn optional_value(values: &BTreeMap<String, String>, key: &str) -> Option<String> {
    values
        .get(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_millis(
    values: &BTreeMap<String, String>,
    key: &str,
    fallback: u64,
) -> Result<u64, String> {
    let raw = value_or(values, key, &fallback.to_string());
    raw.parse::<u64>()
        .filter(|value| *value > 0)
        .map_err(|_| format!("invalid {key}: {raw}"))
}
```

- [ ] **Step 5: Run tests**

Run:

```bash
cargo test --locked gateway_config
```

Expected: all `gateway_config` tests pass.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml src/lib.rs src/gateway_config.rs src/gateway_config_tests.rs
git commit -m "Add Pingora gateway config"
```

## Task 2: Pingora Gateway Binary

**Files:**
- Modify: `Cargo.toml`
- Create: `src/bin/mini_rs_gateway.rs`

- [ ] **Step 1: Add dependencies and binary**

Modify `Cargo.toml`:

```toml
[[bin]]
name = "mini_rs_gateway"
path = "src/bin/mini_rs_gateway.rs"

[dependencies]
pingora = { version = "0.8.1", features = ["proxy", "rustls"] }
pingora-core = { version = "0.8.1", features = ["rustls"] }
pingora-http = "0.8.1"
pingora-proxy = { version = "0.8.1", features = ["rustls"] }
bytes = "1"
```

Keep existing dependencies unchanged.

- [ ] **Step 2: Create gateway binary**

Create `src/bin/mini_rs_gateway.rs`:

```rust
use async_trait::async_trait;
use bytes::Bytes;
use mini_rs_erp::gateway_config::GatewayConfig;
use pingora_core::listeners::tls::TlsSettings;
use pingora_core::server::configuration::Opt;
use pingora_core::server::Server;
use pingora_core::services::listening::Service;
use pingora_core::upstreams::peer::HttpPeer;
use pingora_core::Result;
use pingora_http::ResponseHeader;
use pingora_proxy::{ProxyHttp, Session};

#[derive(Clone)]
struct MiniRsGateway {
    config: GatewayConfig,
}

#[async_trait]
impl ProxyHttp for MiniRsGateway {
    type CTX = ();

    fn new_ctx(&self) -> Self::CTX {}

    async fn request_filter(&self, session: &mut Session, _ctx: &mut Self::CTX) -> Result<bool> {
        if session.req_header().uri.path() == self.config.health_path {
            let body = Bytes::from_static(br#"{"ok":true,"service":"mini_rs_gateway"}"#);
            session.respond_error_with_body(200, body).await?;
            return Ok(true);
        }
        Ok(false)
    }

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        Ok(Box::new(HttpPeer::new(
            self.config.upstream_addr.as_str(),
            false,
            self.config.upstream_host.clone(),
        )))
    }

    async fn upstream_request_filter(
        &self,
        session: &mut Session,
        upstream_request: &mut pingora_http::RequestHeader,
        _ctx: &mut Self::CTX,
    ) -> Result<()> {
        upstream_request.insert_header("Host", &self.config.upstream_host)?;
        if let Some(peer) = session.client_addr() {
            upstream_request.insert_header("X-Forwarded-For", peer.to_string())?;
        }
        upstream_request.insert_header("X-Forwarded-Proto", "https")?;
        Ok(())
    }

    async fn response_filter(
        &self,
        _session: &mut Session,
        upstream_response: &mut ResponseHeader,
        _ctx: &mut Self::CTX,
    ) -> Result<()>
    where
        Self::CTX: Send + Sync,
    {
        upstream_response.insert_header("Server", "mini-rs-gateway")?;
        upstream_response.remove_header("alt-svc");
        Ok(())
    }
}

fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = GatewayConfig::from_env().expect("valid gateway config");
    let opt = Opt::parse_args();
    let mut server = Server::new(Some(opt)).expect("pingora server");
    server.bootstrap();

    let mut proxy = pingora_proxy::http_proxy_service(
        &server.configuration,
        MiniRsGateway {
            config: config.clone(),
        },
    );

    let bind_addr = config.bind_addr.to_string();
    match (config.tls_cert_path.as_deref(), config.tls_key_path.as_deref()) {
        (Some(cert), Some(key)) => {
            let mut tls = TlsSettings::intermediate(cert, key).expect("valid tls settings");
            tls.enable_h2();
            proxy
                .add_tls_with_settings(&bind_addr, None, tls)
                .expect("tls listener");
        }
        _ => proxy.add_tcp(&bind_addr),
    }

    let mut metrics = Service::prometheus_http_service();
    metrics.add_tcp("127.0.0.1:19091");

    server.add_service(proxy);
    server.add_service(metrics);
    server.run_forever();
}
```

- [ ] **Step 3: Build gateway**

Run:

```bash
cargo build --locked --release --bin mini_rs_gateway
```

Expected: build succeeds.

- [ ] **Step 4: Run backend and gateway locally**

Run backend in one terminal:

```bash
MOBILE_API_ADDR=127.0.0.1:18081 cargo run --release --bin mini_rs_erp
```

Run gateway in another terminal:

```bash
MINI_RS_GATEWAY_ADDR=127.0.0.1:18080 \
MINI_RS_GATEWAY_UPSTREAM=127.0.0.1:18081 \
MINI_RS_GATEWAY_UPSTREAM_HOST=127.0.0.1 \
cargo run --release --bin mini_rs_gateway
```

- [ ] **Step 5: Verify local proxy**

Run:

```bash
curl -sS http://127.0.0.1:18080/healthz
```

Expected body:

```json
{"ok":true,"service":"mini_rs_gateway"}
```

Run:

```bash
curl -sS http://127.0.0.1:18081/healthz
curl -sS http://127.0.0.1:18080/v1/mobile/admin/server-monitor
```

Expected: backend health succeeds; proxied endpoint returns the same auth behavior as direct backend.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml src/bin/mini_rs_gateway.rs
git commit -m "Add Pingora gateway binary"
```

## Task 3: Gateway Env and systemd Templates

**Files:**
- Modify: `.env.example`
- Create: `deploy/systemd/mini-rs-gateway.service`
- Create: `deploy/systemd/mini-rs-erp-native-postgres.env.example`

- [ ] **Step 1: Extend `.env.example`**

Append:

```env
# Pingora gateway.
MINI_RS_GATEWAY_ADDR=127.0.0.1:18080
MINI_RS_GATEWAY_UPSTREAM=127.0.0.1:18081
MINI_RS_GATEWAY_UPSTREAM_HOST=127.0.0.1
MINI_RS_GATEWAY_HEALTH_PATH=/healthz
MINI_RS_GATEWAY_CONNECT_TIMEOUT_MS=1000
MINI_RS_GATEWAY_UPSTREAM_TIMEOUT_MS=30000
MINI_RS_GATEWAY_TLS_CERT_PATH=
MINI_RS_GATEWAY_TLS_KEY_PATH=
```

- [ ] **Step 2: Create gateway systemd template**

Create `deploy/systemd/mini-rs-gateway.service`:

```ini
[Unit]
Description=Mini RS Gateway
Wants=mini-rs-erp.service
After=mini-rs-erp.service

[Service]
Type=simple
WorkingDirectory=/home/wikki/mini_rs_erp_deploy/src
EnvironmentFile=/home/wikki/mini_rs_erp_deploy/src/.env
ExecStartPre=/usr/bin/curl -fsS http://127.0.0.1:18081/healthz
ExecStart=/home/wikki/mini_rs_erp_deploy/src/target/release/mini_rs_gateway
Restart=always
RestartSec=3
LimitNOFILE=65535

[Install]
WantedBy=default.target
```

- [ ] **Step 3: Create native Postgres env template**

Create `deploy/systemd/mini-rs-erp-native-postgres.env.example`:

```env
MOBILE_API_ADDR=127.0.0.1:18081
MINI_ERP_DATABASE_URL=postgres://mini_rs_erp:${MINI_RS_ERP_DB_PASSWORD}@127.0.0.1:5432/mini_rs_erp
MOBILE_API_SESSION_STORE_BACKEND=lmdb
MOBILE_API_SESSION_LMDB_PATH=data/mobile_sessions.lmdb
MOBILE_API_PROFILE_STORE_BACKEND=lmdb
MOBILE_API_PROFILE_LMDB_PATH=data/mobile_profile_prefs.lmdb
MOBILE_API_PUSH_TOKEN_STORE_BACKEND=lmdb
MOBILE_API_PUSH_TOKEN_LMDB_PATH=data/mobile_push_tokens.lmdb
MOBILE_API_LOCAL_STORE_ALLOW_JSON_FALLBACK=0
MINI_RS_GATEWAY_ADDR=127.0.0.1:18080
MINI_RS_GATEWAY_UPSTREAM=127.0.0.1:18081
MINI_RS_GATEWAY_UPSTREAM_HOST=127.0.0.1
```

- [ ] **Step 4: Verify files**

Run:

```bash
test -f deploy/systemd/mini-rs-gateway.service
test -f deploy/systemd/mini-rs-erp-native-postgres.env.example
cargo fmt --check
```

Expected: all commands pass.

- [ ] **Step 5: Commit**

```bash
git add .env.example deploy/systemd/mini-rs-gateway.service deploy/systemd/mini-rs-erp-native-postgres.env.example
git commit -m "Add gateway deployment templates"
```

## Task 4: Fedora Native PostgreSQL Migration

**Files:**
- Create: `docs/deploy/pingora-native-postgres-fedora.md`

- [ ] **Step 1: Write deploy runbook**

Create `docs/deploy/pingora-native-postgres-fedora.md` with these commands:

```markdown
# Fedora Pingora + Native PostgreSQL Deploy

## Backup Docker PostgreSQL

```bash
TS=$(date +%Y%m%d_%H%M%S)
mkdir -p /home/wikki/mini_rs_erp_deploy/backups/native_pg_$TS
docker exec mini-rs-erp-postgres pg_dump -U mini_rs_erp -d mini_rs_erp \
  | gzip > /home/wikki/mini_rs_erp_deploy/backups/native_pg_$TS/mini_rs_erp.sql.gz
grep '^MINI_ERP_DATABASE_URL=' /home/wikki/mini_rs_erp_deploy/src/.env \
  > /home/wikki/mini_rs_erp_deploy/backups/native_pg_$TS/docker_database_url.env
```

## Install native PostgreSQL

```bash
sudo dnf install -y postgresql-server postgresql-contrib
sudo postgresql-setup --initdb
sudo systemctl enable --now postgresql
```

## Create database and user

```bash
read -r -s -p 'Native mini_rs_erp PostgreSQL password: ' MINI_RS_ERP_DB_PASSWORD
printf '\n'
sudo -u postgres psql <<'SQL'
create user mini_rs_erp;
create database mini_rs_erp owner mini_rs_erp;
SQL
sudo -u postgres psql -v password="$MINI_RS_ERP_DB_PASSWORD" <<'SQL'
alter user mini_rs_erp with password :'password';
SQL
```

## Restore dump

```bash
gunzip -c /home/wikki/mini_rs_erp_deploy/backups/native_pg_$TS/mini_rs_erp.sql.gz \
  | sudo -u postgres psql -d mini_rs_erp
```

## Point mini ERP to native PostgreSQL

```bash
grep -v '^MINI_ERP_DATABASE_URL=' /home/wikki/mini_rs_erp_deploy/src/.env > /tmp/mini-rs-env
printf 'MINI_ERP_DATABASE_URL=postgres://mini_rs_erp:%s@127.0.0.1:5432/mini_rs_erp\n' "$MINI_RS_ERP_DB_PASSWORD" >> /tmp/mini-rs-env
cp /tmp/mini-rs-env /home/wikki/mini_rs_erp_deploy/src/.env
systemctl --user restart mini-rs-erp.service
curl -fsS http://127.0.0.1:18081/healthz
```

## Install gateway service

```bash
cp deploy/systemd/mini-rs-gateway.service /home/wikki/.config/systemd/user/mini-rs-gateway.service
systemctl --user daemon-reload
systemctl --user enable --now mini-rs-gateway.service
curl -fsS http://127.0.0.1:18080/healthz
```

## Verify data counts

```bash
psql "postgres://mini_rs_erp:${MINI_RS_ERP_DB_PASSWORD}@127.0.0.1:5432/mini_rs_erp" <<'SQL'
select 'mini_items', count(*) from mini_items;
select 'mini_item_groups', count(*) from mini_item_groups;
select 'mini_workers', count(*) from mini_workers;
select 'mini_orders', count(*) from mini_orders;
select 'mini_production_maps', count(*) from mini_production_maps;
SQL
```

## Rollback

```bash
systemctl --user stop mini-rs-gateway.service
systemctl --user restart cloudflared-mini-rs-erp.service
cp /home/wikki/mini_rs_erp_deploy/backups/native_pg_$TS/docker_database_url.env /tmp/docker_database_url.env
grep -v '^MINI_ERP_DATABASE_URL=' /home/wikki/mini_rs_erp_deploy/src/.env > /tmp/mini-rs-env
cat /tmp/docker_database_url.env >> /tmp/mini-rs-env
cp /tmp/mini-rs-env /home/wikki/mini_rs_erp_deploy/src/.env
systemctl --user restart mini-rs-erp.service
```
```

- [ ] **Step 2: Commit**

```bash
git add docs/deploy/pingora-native-postgres-fedora.md
git commit -m "Document Fedora native Postgres deploy"
```

## Task 5: Build and Fedora Deploy

**Files:**
- No source changes.

- [ ] **Step 1: Build release binaries on amd64**

Run in OrbStack amd64 environment:

```bash
cargo build --locked --release --bin mini_rs_erp --bin mini_rs_gateway
```

Expected binaries:

```text
target/release/mini_rs_erp
target/release/mini_rs_gateway
```

- [ ] **Step 2: Upload binaries to Fedora**

```bash
scp target/release/mini_rs_erp target/release/mini_rs_gateway \
  wikki@100.92.208.128:/home/wikki/mini_rs_erp_deploy/src/target/release/
```

- [ ] **Step 3: Backup Fedora DB**

```bash
ssh wikki@100.92.208.128 \
  'TS=$(date +%Y%m%d_%H%M%S); mkdir -p /home/wikki/mini_rs_erp_deploy/backups/pre_native_pg_$TS; docker exec mini-rs-erp-postgres pg_dump -U mini_rs_erp -d mini_rs_erp | gzip > /home/wikki/mini_rs_erp_deploy/backups/pre_native_pg_$TS/mini_rs_erp.sql.gz; echo /home/wikki/mini_rs_erp_deploy/backups/pre_native_pg_$TS'
```

- [ ] **Step 4: Stop before risky migration**

Stop here and confirm with Wikki before installing native PostgreSQL or modifying Fedora database runtime.

## Task 6: Public Cutover Verification

**Files:**
- No source changes.

- [ ] **Step 1: Verify local gateway**

```bash
ssh wikki@100.92.208.128 'curl -fsS http://127.0.0.1:18080/healthz'
```

Expected:

```json
{"ok":true,"service":"mini_rs_gateway"}
```

- [ ] **Step 2: Verify domain after DNS/firewall cutover**

```bash
for i in 1 2 3 4 5; do
  curl -4 -k -sS -o /dev/null \
    -w "domain_$i http=%{http_code} total=%{time_total}\n" \
    https://mini-rs-erp-test.wspace.sbs/healthz
done
```

Expected: HTTP `200` and stable response times.

- [ ] **Step 3: Verify WebSocket live path**

Run:

```bash
WS_CLIENTS=1000 \
BASE_URL='https://mini-rs-erp-test.wspace.sbs' \
WS_URL='wss://mini-rs-erp-test.wspace.sbs/v1/mobile/admin/production-maps/live' \
node - <<'NODE'
const total = Number(process.env.WS_CLIENTS || '1000');
const baseHttpUrl = process.env.BASE_URL || '';
const baseUrl = process.env.WS_URL || '';
if (!baseHttpUrl) {
  console.error('BASE_URL is required');
  process.exit(2);
}
if (!baseUrl) {
  console.error('WS_URL is required');
  process.exit(2);
}
if (typeof WebSocket !== 'function') {
  console.error('Node global WebSocket is not available');
  process.exit(2);
}
const login = await fetch(`${baseHttpUrl}/v1/mobile/auth/login`, {
  method: 'POST',
  headers: { 'content-type': 'application/json' },
  body: JSON.stringify({ phone: '+998880000000', code: '19621978' }),
});
if (!login.ok) {
  console.error(`admin login failed: ${login.status}`);
  process.exit(1);
}
const loginBody = await login.json();
const token = loginBody.token;
if (!token) {
  console.error('admin login did not return token');
  process.exit(1);
}
const url = `${baseUrl}?token=${encodeURIComponent(token)}`;
let opened = 0;
let messaged = 0;
let closed = 0;
let failed = 0;
const sockets = [];
const done = new Promise((resolve) => {
  const timeout = setTimeout(resolve, 30000);
  for (let i = 0; i < total; i += 1) {
    const ws = new WebSocket(url);
    sockets.push(ws);
    ws.onopen = () => {
      opened += 1;
    };
    ws.onmessage = () => {
      messaged += 1;
      if (messaged === total) {
        clearTimeout(timeout);
        resolve();
      }
    };
    ws.onerror = () => {
      failed += 1;
    };
    ws.onclose = () => {
      closed += 1;
    };
  }
});
await done;
for (const ws of sockets) {
  if (ws.readyState === WebSocket.OPEN) {
    ws.close();
  }
}
await new Promise((resolve) => setTimeout(resolve, 1000));
console.log(JSON.stringify({ total, opened, messaged, failed, closed }));
if (opened !== total || messaged !== total || failed !== 0) {
  process.exit(1);
}
NODE
```

Expected: 1k WebSocket clients connect and receive live updates without persistent disconnects.

- [ ] **Step 4: Commit verification note**

Append measured numbers to `docs/deploy/pingora-native-postgres-fedora.md`, then commit:

```bash
git add docs/deploy/pingora-native-postgres-fedora.md
git commit -m "Record Pingora gateway deploy verification"
```
