# Mini RS ERP

## What this is

`mini_rs_erp` is the new Accord mini ERP core service copied from the current
Rust server state. This repo is intentionally separated from
`accord_mobile_server_rs` so new mini ERP work does not go back to the old RS
server repository.

The old ERPNext-facing adapters have been removed from the runtime and source
tree. New work should add mini ERP domain storage and service ports directly in
this repository.

## Why it matters

The goal is a clean RS-owned ERP core built around Accord production logic, not
ERPNext doctypes. New work should target the mini ERP domain and its own
production database.

## Quick Start

```bash
cp .env.example .env
cargo fmt --check
cargo test --locked
cargo run --release
```

Configure real `.env` values before production use. At minimum set
`MINI_ERP_DATABASE_URL` and the persistent store paths.

## Current Snapshot Notes

The current snapshot is the mini ERP fork of the stable Rust mobile backend.
Legacy ERP adapters have been removed. Mini ERP production state is being moved
into PostgreSQL behind domain ports.

### PostgreSQL foundation

`MINI_ERP_DATABASE_URL` enables the mini ERP PostgreSQL stores. The foundation
migration currently covers engine event/idempotency tables plus migrated
production map, apparatus group, calculate-order, and mini order persistence.

### Role capability packages

Admin-controlled role packages are now separated from the built-in mobile roles.
The base roles still exist and remain the compatibility boundary:

- `admin`
- `werka`
- `supplier`
- `customer`

Custom role packages are stored as named capability sets and can be assigned to
specific principals by base role and reference. This lets the server restrict or
combine existing features without adding a new hard-coded Rust role for every
operator variant.

Runtime endpoints:

- `GET /v1/mobile/admin/capabilities`
- `GET /v1/mobile/admin/roles`
- `PUT /v1/mobile/admin/roles`
- `GET /v1/mobile/admin/role-assignments`
- `PUT /v1/mobile/admin/role-assignments`

Role assignments override the default capability set for the assigned
principal. If an assignment references a missing role package, access fails
closed instead of falling back to the broader default role. The role store also
keeps backward-compatible reading for the first JSON role-map format.

Configure the persistent role store path with:

```env
MOBILE_API_ROLE_STORE_PATH=data/mobile_roles.json
```

## Performance Validation

The current production candidate was validated on 2026-05-15 after the LMDB
local-state migration, SQL pushdown work, HTTP/healthz tuning, Hyper HTTP/1
accept-loop tuning, and FD-pressure fix.

Key results from the final Go vs Rust production benchmark:

| Case | Rust | Go | Result |
| --- | ---: | ---: | --- |
| `admin_login_5k_100` | `5313.61 RPS`, p95 `28ms` | `36.52 RPS`, p95 `5106ms` | Rust much faster |
| `push_fixed_5k_100` | `5847.71 RPS`, p95 `21ms` | `500.24 RPS`, p95 `44ms` | Rust much faster |
| `read_summary_3k_100` | `2488.98 RPS`, p95 `55ms` | `2712.03 RPS`, p95 `73ms` | Go slightly higher RPS, Rust lower p95 |
| `read_home_3k_100` | `735.85 RPS`, p95 `194ms` | `662.19 RPS`, p95 `340ms` | Rust faster |
| `crash_health_60k_1000` | `5819.38 RPS`, `0` failed | `5515.64 RPS`, `0` failed | Rust stable |
| `crash_push_20k_500` | `5152.66 RPS`, `0` failed | `3726.97 RPS`, `0` failed | Rust stable |

The production service stayed healthy after the run with `{"ok":true}`,
`NRestarts=0`, and `LimitNOFILE=65535`. Rust is ready as the primary Accord
mobile backend for this workload.

Important benchmark notes:

- LMDB is now the production default for session, profile, push token, and
  admin local state.
- Session tokens are stored as `SHA-256(token)` keys, with versioned binary
  values and an LMDB expiry index.
- JSON local stores are legacy migration/rollback inputs, not the default
  production path.
- Rust session-heavy workloads are about `88x-90x` faster than the legacy Go
  JSON session store in the measured login benchmarks.
- SQL pushdown is used where it preserves the exact mobile result shape and
  reduces Rust-side row aggregation.
- Mini ERP production state is moving behind PostgreSQL-backed domain ports.

Full benchmark notes:

- [docs/benchmarks/2026-05-15-final-go-vs-rust-production-battle.md](docs/benchmarks/2026-05-15-final-go-vs-rust-production-battle.md)
- [docs/benchmarks/2026-05-15-go-vs-rust-lmdb-default-battle.md](docs/benchmarks/2026-05-15-go-vs-rust-lmdb-default-battle.md)
- [docs/benchmarks/2026-05-15-go-vs-rust-lmdb-v2-stress.md](docs/benchmarks/2026-05-15-go-vs-rust-lmdb-v2-stress.md)
- [docs/benchmarks/2026-05-15-session-store-lmdb.md](docs/benchmarks/2026-05-15-session-store-lmdb.md)
- [docs/benchmarks/2026-05-15-sql-pushdown.md](docs/benchmarks/2026-05-15-sql-pushdown.md)
- [docs/benchmarks/2026-05-15-healthz-tuning.md](docs/benchmarks/2026-05-15-healthz-tuning.md)
- [docs/benchmarks/2026-05-15-hyper-http1-tuning.md](docs/benchmarks/2026-05-15-hyper-http1-tuning.md)

For historical performance notes and migration context, see
[AI_HANDOFF_PERFORMANCE.md](AI_HANDOFF_PERFORMANCE.md).

`mini_rs_erp` is an independent Rust service for the Accord mobile backend and
mini ERP core. It is a standalone Axum/Tokio application that speaks directly to
mobile clients, PostgreSQL, Firebase Cloud Messaging, Gemini Vision, and
LMDB-backed local state stores.

This repository is not a wrapper around the Go service, does not shell out to
the Go binary, and does not require the Go project at runtime. The compatibility
target is the mobile HTTP contract and the ERPNext side effects expected by the
existing Accord mobile application. In other words, the mobile app should be
able to talk to this Rust service without observing an API or behavior change.

## Purpose

The service exists to provide the mobile API for the operational flow around:

- supplier authentication, item visibility, dispatch creation, and supplier
  response flows;
- Werka dashboard, pending work, archive, PDF export, item/customer/supplier
  search, confirmation, unannounced receipt, customer delivery issue, and AI
  image search flows;
- customer dashboard, delivery note detail, and delivery response flows;
- profile, avatar upload, avatar proxy, session, and mobile identity flows;
- admin settings, supplier/customer/item management, item group tree
  management, code regeneration, and operational activity flows;
- notification comments/details and role-targeted push notifications.

The implementation is organized as a layered service rather than a monolithic
handler file. HTTP handlers are thin adapters; domain services contain business
rules; PostgreSQL stores, local state stores, push, and AI are plugged in through
explicit ports.

## System Architecture

```mermaid
flowchart TB
    Mobile[Mobile clients<br/>supplier, werka, customer, admin]
    HTTP[Axum HTTP router<br/>/healthz and /v1/mobile/*]
    Handlers[HTTP handlers<br/>auth, profile, push, stock, notifications,<br/>supplier, customer, werka, admin]
    Core[Core domain services<br/>auth, sessions, profile, push,<br/>customer, werka, admin]
    Ports[Domain ports<br/>read, write, lookup, state,<br/>credentials, push sender]

    Postgres[Mini ERP PostgreSQL<br/>production maps, apparatus groups,<br/>calculate orders, mini orders, engine events]
    LocalState[Local state stores<br/>LMDB sessions/profile prefs/push tokens/admin state]
    FCM[Firebase Cloud Messaging<br/>HTTP v1]
    Gemini[Gemini Vision<br/>Werka AI search]
    Env[.env runtime persistence<br/>admin settings updates]

    Mobile --> HTTP
    HTTP --> Handlers
    Handlers --> Core
    Core --> Ports
    Ports --> Postgres
    Ports --> LocalState
    Ports --> FCM
    Ports --> Gemini
    Core --> Env
```

## Request Flow

```mermaid
sequenceDiagram
    participant App as Mobile app
    participant Router as Axum router
    participant Handler as HTTP handler
    participant Domain as Core service
    participant Store as Session/local store
    participant DB as Mini ERP PostgreSQL
    participant Push as FCM sender

    App->>Router: HTTP request /v1/mobile/*
    Router->>Handler: Route dispatch
    Handler->>Store: Bearer token lookup when auth is required
    Handler->>Domain: Validated method/query/body
    alt persisted domain flow
        Domain->>DB: Domain store read/write
        DB-->>Domain: Mini ERP state
    else local runtime flow
        Domain->>Store: Local state read/write
        Store-->>Domain: Local state
    end
    opt push event
        Domain->>Push: Best-effort role/ref push
        Push-->>Domain: Delivery result or logged failure
    end
    Domain-->>Handler: Mobile response model
    Handler-->>App: JSON/PDF response with Go-compatible shape
```

## Design Principles

### Standalone service boundary

The service owns its runtime process, HTTP router, domain state, mini ERP
PostgreSQL stores, local stores, and push sender. The Go implementation is not
loaded, embedded, proxied, or required. During migration, Go-compatible behavior
is used as a contract reference so the mobile client can switch services without
a protocol change.

### Contract compatibility

The API is intentionally conservative. Handler method checks, auth order, query
defaults, JSON parse errors, status codes, error bodies, success response shapes,
push payloads, and omit/default serialization behavior are tested against the
same mobile contract.

Important examples:

- unauthorized requests return `401 {"error":"unauthorized"}`;
- role mismatch returns `403 {"error":"forbidden"}`;
- unsupported methods return `405 {"error":"method not allowed"}`;
- invalid JSON returns `400 {"error":"invalid json"}`;
- query parameters such as `ref`, `limit`, `offset`, `receipt_id`, `kind`,
  `item_code`, and `delivery_note_id` are trimmed and defaulted per endpoint;
- push sends are best effort for business flows and must not fail the primary
  HTTP response;
- push token register/delete performs read-before and read-after store access so
  store read failures map to `push token read failed`.

### Separation of concerns

The implementation avoids concentrating the system in a single large file:

- `src/http` contains routing, handlers, PDF generation, and route tests.
- `src/core` contains domain models, service logic, ports, and focused tests.
- `src/db` contains mini ERP PostgreSQL and local SQLite-backed persistence.
- `src/store` contains local JSON and LMDB-backed state stores.
- `src/fcm.rs` contains Firebase Cloud Messaging HTTP v1 delivery.
- `src/ai` contains Gemini Vision integration for Werka image search.

Production Rust files are kept small and focused. Large behavioral coverage
lives in tests, where size is allowed to grow with contract coverage.

## Runtime Components

### HTTP layer

The HTTP service is built with:

- `axum` for routing and request extraction;
- `tower-http` tracing middleware;
- `tokio` for the async runtime;
- `serde` and `serde_json` for JSON request/response models.

The entrypoint is `src/main.rs`:

1. load `.env` with `dotenvy`;
2. initialize `tracing_subscriber`;
3. build `AppConfig` from environment variables;
4. construct `AppState`;
5. build the Axum router;
6. bind `MOBILE_API_ADDR`;
7. serve the mobile API.

### Application state

`src/app.rs` wires runtime dependencies:

- `AuthService` for login, role inference, admin/Werka identity, and session
  principal construction;
- `AdminService` for settings, supplier/customer/item management, code
  regeneration, and activity;
- `CustomerService` for customer delivery summaries, details, and responses;
- `ProfileService` for profile refresh, nickname prefs, avatar upload, and
  avatar proxy;
- `PushService` for push token registration and role/ref push delivery;
- `WerkaService` for dashboard, lookup, archive, confirmations, unannounced
  receipts, notification details/comments, supplier reads, and issue creation;
- `SessionManager` for persistent bearer sessions.

### Mini ERP persistence

`src/db` owns mini ERP persistence. PostgreSQL is used for production mini ERP
state when `MINI_ERP_DATABASE_URL` is configured. Local SQLite/LMDB stores remain
as bounded local stores for migrated mobile state and development fallback.

### Admin item group tree management

Admin item group workflows are moving to mini ERP owned data stores.
The mobile API supports:

- item group search for parent pickers;
- item group creation with parent and `is_group`;
- moving an existing group under a new parent;
- bulk moving items into an item group.

The service keeps the `is_group` invariant valid for mobile-created trees. When
a child group is created under a parent, the parent is promoted to a group if
needed. When a legacy or manually-created
node already has children but is still marked as a leaf, the move flow promotes
it before asking ERPNext to save the new parent.

### Local state

The service keeps small local operational state on disk:

- session store: bearer tokens and principals, with `json` and `lmdb` backends;
- profile prefs: nickname and user-specific profile preferences, with `json` and `lmdb` backends;
- push token store: role/ref keys mapped to FCM device tokens, with `json` and `lmdb` backends;
- admin supplier/customer state: generated codes, blocked/removed flags,
  assignment cache, and cooldown metadata, with `json` and `lmdb` backends.

LMDB is the production default for local state. JSON files are kept as explicit
legacy stores and migration inputs, so existing data can move gradually without
changing the mobile API contract.

The production path is fail-fast by default: if an LMDB backend is selected and
cannot open, the service does not silently split state into JSON unless
`MOBILE_API_LOCAL_STORE_ALLOW_JSON_FALLBACK=1` is explicitly set for emergency
rollback.

### Push notifications

Push token registration stores device tokens under role/ref keys such as:

- `supplier:<supplier_ref>`;
- `customer:<customer_ref>`;
- `werka:werka`;
- `admin:admin`.

FCM delivery uses Firebase Cloud Messaging HTTP v1. The sender discovers a
service account JSON in this order:

1. `FCM_SERVICE_ACCOUNT_PATH` if it points to an existing file;
2. the first `*firebase-adminsdk*.json` file in the current directory;
3. `service-account.json`.

If no valid service account is found, push is disabled with a no-op sender.
Business operations still succeed because push delivery is best effort.

### AI search

Werka image search can use Gemini Vision when `GEMINI_API_KEY` is configured.
If the key is absent, the AI search service is not wired and the relevant route
returns the configured error behavior.

## API Surface

The service exposes the following mobile routes. The router registers all of
them under the same paths expected by the mobile app.

### Health and auth

| Route | Purpose |
| --- | --- |
| `/healthz` | Liveness response. |
| `/v1/mobile/auth/login` | Login by phone/code. |
| `/v1/mobile/auth/logout` | Logout current bearer session. |
| `/v1/mobile/me` | Return current principal. |

### Profile

| Route | Purpose |
| --- | --- |
| `/v1/mobile/profile` | Get/update profile and nickname preferences. |
| `/v1/mobile/profile/avatar` | Supplier avatar upload. |
| `/v1/mobile/profile/avatar/view` | Supplier avatar proxy by bearer token or token query. |

### Push and stock lookup

| Route | Purpose |
| --- | --- |
| `/v1/mobile/push/token` | Register/delete push token for supplier or Werka. |
| `/v1/mobile/stock-entry/lookup` | Stock entry lookup by barcode. |

### Customer

| Route | Purpose |
| --- | --- |
| `/v1/mobile/customer/summary` | Customer delivery summary. |
| `/v1/mobile/customer/history` | Customer delivery history. |
| `/v1/mobile/customer/status-details` | Customer status-detail list by kind. |
| `/v1/mobile/customer/detail` | Delivery note detail. |
| `/v1/mobile/customer/respond` | Customer accept/reject/partial response. |

### Notifications

| Route | Purpose |
| --- | --- |
| `/v1/mobile/notifications/detail` | Supplier/customer/Werka notification detail. |
| `/v1/mobile/notifications/comments` | Add notification comment or supplier acknowledgment. |

### Supplier

| Route | Purpose |
| --- | --- |
| `/v1/mobile/supplier/unannounced/respond` | Supplier approve/reject Werka-created unannounced draft. |
| `/v1/mobile/supplier/summary` | Supplier receipt summary. |
| `/v1/mobile/supplier/status-breakdown` | Supplier status aggregate by item. |
| `/v1/mobile/supplier/status-details` | Supplier receipt details by kind/item. |
| `/v1/mobile/supplier/history` | Supplier receipt history. |
| `/v1/mobile/supplier/items` | Supplier item list with fallback behavior. |
| `/v1/mobile/supplier/dispatch` | Supplier dispatch creation. |

### Werka

| Route | Purpose |
| --- | --- |
| `/v1/mobile/werka/summary` | Werka dashboard summary. |
| `/v1/mobile/werka/home` | Werka home with summary and pending items. |
| `/v1/mobile/werka/customers` | Customer directory. |
| `/v1/mobile/werka/suppliers` | Supplier directory. |
| `/v1/mobile/werka/ai-search-suggestion` | AI item/customer/supplier suggestion from image. |
| `/v1/mobile/werka/supplier-items` | Supplier item search. |
| `/v1/mobile/werka/customer-items` | Customer item search. |
| `/v1/mobile/werka/customer-item-options` | Customer item option search. |
| `/v1/mobile/werka/customer-issue/create` | Single customer delivery issue. |
| `/v1/mobile/werka/customer-issue/batch-create` | Batch customer delivery issue. |
| `/v1/mobile/werka/unannounced/create` | Create supplier unannounced draft. |
| `/v1/mobile/werka/status-breakdown` | Werka status aggregate. |
| `/v1/mobile/werka/status-details` | Werka status details. |
| `/v1/mobile/werka/pending` | Werka pending work list. |
| `/v1/mobile/werka/history` | Werka recent activity. |
| `/v1/mobile/werka/notifications` | Alias to Werka history behavior. |
| `/v1/mobile/werka/archive` | Archive query. |
| `/v1/mobile/werka/archive/pdf` | Archive PDF export. |
| `/v1/mobile/werka/confirm` | Confirm receipt accepted/returned quantities. |

### Admin

| Route | Purpose |
| --- | --- |
| `/v1/mobile/admin/settings` | Read/update runtime settings. |
| `/v1/mobile/admin/suppliers` | Supplier management page and supplier create. |
| `/v1/mobile/admin/suppliers/list` | Paged supplier list. |
| `/v1/mobile/admin/suppliers/summary` | Supplier summary. |
| `/v1/mobile/admin/suppliers/detail` | Supplier detail and assigned items. |
| `/v1/mobile/admin/suppliers/inactive` | Inactive/removed supplier list. |
| `/v1/mobile/admin/suppliers/status` | Block/unblock supplier. |
| `/v1/mobile/admin/suppliers/phone` | Update supplier phone. |
| `/v1/mobile/admin/suppliers/items` | Replace supplier item assignments. |
| `/v1/mobile/admin/suppliers/items/assigned` | Assigned supplier items. |
| `/v1/mobile/admin/suppliers/items/add` | Assign one supplier item. |
| `/v1/mobile/admin/suppliers/items/remove` | Unassign one supplier item. |
| `/v1/mobile/admin/suppliers/code/regenerate` | Regenerate supplier code. |
| `/v1/mobile/admin/suppliers/remove` | Soft-remove supplier. |
| `/v1/mobile/admin/suppliers/restore` | Restore supplier. |
| `/v1/mobile/admin/customers` | Customer list and customer create. |
| `/v1/mobile/admin/customers/list` | Paged customer list. |
| `/v1/mobile/admin/customers/detail` | Customer detail and assigned items. |
| `/v1/mobile/admin/customers/phone` | Update customer phone. |
| `/v1/mobile/admin/customers/code/regenerate` | Regenerate customer code. |
| `/v1/mobile/admin/customers/items/add` | Assign one customer item. |
| `/v1/mobile/admin/customers/items/remove` | Unassign one customer item. |
| `/v1/mobile/admin/customers/remove` | Soft-remove customer. |
| `/v1/mobile/admin/item-groups` | Item group search, create, and parent move. |
| `/v1/mobile/admin/items` | Item list and item create. |
| `/v1/mobile/admin/items/bulk-move-group` | Move multiple items to an item group. |
| `/v1/mobile/admin/activity` | Admin activity feed. |
| `/v1/mobile/admin/werka/code/regenerate` | Regenerate Werka code. |

## Configuration

Configuration is read from the environment after `.env` is loaded.

### Required for ERPNext-backed runtime

| Variable | Description |
| --- | --- |
| `ERP_URL` | ERPNext base URL. |
| `ERP_API_KEY` | ERPNext API key. |
| `ERP_API_SECRET` | ERPNext API secret. |

When any of these are missing, ERPNext-backed read/write ports are not wired.
The service can still start, but ERP-dependent routes return their configured
failure responses.

### Core service settings

| Variable | Default | Description |
| --- | --- | --- |
| `MOBILE_API_ADDR` | `:8081` | Bind address. Leading `:8081` is normalized to `0.0.0.0:8081`. |
| `MOBILE_API_LOCAL_STORE_ALLOW_JSON_FALLBACK` | `0` | Set to `1` only for emergency rollback. When LMDB is selected and cannot open, the service fails fast by default instead of silently splitting state into JSON. |
| `MOBILE_API_SESSION_STORE_PATH` | `data/mobile_sessions.json` | Persistent session store path. |
| `MOBILE_API_SESSION_STORE` | fallback only | Legacy session store variable used when `MOBILE_API_SESSION_STORE_PATH` is absent. |
| `MOBILE_API_SESSION_STORE_BACKEND` | `lmdb` | Session backend: `lmdb` or `json`. LMDB is the production default. |
| `MOBILE_API_SESSION_LMDB_PATH` | `data/mobile_sessions.lmdb` | LMDB environment directory when the LMDB session backend is enabled. |
| `MOBILE_API_SESSION_LMDB_MAP_SIZE_MB` | `64` | LMDB map size for session storage. |
| `MOBILE_API_PROFILE_STORE_PATH` | `data/mobile_profile_prefs.json` | Profile preferences store path. |
| `MOBILE_API_PROFILE_STORE_BACKEND` | `lmdb` | Profile preferences backend: `lmdb` or `json`. LMDB is the production default. |
| `MOBILE_API_PROFILE_LMDB_PATH` | `data/mobile_profile_prefs.lmdb` | LMDB environment directory when the LMDB profile backend is enabled. |
| `MOBILE_API_PROFILE_LMDB_MAP_SIZE_MB` | `64` | LMDB map size for profile preference storage. |
| `MOBILE_API_PUSH_TOKEN_STORE_PATH` | `data/mobile_push_tokens.json` | Push token store path. |
| `MOBILE_API_PUSH_TOKEN_STORE_BACKEND` | `lmdb` | Push token backend: `lmdb` or `json`. LMDB is the production default. |
| `MOBILE_API_PUSH_TOKEN_LMDB_PATH` | `data/mobile_push_tokens.lmdb` | LMDB environment directory when the LMDB push token backend is enabled. |
| `MOBILE_API_PUSH_TOKEN_LMDB_MAP_SIZE_MB` | `64` | LMDB map size for push token storage. |
| `MOBILE_API_ADMIN_SUPPLIER_STORE_PATH` | `data/mobile_admin_suppliers.json` | Admin supplier/customer state store path. |
| `MOBILE_API_ADMIN_SUPPLIER_STORE_BACKEND` | `lmdb` | Admin supplier/customer state backend: `lmdb` or `json`. LMDB is the production default. |
| `MOBILE_API_ADMIN_SUPPLIER_LMDB_PATH` | `data/mobile_admin_suppliers.lmdb` | LMDB environment directory when the LMDB admin state backend is enabled. |
| `MOBILE_API_ADMIN_SUPPLIER_LMDB_MAP_SIZE_MB` | `64` | LMDB map size for admin supplier/customer state storage. |
| `MOBILE_API_SESSION_TTL_HOURS` | `720` | Bearer session TTL in hours. |
| `ERP_TIMEOUT_SECONDS` | `15` | AI and HTTP client timeout baseline. |
| `ERP_DEFAULT_TARGET_WAREHOUSE` | empty | Legacy admin setting name for default warehouse during migration. |
| `ERP_DEFAULT_UOM` | `Kg` | Admin default unit of measure. |
| `MOBILE_DEV_SUPPLIER_PREFIX` | `10` | Supplier code prefix. |
| `MOBILE_DEV_WERKA_PREFIX` | `20` | Werka code prefix. |
| `MOBILE_DEV_WERKA_CODE` | empty | Werka login code. |
| `MOBILE_DEV_WERKA_NAME` | `Werka` | Werka display name. |

Admin identity defaults are initialized in configuration and can be updated at
runtime through admin settings. Admin settings persist selected values back to
`.env` through `DotEnvPersister`.

### Push settings

| Variable | Description |
| --- | --- |
| `FCM_SERVICE_ACCOUNT_PATH` | Preferred Firebase service account JSON path. |

If the variable is absent, the service searches the current directory for
`*firebase-adminsdk*.json`, then `service-account.json`.

### AI settings

| Variable | Description |
| --- | --- |
| `GEMINI_API_KEY` | Enables Werka AI search. |
| `GEMINI_VISION_MODEL` | Optional model name override. |

### Logging

Use `RUST_LOG` with `tracing_subscriber`, for example:

```bash
RUST_LOG=info,accord_mobile_server_rs=debug cargo run
```

## Example Runtime Environment

```bash
MOBILE_API_ADDR=:8081
MOBILE_API_LOCAL_STORE_ALLOW_JSON_FALLBACK=0

MINI_ERP_DATABASE_URL=postgres://mini_rs_erp:secret@127.0.0.1:5432/mini_rs_erp
ERP_DEFAULT_TARGET_WAREHOUSE=Stores - CH
ERP_DEFAULT_UOM=Kg
ERP_TIMEOUT_SECONDS=15

MOBILE_API_SESSION_STORE_PATH=data/mobile_sessions.json
MOBILE_API_SESSION_STORE_BACKEND=lmdb
MOBILE_API_SESSION_LMDB_PATH=data/mobile_sessions.lmdb
MOBILE_API_SESSION_LMDB_MAP_SIZE_MB=64
MOBILE_API_PROFILE_STORE_PATH=data/mobile_profile_prefs.json
MOBILE_API_PROFILE_STORE_BACKEND=lmdb
MOBILE_API_PROFILE_LMDB_PATH=data/mobile_profile_prefs.lmdb
MOBILE_API_PROFILE_LMDB_MAP_SIZE_MB=64
MOBILE_API_PUSH_TOKEN_STORE_PATH=data/mobile_push_tokens.json
MOBILE_API_PUSH_TOKEN_STORE_BACKEND=lmdb
MOBILE_API_PUSH_TOKEN_LMDB_PATH=data/mobile_push_tokens.lmdb
MOBILE_API_PUSH_TOKEN_LMDB_MAP_SIZE_MB=64
MOBILE_API_ADMIN_SUPPLIER_STORE_PATH=data/mobile_admin_suppliers.json
MOBILE_API_ADMIN_SUPPLIER_STORE_BACKEND=lmdb
MOBILE_API_ADMIN_SUPPLIER_LMDB_PATH=data/mobile_admin_suppliers.lmdb
MOBILE_API_ADMIN_SUPPLIER_LMDB_MAP_SIZE_MB=64
MOBILE_API_SESSION_TTL_HOURS=720

MOBILE_DEV_SUPPLIER_PREFIX=10
MOBILE_DEV_WERKA_PREFIX=20
MOBILE_DEV_WERKA_CODE=20ABCDEF1234
MOBILE_DEV_WERKA_NAME=Werka

FCM_SERVICE_ACCOUNT_PATH=/path/to/firebase-adminsdk.json
GEMINI_API_KEY=
GEMINI_VISION_MODEL=
RUST_LOG=info
```

## Running

### Development

```bash
cargo run
```

### Production-style build

```bash
cargo build --release
./target/release/accord_mobile_server_rs
```

### Health check

```bash
curl http://127.0.0.1:8081/healthz
```

Expected response:

```json
{"ok":true}
```

## Testing

Run the complete test suite:

```bash
cargo test
```

Run focused suites:

```bash
cargo test admin
cargo test push
cargo test fcm
cargo test werka
cargo test supplier
cargo test customer
```

Check compilation without running tests:

```bash
cargo check
```

Check formatting:

```bash
cargo fmt --check
```

The test suite covers:

- route inventory registration for every mobile path;
- method/auth order parity for high-risk routes;
- exact status/error bodies for common failure modes;
- response shape and serialization behavior;
- supplier, Werka, customer, profile, notifications, stock-entry, admin, push,
  and FCM flows;
- mini ERP PostgreSQL migration and store behavior;
- local JSON store compatibility;
- archive PDF generation contract.

## Data and Side Effects

### Session lifecycle

Sessions are bearer tokens stored through `SessionManager`. The production
backend is LMDB, with JSON kept for legacy migration and explicit rollback.
Sessions expire according to `MOBILE_API_SESSION_TTL_HOURS`.

### Admin settings persistence

Admin settings update runtime auth/admin configuration and persist selected
settings into `.env`.

### Push behavior

Push is role/ref targeted and best effort in business handlers. Failed push
delivery is logged and does not roll back successful domain actions.

FCM stale-token behavior removes tokens on:

- HTTP `404` with "requested entity was not found";
- HTTP `400`/`404` with "unregistered";
- HTTP `400` with "registration token is not a valid FCM registration token".

## Repository Layout

```text
src/
  ai/                 Gemini Vision integration for Werka search.
  app.rs              Runtime dependency wiring.
  config.rs           Environment configuration and .env persistence.
  core/               Domain models, ports, and services.
    admin/            Admin state, read/write service, mutations.
    auth/             Login, access code, principal, auth ports.
    customer/         Customer delivery note response flow.
    profile/          Profile refresh, preferences, avatar flow.
    push/             Push token store service and sender port.
    session/          Persistent session manager.
    werka/            Werka dashboard, archive, confirm, issue, notification flows.
  db/                 Mini ERP PostgreSQL and local SQLite persistence.
  http/               Axum router, handlers, route tests, PDF generator.
  store/              JSON and LMDB-backed local state stores.
  fcm.rs              Firebase Cloud Messaging HTTP v1 sender.
  main.rs             Process entrypoint.
```

## Operational Notes

- Configure `MINI_ERP_DATABASE_URL` before enabling production mini ERP flows.
- Keep LMDB store directories on persistent storage in production.
- Keep JSON store paths only when legacy migration or emergency rollback is
  required.
- Keep Firebase service account JSON outside the repository and pass its path
  through `FCM_SERVICE_ACCOUNT_PATH`.
- Use `RUST_LOG=info` or more specific module filters during smoke tests.
- Admin mutations are powerful and should be exposed only behind the same
  network/auth boundary as the mobile app expects.

## Compatibility Status

The current implementation registers the full mobile route surface and has
focused route/domain tests for the mobile API contract. Production-like ERPNext
benchmarking and smoke testing have been performed for the main read/write hot
paths. Before switching a new live ERPNext company to this service, repeat the
smoke test against that company's data and verify both HTTP responses and
ERPNext document side effects.

Recommended smoke-test order:

1. `/healthz`;
2. auth login for admin, Werka, supplier, and customer;
3. read-only dashboard/list/detail endpoints;
4. supplier dispatch and unannounced response;
5. Werka confirm, unannounced create, and customer issue create;
6. customer respond;
7. profile and avatar flows;
8. admin settings and small reversible admin mutations;
9. push token register/delete and one controlled FCM send path.

## Engineering Rules

- Keep production modules focused and below the repository line-size policy.
- Keep large behavioral coverage in test files.
- Preserve the mobile contract before refactoring internals.
- Prefer explicit ports over hidden global dependencies.
- Do not make push delivery a hard dependency for successful business writes.
- Keep domain mutations behind explicit mini ERP service/store ports.
- Keep local state stores inspectable and compatible with the mobile runtime.
