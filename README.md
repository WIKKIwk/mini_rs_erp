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

### Working Principles

Mini ERP is built around a few strict principles:

- The map defines the planned production route.
- The queue controls what an operator is allowed to do next.
- Progress actions create immutable operational evidence: sessions, events, WIP
  batches, QR payloads, and audit logs.
- Physical stock movement must be backed by identity checks, QR scans, or
  explicit operator action.
- External integrations such as printing and push notifications support the
  workflow, but they should not hide the ERP state transition.

At runtime, most write flows follow this shape:

```mermaid
sequenceDiagram
    participant Operator as Mobile Operator
    participant Handler as HTTP Handler
    participant Service as Domain Service
    participant Store as PostgreSQL Store
    participant Print as Print/Push Side Effects

    Operator->>Handler: Action request
    Handler->>Service: Authenticated input
    Service->>Store: Load current state
    Service->>Service: Validate role, queue state, QR, material, and quantity
    Service->>Store: Persist state transition
    Service->>Print: Optional label, QR, push, or live notification
    Service-->>Handler: Updated ERP state
    Handler-->>Operator: JSON response
```

The important rule is that PostgreSQL is the source of truth for ERP state.
Handlers should not calculate business outcomes by themselves; they should call
domain services, and services should persist through explicit store ports.

### Calculation System

Production calculation starts from a `ProductionMapDefinition`. A map is a graph
of nodes and edges. Nodes may represent start/end markers, tasks, formula
calculations, condition branches, or location markers.

The calculation flow is:

1. `compile_map` validates the graph, rejects invalid formulas, rejects cycles,
   and produces an ordered `ProductionMapProgram`.
2. `run_map_with_variables` starts with `order_qty` and optional runtime
   variables.
3. Formula nodes write calculated variables, for example `cpp_kg = order_qty *
   1.08`.
4. Condition nodes decide which branch can continue. If a required variable is
   missing, the run can stop at that condition and wait for runtime input.
5. Task nodes calculate their own quantity through `qty_formula`; if it is
   empty, the task inherits `order_qty`.
6. Non-positive or non-finite quantities are rejected.

```mermaid
flowchart LR
    Map["ProductionMapDefinition"]
    Validate["Validate graph<br/>nodes, edges, formulas"]
    Program["ProductionMapProgram"]
    Variables["Variables<br/>order_qty + calculated values"]
    Tasks["Task drafts<br/>apparatus + qty"]

    Map --> Validate
    Validate --> Program
    Program --> Variables
    Variables --> Tasks
```

The formula system intentionally supports a narrow, auditable expression model:

- arithmetic formulas are evaluated against named numeric variables;
- condition formulas can compare values with operators such as `>`, `>=`, `<`,
  `<=`, `==`, and `!=`;
- formula targets must be valid identifiers;
- location references are validated separately;
- invalid expressions fail before the map is accepted.

#### Quantity and Waste Metrics

Runtime progress does not blindly trust client input. Progress quantities and
completion metrics are validated by production stage:

- progress quantity must be finite and positive;
- Bosma completion requires return ink, total waste, finished goods kg, and
  finished goods meter;
- Laminatsiya completion requires leftover roll information, total waste,
  finished goods kg, and finished goods meter;
- Rezka progress requires bosma waste, lamination waste, and edge waste;
- final finished-goods receiving uses the completed progress batch quantity and
  stores a stock entry for the receiving warehouse.

This keeps operational metrics tied to the actual queue action that produced
them.

#### Raw Material Validation

Some apparatus stages require material scans before start. The rule system is:

- admin defines apparatus material requirements;
- raw materials are assigned to an order by barcode;
- start actions can require an exact scan set;
- alternative material groups can satisfy a requirement when configured;
- duplicate or mismatched scans are rejected.

The purpose is to prevent an operator from starting a production stage with the
wrong input material.

#### Queue and WIP Flow

Production queue flow is stateful:

```mermaid
stateDiagram-v2
    [*] --> Pending
    Pending --> InProgress: start
    InProgress --> Paused: pause + output QR
    Paused --> InProgress: resume
    InProgress --> Completed: complete + metrics
    Completed --> FinishedGoods: final stage receive
```

For multi-stage production, WIP QR links output from one stage to input for the
next stage:

- pause/complete can create a progress batch with QR payload;
- downstream start must scan the previous stage QR when required;
- scanned WIP can move from `waiting` to `in_use` to `processed`;
- an order is not fully closed while required previous WIPs are still waiting
  for downstream processing;
- QR reports can identify stale or superseded batches.

This lets the system track not only that an order moved forward, but which
physical intermediate output moved to the next apparatus.

#### Production Action Contract

| Action | Preconditions | Persisted effects |
| --- | --- | --- |
| `start` | order exists in apparatus queue, queue policy allows action, required previous-stage QR/material scan is valid | active `OrderRunSession`, queue state moves to in-progress, optional WIP input marked `in_use` |
| `pause` | active session exists for order/apparatus | session paused, progress event saved, output progress batch saved with QR payload |
| `resume` | paused session exists or accepted WIP batch exists | session returns to active, resumed batch state is refreshed |
| `complete` | active/paused work can be completed, required metrics are present for the apparatus type | session completed, progress event saved, progress batch saved or updated, queue state moves to completed or pending when previous WIP is still unprocessed |
| `finished-goods receive` | completed final-stage batch exists, batch is still waiting, no next apparatus exists | finished-goods stock entry saved, batch marked received/processed for warehouse |

Queue actions are serialized by a queue-action guard in service code. This
prevents two operators from racing the same order/apparatus state transition in
one process. PostgreSQL constraints and transactional writes protect persisted
state.

#### Qolip Operation Contract

| Operation | Preconditions | Persisted effects |
| --- | --- | --- |
| create block | admin or assigned warehouse access | child warehouse/block saved |
| upsert product spec | Qolip capability | product Qolip code and size saved |
| upsert location | authorized block, valid item and cell identity | creates location or increments existing compatible location quantity |
| move location | authorized source block, positive quantity, target cell compatible | source decremented/deleted, target created/incremented |
| create cell QR | authorized block/cell | deterministic cell QR row created or reused |
| checkout | authorized block, location exists, worker exists, enough quantity | location decremented/deleted, checkout row created |
| return checkout | checkout exists and is open, target cell compatible | checkout marked returned, location restored/incremented |

Qolip identity is strict. Two rows can merge only when item code, Qolip code,
size, block, warehouse, and cell identity match. Otherwise the operation returns
a conflict-style domain error instead of corrupting stock.

#### Warehouse Quantity Contract

Warehouse/werka flows track sent, accepted, and returned quantities:

| Flow | Quantity rule |
| --- | --- |
| supplier dispatch | supplier sends a declared quantity for an item/customer context |
| warehouse confirm | accepted quantity determines `accepted`, `partial`, or `returned/rejected` status |
| unannounced receipt | warehouse can create a pending supplier draft when supplier did not pre-register it |
| customer issue | warehouse records delivery issue lines and can notify customer/admin |
| notification comment | comments and acknowledgments are attached to the source document/event |

Status derivation follows quantity semantics: zero accepted means rejected or
returned, accepted less than sent means partial, and accepted equal to sent means
accepted. The exact response shape remains mobile-compatible.

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

### Endpoint Catalog

This is the current route inventory at the group level. It is intentionally kept
in README so a new engineer can understand the public surface before reading
handlers.

#### Core Mobile Routes

| Route | Purpose |
| --- | --- |
| `/v1/mobile/auth/login` | Login by role-specific phone/code. |
| `/v1/mobile/auth/logout` | Revoke current bearer session. |
| `/v1/mobile/me` | Return current principal/session profile. |
| `/v1/mobile/calculate` | Quick production calculation. |
| `/v1/mobile/calculate/orders` | Save/list quick calculate orders. |
| `/v1/mobile/calculate/orders/delete` | Delete quick calculate order. |
| `/v1/mobile/calculate/orders/image` | Upload quick order image. |
| `/v1/mobile/calculate/orders/image/view` | View quick order image. |
| `/v1/mobile/profile` | Read/update profile and nickname preferences. |
| `/v1/mobile/profile/avatar` | Upload profile avatar. |
| `/v1/mobile/profile/avatar/view` | View avatar by token or bearer auth. |
| `/v1/mobile/push/token` | Register/delete FCM token. |
| `/v1/mobile/stock-entry/lookup` | Lookup stock entry by barcode. |

#### Production, Qolip, Print, and Warehouse Routes

| Route | Purpose |
| --- | --- |
| `/v1/mobile/gscale/items` | GScale item catalog lookup. |
| `/v1/mobile/gscale/material-receipt/print` | Print material receipt and create stock draft. |
| `/v1/mobile/rps/batch/start` | Start RPS batch session. |
| `/v1/mobile/rps/batch/state` | Read RPS batch state. |
| `/v1/mobile/rps/batch/stop` | Stop active RPS batch. |
| `/v1/mobile/rps/batch/print` | Print RPS batch label. |
| `/v1/mobile/rezka/source` | Read source stock for rezka split. |
| `/v1/mobile/rezka/split` | Split rezka source stock into outputs. |
| `/v1/mobile/qolip/blocks` | List/create Qolip blocks. |
| `/v1/mobile/qolip/products` | Search products with Qolip specs. |
| `/v1/mobile/qolip/product-specs` | Upsert Qolip product spec. |
| `/v1/mobile/qolip/locations` | List/create Qolip locations. |
| `/v1/mobile/qolip/locations/move` | Move quantity between Qolip cells. |
| `/v1/mobile/qolip/cell-qr` | Resolve Qolip cell QR. |
| `/v1/mobile/qolip/cell-qr/print` | Create/print Qolip cell QR label. |
| `/v1/mobile/qolip/workers` | Search workers for Qolip checkout. |
| `/v1/mobile/qolip/checkouts` | List/create Qolip checkouts. |
| `/v1/mobile/qolip/checkouts/return` | Return checkout to a Qolip cell. |

#### Customer, Supplier, and Warehouse Legacy Routes

| Route | Purpose |
| --- | --- |
| `/v1/mobile/customer/summary` | Customer delivery summary. |
| `/v1/mobile/customer/history` | Customer delivery history. |
| `/v1/mobile/customer/status-details` | Customer status details by kind. |
| `/v1/mobile/customer/detail` | Delivery note detail. |
| `/v1/mobile/customer/respond` | Accept/reject/partial delivery response. |
| `/v1/mobile/supplier/dispatch` | Supplier dispatch creation. |
| `/v1/mobile/supplier/history` | Supplier receipt history. |
| `/v1/mobile/supplier/items` | Supplier item list. |
| `/v1/mobile/supplier/status-breakdown` | Supplier status aggregate. |
| `/v1/mobile/supplier/status-details` | Supplier status details. |
| `/v1/mobile/supplier/summary` | Supplier summary. |
| `/v1/mobile/supplier/unannounced/respond` | Supplier approve/reject unannounced warehouse draft. |
| `/v1/mobile/werka/summary` | Warehouse dashboard summary. |
| `/v1/mobile/werka/home` | Warehouse dashboard home. |
| `/v1/mobile/werka/pending` | Warehouse pending work. |
| `/v1/mobile/werka/history` | Warehouse recent activity. |
| `/v1/mobile/werka/notifications` | Alias to warehouse history behavior. |
| `/v1/mobile/werka/archive` | Warehouse archive query. |
| `/v1/mobile/werka/archive/pdf` | Warehouse archive PDF export. |
| `/v1/mobile/werka/confirm` | Confirm accepted/returned receipt quantities. |
| `/v1/mobile/werka/unannounced/create` | Create unannounced supplier draft. |
| `/v1/mobile/werka/customer-issue/create` | Create one customer issue. |
| `/v1/mobile/werka/customer-issue/batch-create` | Create customer issues in batch. |
| `/v1/mobile/werka/status-breakdown` | Warehouse status aggregate. |
| `/v1/mobile/werka/status-details` | Warehouse status details. |
| `/v1/mobile/werka/customers` | Customer directory. |
| `/v1/mobile/werka/suppliers` | Supplier directory. |
| `/v1/mobile/werka/supplier-items` | Supplier item lookup. |
| `/v1/mobile/werka/customer-items` | Customer item lookup. |
| `/v1/mobile/werka/customer-item-options` | Customer item option lookup. |
| `/v1/mobile/werka/ai-search-suggestion` | AI suggestion from uploaded image. |

#### Admin Routes

| Route | Purpose |
| --- | --- |
| `/v1/mobile/admin/settings` | Read/update runtime settings. |
| `/v1/mobile/admin/capabilities` | Read capability catalog. |
| `/v1/mobile/admin/roles` | Read/upsert custom role packages. |
| `/v1/mobile/admin/role-assignments` | Read/upsert role assignments. |
| `/v1/mobile/admin/users/list` | Unified user list. |
| `/v1/mobile/admin/workers` | Worker list/create. |
| `/v1/mobile/admin/workers/detail` | Worker detail. |
| `/v1/mobile/admin/workers/profile-detail` | Worker profile detail. |
| `/v1/mobile/admin/workers/code/regenerate` | Regenerate worker code. |
| `/v1/mobile/admin/worker-groups` | Manage worker groups by apparatus. |
| `/v1/mobile/admin/apparatus` | Create/list apparatus metadata. |
| `/v1/mobile/admin/apparatus-groups` | Manage apparatus groups. |
| `/v1/mobile/admin/warehouses` | Create/list warehouses. |
| `/v1/mobile/admin/warehouses/live` | Warehouse live event stream. |
| `/v1/mobile/admin/warehouses/summary` | Warehouse summaries. |
| `/v1/mobile/admin/warehouses/assignments` | Principal to warehouse/block assignment. |
| `/v1/mobile/admin/items` | Item list/create. |
| `/v1/mobile/admin/items/bulk-move-group` | Bulk move items to item group. |
| `/v1/mobile/admin/item-groups` | Item group search/create/move. |
| `/v1/mobile/admin/item-groups/tree` | Item group tree. |
| `/v1/mobile/admin/suppliers/*` | Supplier directory, details, codes, status, item assignments. |
| `/v1/mobile/admin/customers/*` | Customer directory, details, codes, item assignments. |
| `/v1/mobile/admin/activity` | Admin activity feed. |
| `/v1/mobile/admin/system/monitor` | Runtime/database/backup monitor snapshot. |
| `/v1/mobile/admin/system/monitor/live` | Monitor WebSocket stream. |
| `/v1/mobile/admin/werka/code/regenerate` | Regenerate warehouse/werka code. |

#### Admin Production Routes

| Route | Purpose |
| --- | --- |
| `/v1/mobile/admin/production-maps` | List/create/update production maps. |
| `/v1/mobile/admin/production-maps/run` | Run map calculation. |
| `/v1/mobile/admin/production-maps/audit` | Audit production-map consistency. |
| `/v1/mobile/admin/production-maps/with-order` | Save map with order metadata. |
| `/v1/mobile/admin/production-maps/move` | Move order between apparatus chains. |
| `/v1/mobile/admin/production-maps/move-batch` | Batch move production maps. |
| `/v1/mobile/admin/production-maps/sequence` | Read/write apparatus queue sequence. |
| `/v1/mobile/admin/production-maps/queue-policies` | Read/write queue policy. |
| `/v1/mobile/admin/production-maps/live` | Queue live WebSocket stream. |
| `/v1/mobile/admin/production-maps/queue-action` | Apply start/pause/resume/complete. |
| `/v1/mobile/admin/production-maps/completed-orders` | Read completed queue orders. |
| `/v1/mobile/admin/production-maps/closed-orders` | Read closed orders. |
| `/v1/mobile/admin/production-maps/completion-requests` | Read completion requests. |
| `/v1/mobile/admin/production-maps/completion-requests/decision` | Approve/reject completion request. |
| `/v1/mobile/admin/production-maps/completion-request-decisions` | Read completion request decisions. |
| `/v1/mobile/admin/production-maps/progress-qr/lookup` | Lookup progress QR. |
| `/v1/mobile/admin/production-maps/progress-qr/history` | Progress QR history. |
| `/v1/mobile/admin/production-maps/progress-qr/report` | Progress QR report. |
| `/v1/mobile/admin/production-maps/progress-qr/reprint` | Reprint progress QR label. |
| `/v1/mobile/admin/production-maps/wip-batches` | List WIP progress batches. |
| `/v1/mobile/admin/production-maps/finished-goods/receive` | Receive final goods into warehouse. |
| `/v1/mobile/admin/raw-material-rules` | Manage apparatus material rules. |
| `/v1/mobile/admin/raw-material-assignments/lookup` | Lookup raw material assignment. |
| `/v1/mobile/admin/raw-material-assignments` | Assign/unassign raw material to order. |
| `/v1/mobile/admin/raw-material-stock` | Search raw material stock. |

## Authorization Model

The system has built-in base roles for compatibility and capability packages
for operational precision. Base roles are still used by login, route contracts,
push keys, and legacy mobile behavior. Capability checks decide whether a
principal can execute a specific action.

| Role | Default meaning |
| --- | --- |
| `admin` | Full administrative operator. |
| `werka` | Warehouse/operator flow. Legacy name retained for compatibility. |
| `supplier` | Supplier portal and dispatch flow. |
| `customer` | Customer delivery review/response flow. |
| `aparatchi` or worker roles | Production apparatus operator. |
| `qolipchi` | Qolip inventory operator. |

Capability groups:

| Capability area | Examples |
| --- | --- |
| Admin and role control | `admin.access`, `role.capability.read`, `role.capability.manage`, `admin.settings.read`, `admin.settings.manage`. |
| Catalog and directory | `catalog.item.read`, `catalog.item.create`, `catalog.item_group.manage`, `party.supplier.manage`, `party.customer.manage`. |
| Production | `production.map.manage`, `apparatus.queue.read`, `apparatus.queue.manage`, `raw_material.rule.manage`, `raw_material.assign`. |
| Warehouse legacy | `werka.access`, `werka.code.manage`. |
| Qolip | `qolip.manage`. |
| Printing and split | `gscale.catalog.read`, `gscale.print`, `rps.batch.manage`, `rezka.split.manage`. |
| Mobile state | `push.token.manage`, `supplier.avatar.manage`. |

Role assignment rules:

- role assignments are keyed by principal role and principal reference;
- assignment to a missing role package fails closed;
- assignment base role must match the principal role when a role package is
  bound to a base;
- assigned apparatus can scope production operators to specific machines;
- system roles are reserved and cannot be overwritten as custom roles.

## Request and Response Conventions

General conventions:

- bearer auth is passed through the `Authorization` header;
- handlers trim id-like string inputs before validation;
- list endpoints clamp or default `limit` and `offset` values per route;
- unsupported methods are rejected explicitly instead of falling through;
- JSON responses keep mobile-compatible field names even when internal Rust
  modules use clearer domain names;
- successful mutation responses include the updated entity or enough state for
  mobile to refresh the affected screen;
- push and live-notification side effects are best effort unless the endpoint is
  explicitly a print/push endpoint.

Common response shapes:

```json
{"ok":true}
```

```json
{"ok":false,"error":"forbidden"}
```

Some legacy routes return `{"error":"..."}` without `ok`; route tests protect
those shapes. Do not normalize response bodies globally without updating mobile
and tests together.

## Data Ownership

PostgreSQL tables are grouped by domain. The exact schema lives in
`migrations/postgres/0001_mini_erp_foundation.sql`; this section documents
ownership and intent.

| Domain | Tables |
| --- | --- |
| Engine/idempotency | `mini_engine_events`, `mini_idempotency_keys` |
| Orders and quick calculation | `mini_orders`, `mini_order_products`, `mini_quick_order_templates`, `mini_quick_order_images` |
| Catalog | `mini_items`, `mini_item_groups` |
| Production maps | `mini_production_maps`, `mini_production_map_nodes`, `mini_production_map_edges` |
| Apparatus and workers | `mini_apparatus`, `mini_apparatus_groups`, `mini_workers`, `mini_worker_groups` |
| Queue state | `mini_queue_sequences`, `mini_queue_states`, `mini_apparatus_queue_policies`, `mini_queue_action_events` |
| Progress and WIP | `mini_order_run_sessions`, `mini_order_progress_events`, `mini_progress_batches` |
| Materials | `mini_apparatus_material_rules`, `mini_raw_material_assignments`, `mini_raw_material_stock`, `mini_gscale_receipts` |
| Finished goods | `mini_finished_goods_stock` |
| Warehouse and Qolip | `mini_warehouses`, `mini_warehouse_assignments`, `mini_qolip_product_specs`, `mini_qolip_locations`, `mini_qolip_cell_qrs`, `mini_qolip_checkouts` |
| RPS and push | `mini_rps_batches`, `mini_push_tokens` |

State ownership rules:

- production maps and queue state are changed only through
  `ProductionMapService`;
- WIP progress batches are created by queue actions, not by direct handler
  writes;
- Qolip location quantity changes are transactional and identity-checked;
- raw material stock status is updated through GScale/material store ports;
- push tokens can move between owners, but business writes do not depend on FCM
  delivery succeeding;
- local LMDB state is limited to runtime identity/session support, not core ERP
  production state.

## Failure and Error Contract

Handlers preserve a predictable mobile contract:

| Condition | HTTP behavior |
| --- | --- |
| Missing/invalid bearer token | `401` with `unauthorized`. |
| Valid user without required role/capability | `403` with `forbidden`. |
| Unsupported HTTP method | `405` with method error body. |
| Invalid JSON body | `400` with invalid JSON error. |
| Missing required id/input | `400` with domain-specific error. |
| Conflict such as stock mismatch or insufficient quantity | `409` where the handler maps it as a conflict. |
| Store/database failure | `500` or dependency-specific failure response. |
| Printer/driver failure | printing endpoint returns print-specific error; business state must remain explicit. |

Domain-level invariant failures should be explicit rather than silently repaired.
Examples:

- queue action not allowed;
- previous-stage QR missing or mismatched;
- raw material scan missing, duplicate, or mismatched;
- Qolip location identity mismatch;
- checkout not found or not returnable;
- database not configured for production ERP flows.

## Operational Runbook

### First Deployment

1. Provision PostgreSQL.
2. Set `MINI_ERP_DATABASE_URL`.
3. Set persistent LMDB directories.
4. Set `MOBILE_API_LOCAL_STORE_ALLOW_JSON_FALLBACK=0`.
5. Configure Firebase credentials if push is needed.
6. Configure `GEMINI_API_KEY` only if AI image search is needed.
7. Start with `make up-domain DOMAIN=... REQUIRE_DATABASE_URL=1`.
8. Verify `/healthz`.
9. Verify `/v1/mobile/admin/system/monitor`.
10. Run a small end-to-end smoke test before connecting real operators.

### Smoke Test Checklist

Run these in order after deploy:

1. `/healthz` returns `{"ok":true}`.
2. Admin login succeeds.
3. Warehouse/werka login succeeds.
4. Supplier and customer login still match mobile contract.
5. Admin monitor shows database reachable.
6. Admin can list production maps.
7. One production map can be run with a known `order_qty`.
8. Apparatus queue state can be read.
9. A safe test order can run `start -> pause -> resume -> complete`.
10. Progress QR lookup/report works for the generated batch.
11. WIP batch listing shows expected status.
12. Final-stage finished goods receive works on a safe test batch.
13. Qolip block/location/cell QR lookup works.
14. Qolip checkout and return work on a test location.
15. Push token register/delete works for a test device token.
16. GScale/RPS print endpoints are checked against a test driver if printing is
    enabled.

### Incident Checks

If the service is unhealthy:

- check process logs with `RUST_LOG=info` or a narrower module filter;
- check PostgreSQL connectivity and migrations;
- check LMDB directory permissions and disk space;
- check Cloudflare tunnel process and hostname route;
- check Firebase credentials only if push-specific flows fail;
- check print driver URL only if print endpoints fail;
- avoid switching to JSON fallback unless this is an explicit rollback action.

### Backup and Recovery

The monitor reads backup directory information from `MINI_ERP_BACKUP_DIR` when
configured. If it is not configured, it checks the default backup paths used by
the deployment scripts.

Recovery expectations:

- PostgreSQL backup is required for ERP state;
- LMDB directory backup is required for active sessions, push tokens, profile
  prefs, and admin local state;
- Firebase/Gemini credentials should be recoverable from secret storage, not
  from the repository;
- after restore, run the smoke test checklist before letting operators resume.

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
