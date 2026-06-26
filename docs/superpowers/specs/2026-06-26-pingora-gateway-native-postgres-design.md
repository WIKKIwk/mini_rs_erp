# Pingora Gateway + Native PostgreSQL Design

## Goal

Replace the Cloudflare Tunnel based public path with a direct public-origin path:

```text
Internet
-> Cloudflare DNS/proxy
-> Fedora public IP:443
-> mini_rs_gateway (Pingora)
-> mini_rs_erp 127.0.0.1:18081
-> PostgreSQL native 127.0.0.1:5432
```

The target keeps the runtime stack in Rust for the public reverse proxy and removes Docker as the PostgreSQL runtime dependency.

## Non-Goals

- Do not change mini ERP HTTP API behavior.
- Do not expose `mini_rs_erp` directly to the internet.
- Do not expose PostgreSQL to the internet.
- Do not keep Docker as the target PostgreSQL runtime.
- Do not remove the existing Cloudflare Tunnel until the direct path is verified.

## Components

### `mini_rs_erp`

- Existing Axum/Tokio service.
- Binds to `127.0.0.1:18081` in production.
- Talks to native PostgreSQL through `MINI_ERP_DATABASE_URL`.
- Remains the owner of business logic, WebSocket/live monitor, sessions, and production workflow.

### Native PostgreSQL

- Runs as a Fedora system service.
- Binds to localhost only.
- Database name: `mini_rs_erp`.
- User: `mini_rs_erp`.
- Backup/restore uses `pg_dump`, `pg_restore`, and systemd-managed service lifecycle.
- Existing Docker database must be backed up before migration.

### `mini_rs_gateway`

- New Rust binary inside this repo.
- Uses Pingora as reverse proxy.
- Public bind: `0.0.0.0:443` when TLS is enabled, or `127.0.0.1:18080` for local validation.
- Upstream: `http://127.0.0.1:18081`.
- Proxies normal HTTP and WebSocket upgrade traffic.
- Provides gateway health/readiness endpoint.
- Enforces request timeout, upstream timeout, connection keepalive, and safe forwarding headers.

### Cloudflare DNS/Proxy

- `A` record points to Fedora public IP.
- Cloudflare proxy can remain enabled.
- Cloudflare Tunnel stays as rollback until direct path passes verification.

### Fedora Firewall

- Open: `80`, `443`.
- Closed externally: `18081`, `5432`, `55433`.
- `mini_rs_erp` and PostgreSQL remain localhost-only.

## Configuration

Gateway env:

```env
MINI_RS_GATEWAY_ADDR=0.0.0.0:443
MINI_RS_GATEWAY_UPSTREAM=http://127.0.0.1:18081
MINI_RS_GATEWAY_HEALTH_PATH=/healthz
MINI_RS_GATEWAY_CONNECT_TIMEOUT_MS=1000
MINI_RS_GATEWAY_UPSTREAM_TIMEOUT_MS=30000
MINI_RS_GATEWAY_TLS_CERT_PATH=
MINI_RS_GATEWAY_TLS_KEY_PATH=
```

ERP env:

```env
MOBILE_API_ADDR=127.0.0.1:18081
MINI_ERP_DATABASE_URL=postgres://mini_rs_erp:<password>@127.0.0.1:5432/mini_rs_erp
```

## TLS Strategy

Phase 1 validates Pingora locally without changing public DNS.

Phase 2 uses Cloudflare proxied DNS. If Cloudflare is set to `Full`, Pingora may run HTTP behind a local system TLS terminator or serve origin HTTP behind Cloudflare. If `Full strict` is required, Pingora must load an origin certificate and key.

Production target is `Full strict` after certificate handling is verified.

## systemd

Services:

```text
postgresql.service
mini-rs-erp.service
mini-rs-gateway.service
```

Ordering:

```text
mini-rs-erp.service After=postgresql.service
mini-rs-gateway.service After=mini-rs-erp.service
```

Both application services use `Restart=always`.

## Migration Plan

1. Backup current Docker PostgreSQL database with `pg_dump`.
2. Install/enable native PostgreSQL on Fedora.
3. Create native database/user.
4. Restore dump into native PostgreSQL.
5. Point `MINI_ERP_DATABASE_URL` to `127.0.0.1:5432`.
6. Restart `mini-rs-erp.service`.
7. Verify local API and DB counts.
8. Build and run `mini_rs_gateway` locally.
9. Verify HTTP/WebSocket proxy behavior through gateway.
10. Enable public firewall/DNS path.
11. Verify domain latency, WebSocket live monitor, and restart recovery.
12. Keep Docker DB stopped but not deleted until rollback window passes.

## Verification

Required checks:

```text
curl http://127.0.0.1:18081/healthz
curl http://127.0.0.1:<gateway-port>/healthz
curl https://mini-rs-erp-test.wspace.sbs/healthz
```

Database checks:

```text
select count(*) from mini_items;
select count(*) from mini_item_groups;
select count(*) from mini_workers;
select count(*) from mini_orders;
select count(*) from mini_production_maps;
```

Runtime checks:

- WebSocket live monitor connects and receives updates.
- 1k WebSocket load test passes without dropped broadcast path.
- Domain latency remains stable.
- `systemctl restart mini-rs-erp.service` recovers.
- `systemctl restart mini-rs-gateway.service` recovers.

## Rollback

Rollback keeps current Cloudflare Tunnel and Docker PostgreSQL untouched until the new path is verified.

Rollback steps:

1. Point DNS back to Cloudflare Tunnel route or previous hostname path.
2. Restart old `cloudflared-mini-rs-erp.service`.
3. Point `MINI_ERP_DATABASE_URL` back to Docker PostgreSQL if native migration fails.
4. Restore from the pre-migration dump if data mismatch is detected.

## Acceptance Criteria

- `mini_rs_gateway` builds in release mode.
- `mini_rs_gateway` proxies HTTP and WebSocket traffic to `mini_rs_erp`.
- Native PostgreSQL contains the migrated Fedora data.
- Docker PostgreSQL is not required for normal runtime.
- Public domain health check is stable.
- `mini_rs_erp` remains bound to localhost.
- PostgreSQL remains bound to localhost.
