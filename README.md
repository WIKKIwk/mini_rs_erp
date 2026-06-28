# Mini RS ERP

`mini_rs_erp` is the Accord mini ERP backend. It owns the operational ERP
workflows that the mobile app depends on: authentication, roles, production map
queues, WIP QR tracking, finished-goods receiving, Qolip inventory, warehouse
flows, admin operations, push notifications, profile state, and runtime
monitoring.

The mobile app is a client of this service. The HTTP paths still use the
`/v1/mobile/*` contract because existing mobile releases depend on that shape,
but this repository is no longer just a mobile service. It is the mini ERP core
runtime.

## Current Scope

The service currently covers these business areas:

| Area | Responsibility |
| --- | --- |
| Auth and sessions | Login, bearer sessions, role inference, access codes. |
| Role and capability control | Built-in roles, custom capability packages, principal assignments. |
| Production map | Order maps, apparatus queue sequence, queue policies, start/pause/resume/complete. |
| WIP and QR flow | Progress batches, QR lookup/reporting, previous-stage validation, lineage. |
| Finished goods | Final completed batch receiving into a warehouse. |
| Raw material control | Material assignment and scan validation before production actions. |
| Qolip | Warehouse/block/location inventory, cell QR, checkout, return, movement. |
| Warehouse flow | Dashboard, pending/history/archive, confirmations, unannounced receipt, customer issue flows. |
| GScale/RPS printing | Progress labels, QR labels, driver-first print flow, receipt drafts. |
| Admin | Settings, users, roles, suppliers/customers/items, activity, monitoring. |
| Push/profile/local state | FCM token storage, push dispatch, profile prefs, avatar storage. |

`werka` is still used in routes, roles, config, and older internal module names
as a legacy compatibility name for the warehouse/operator flow. Do not rename it
casually: it is part of the mobile contract and appears in persisted keys such
as `werka:werka`.

## Runtime Model

```mermaid
flowchart TB
    Mobile["Accord Mobile App"]
    HTTP["Axum HTTP API<br/>/healthz, /v1/mobile/*"]
    Core["Mini ERP Domain Services"]
    DB["PostgreSQL<br/>ERP production state"]
    Local["LMDB local state<br/>sessions, profile prefs, push tokens, admin state"]
    Push["Firebase Cloud Messaging"]
    AI["Gemini Vision<br/>optional AI search"]
    Print["GScale / RPS print drivers"]
    Tunnel["Cloudflare Tunnel / domain"]

    Mobile --> Tunnel
    Tunnel --> HTTP
    HTTP --> Core
    Core --> DB
    Core --> Local
    Core --> Push
    Core --> AI
    Core --> Print
```

### Persistence Boundary

Production mini ERP state belongs in PostgreSQL. The service should not silently
pretend to run production ERP workflows without a configured database.

`MINI_ERP_DATABASE_URL` enables PostgreSQL-backed stores for ERP state. Local
LMDB stores are still used for small runtime state:

- sessions;
- profile preferences;
- push tokens;
- admin supplier/customer state and generated codes.

JSON local stores are legacy migration or emergency rollback paths, not the
normal production path.

## Quick Start

```bash
cp .env.example .env
cargo fmt --check
cargo test --locked
cargo run
```

For production-like startup, configure at least:

```env
MOBILE_API_ADDR=0.0.0.0:8081
MINI_ERP_DATABASE_URL=postgres://mini_rs_erp:secret@127.0.0.1:5432/mini_rs_erp
MOBILE_API_LOCAL_STORE_ALLOW_JSON_FALLBACK=0
RUST_LOG=info
```

Health check:

```bash
curl http://127.0.0.1:8081/healthz
```

Expected:

```json
{"ok":true}
```

## Domain Bootstrap

For a test or production instance behind a Cloudflare-managed hostname:

```bash
make up-domain DOMAIN=mini-rs-erp-dev.wspace.sbs
```

For production, require the database URL explicitly:

```bash
MINI_ERP_DATABASE_URL=postgres://mini_rs_erp:secret@db.internal:5432/mini_rs_erp \
REQUIRE_DATABASE_URL=1 \
make up-domain DOMAIN=mini-rs-erp.example.com
```

Useful knobs:

| Variable | Default | Description |
| --- | --- | --- |
| `DOMAIN` | required | Public hostname to publish. |
| `PORT` | `18081` | Local mini ERP port. |
| `CORE_URL` | `http://127.0.0.1:$PORT` | Local service URL exposed through the tunnel. |
| `MOBILE_API_ADDR` | `127.0.0.1:$PORT` | Bind address passed to the service. |
| `TUNNEL_NAME` | hostname-derived | Cloudflare Tunnel name. |
| `REQUIRE_DATABASE_URL` | `0` | Set to `1` for production enforcement. |
| `BUILD_RELEASE` | `1` | Build the release binary before starting. |
| `ROUTE_DNS` | `1` | Route the hostname to the tunnel. |
| `STATE_ROOT` | `garbage/domain` | Runtime state, logs, pid files, and tunnel config. |

Stop only local processes started for a hostname:

```bash
make stop-domain DOMAIN=mini-rs-erp-dev.wspace.sbs
```

## Main Workflows

### Production Map

Production maps define the operation graph for an order. The queue layer turns
those maps into apparatus-specific work queues.

Core behavior:

- apparatus sequence and visible-order filtering;
- strict/free-pick queue policies;
- `start`, `pause`, `resume`, and `complete` actions;
- progress batch creation with QR payloads;
- previous-stage WIP validation for downstream starts;
- completed order status detail and audit data;
- finished-goods receiving for final-stage output.

Important modules:

- `src/core/production_map`;
- `src/http/handlers/admin/production_maps`;
- `src/db/postgres_production_map.rs`.

### Qolip

Qolip handles physical location inventory for molds/forms.

Core behavior:

- warehouses and blocks;
- item/product specs;
- location upsert and movement;
- deterministic cell QR;
- checkout to workers;
- return to a cell;
- stock identity checks to prevent merging incompatible locations.

Important modules:

- `src/core/qolip`;
- `src/http/handlers/qolip.rs`;
- `src/db/postgres_qolip.rs`.

### Warehouse Flow

The legacy `werka` flow is the warehouse/operator workflow.

Core behavior:

- summary/home/pending/history/archive;
- supplier/customer directory and item lookup;
- receipt confirmation;
- unannounced supplier draft creation;
- customer issue creation;
- notification detail/comment flows;
- optional image-based AI search.

Important modules:

- `src/core/werka`;
- `src/http/handlers/werka.rs`;
- `src/ai/werka_search.rs`.

## API Surface

The router keeps the mobile-compatible paths. High-level groups:

| Group | Routes |
| --- | --- |
| Health/auth | `/healthz`, `/v1/mobile/auth/login`, `/v1/mobile/auth/logout`, `/v1/mobile/me` |
| Profile | `/v1/mobile/profile`, `/v1/mobile/profile/avatar`, `/v1/mobile/profile/avatar/view` |
| Push | `/v1/mobile/push/token` |
| Customer | `/v1/mobile/customer/*` |
| Supplier | `/v1/mobile/supplier/*` |
| Warehouse legacy | `/v1/mobile/werka/*` |
| Qolip | `/v1/mobile/qolip/*` |
| GScale/RPS | `/v1/mobile/gscale/*`, `/v1/mobile/rps/*` |
| Admin | `/v1/mobile/admin/*` |

The API contract is intentionally conservative. Existing mobile clients rely on
the route names, method behavior, auth order, response shapes, and error bodies.
Refactors should preserve contract behavior unless mobile changes are planned at
the same time.

## Configuration

Common runtime variables:

| Variable | Default | Description |
| --- | --- | --- |
| `MOBILE_API_ADDR` | `:8081` | Bind address. `:8081` is normalized to `0.0.0.0:8081`. |
| `MINI_ERP_DATABASE_URL` | empty | PostgreSQL URL for mini ERP state. Required for production ERP workflows. |
| `MINI_ERP_HTTP_TIMEOUT_SECONDS` | `15` | HTTP client timeout baseline. |
| `MINI_ERP_DEFAULT_TARGET_WAREHOUSE` | empty | Default target warehouse setting. |
| `MINI_ERP_DEFAULT_UOM` | `Kg` | Default unit of measure. |
| `MOBILE_API_LOCAL_STORE_ALLOW_JSON_FALLBACK` | `0` | Emergency fallback only. Keep `0` in production. |
| `MOBILE_API_SESSION_STORE_BACKEND` | `lmdb` | Session backend. |
| `MOBILE_API_PROFILE_STORE_BACKEND` | `lmdb` | Profile prefs backend. |
| `MOBILE_API_PUSH_TOKEN_STORE_BACKEND` | `lmdb` | Push token backend. |
| `MOBILE_API_ADMIN_SUPPLIER_STORE_BACKEND` | `lmdb` | Admin local state backend. |
| `MOBILE_API_SESSION_TTL_HOURS` | `720` | Bearer session TTL. |
| `MOBILE_DEV_SUPPLIER_PREFIX` | `10` | Supplier login code prefix. |
| `MOBILE_DEV_WERKA_PREFIX` | `20` | Warehouse/werka login code prefix. |
| `MOBILE_DEV_WERKA_CODE` | empty | Warehouse/werka login code. |
| `MOBILE_DEV_WERKA_NAME` | `Werka` | Warehouse/werka display name. |
| `FCM_SERVICE_ACCOUNT_PATH` | auto-discover | Firebase service account JSON. |
| `GEMINI_API_KEY` | empty | Enables AI image search. |
| `GEMINI_VISION_MODEL` | provider default | Optional Gemini model override. |
| `RUST_LOG` | unset | Tracing filter, for example `info`. |

Example:

```env
MOBILE_API_ADDR=0.0.0.0:8081
MINI_ERP_DATABASE_URL=postgres://mini_rs_erp:secret@127.0.0.1:5432/mini_rs_erp
MINI_ERP_DEFAULT_TARGET_WAREHOUSE=Stores - CH
MINI_ERP_DEFAULT_UOM=Kg
MOBILE_API_LOCAL_STORE_ALLOW_JSON_FALLBACK=0

MOBILE_API_SESSION_STORE_BACKEND=lmdb
MOBILE_API_SESSION_LMDB_PATH=data/mobile_sessions.lmdb
MOBILE_API_PROFILE_STORE_BACKEND=lmdb
MOBILE_API_PROFILE_LMDB_PATH=data/mobile_profile_prefs.lmdb
MOBILE_API_PUSH_TOKEN_STORE_BACKEND=lmdb
MOBILE_API_PUSH_TOKEN_LMDB_PATH=data/mobile_push_tokens.lmdb
MOBILE_API_ADMIN_SUPPLIER_STORE_BACKEND=lmdb
MOBILE_API_ADMIN_SUPPLIER_LMDB_PATH=data/mobile_admin_suppliers.lmdb

MOBILE_DEV_SUPPLIER_PREFIX=10
MOBILE_DEV_WERKA_PREFIX=20
MOBILE_DEV_WERKA_CODE=20ABCDEF1234
MOBILE_DEV_WERKA_NAME=Werka

FCM_SERVICE_ACCOUNT_PATH=/secure/firebase-adminsdk.json
GEMINI_API_KEY=
RUST_LOG=info
```

## Build and Run

Development:

```bash
cargo run
```

Release:

```bash
cargo build --release
./target/release/mini_rs_erp
```

Gateway binary:

```bash
cargo build --release --bin mini_rs_gateway
./target/release/mini_rs_gateway
```

## Testing

Run the same core checks expected before pushing:

```bash
cargo fmt --check
cargo test --locked
cargo clippy --all-targets --all-features -- -D warnings
```

Focused suites:

```bash
cargo test production_map
cargo test qolip
cargo test werka
cargo test admin
cargo test push
```

Architecture guard:

- production Rust source files are kept under the repository file-size policy;
- tests can be larger because they carry behavior/contract coverage;
- refactors should move logic into named modules instead of growing monolithic
  handler, service, or store files.

## Repository Layout

```text
src/
  ai/                 Optional AI image search integration.
  app.rs              Runtime dependency wiring.
  config.rs           Environment configuration and .env persistence.
  core/               Domain models, ports, services, and business rules.
    admin/            Admin settings, users, roles, monitor, mutations.
    auth/             Login, access codes, principals.
    production_map/   Production maps, apparatus queues, WIP, finished goods.
    qolip/            Qolip inventory, locations, checkout, QR.
    werka/            Warehouse legacy flow kept under mobile-compatible name.
    profile/          Profile prefs and avatar flow.
    push/             Push token and dispatch service.
    session/          Persistent bearer session manager.
  db/                 PostgreSQL stores and migrations.
  http/               Axum router, handlers, route tests, PDF helpers.
  store/              LMDB/JSON local state stores.
  fcm.rs              Firebase Cloud Messaging sender.
  main.rs             Service entrypoint.
```

## Operational Notes

- Configure PostgreSQL before testing real ERP workflows.
- Keep LMDB directories on persistent storage.
- Keep Firebase credentials out of the repository.
- Use Cloudflare Tunnel/domain bootstrap for plug-and-play test instances.
- Avoid broad renames of legacy contract names like `werka` unless mobile and
  persisted data migration are planned together.
- Do not make push delivery a hard dependency for successful business writes.
- Preserve mobile API compatibility while refactoring internals.

## Engineering Standard

Before changing behavior:

1. Identify the workflow and its persisted state.
2. Preserve existing route/JSON contract unless explicitly changing mobile too.
3. Add or keep tests around the behavior.
4. Keep production modules focused and below the line-size policy.
5. Run `cargo test --locked` and clippy before push.
