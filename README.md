# Accord MES Backend

`mini_rs_erp` is the backend of the Accord Manufacturing Execution System (MES).
It manages production orders, equipment queues, material movements, and product
traceability for printing, lamination, and cutting operations. The
[Accord Mobile V2](https://github.com/WIKKIwk/accord_mobile_v2) application
provides the operator and administrative interface.

## Functional Scope

| Area | Capabilities |
| --- | --- |
| Production planning | Order calculations, production routes, equipment assignment, and queue sequencing. |
| Shop floor execution | Operator work sessions, start/pause/resume/complete actions, and production metrics. |
| Traceability | QR identification, work in progress (WIP), stage lineage, pallet tracking, and finished goods receiving. |
| Material management | Raw material inventory, order assignments, scan validation, preparation receipts, and consumption. |
| Tooling | Mold inventory, storage locations, checkout, return, and transfers. |
| Warehouse operations | Supplier receipts, stock movements, customer dispatch, and delivery confirmation. |
| Administration | Users, roles, capabilities, catalogs, activity records, system monitoring, and database backup operations. |

Integrations include GScale/RPS weighing and label printing, internal messaging,
Firebase push notifications, Telegram workflows, and Android update distribution.

## Architecture

The service is implemented in Rust using Axum, Tokio, and SQLx.

- PostgreSQL is required and stores authoritative production and inventory state.
- Domain services validate operations and persist changes through store interfaces.
- LMDB stores supporting local state, including sessions and profile preferences.
- Access is controlled by bearer sessions, roles, capabilities, and resource assignments.
- Production actions record work sessions, progress events, and WIP batch history.

The mobile API retains the `/v1/mobile/*` namespace. Current endpoint definitions
are maintained in [the HTTP router](src/http/router), and the versioned database
schema is maintained in [PostgreSQL migrations](migrations/postgres).

## Setup

Requirements:

- Rust and Cargo with Rust 2024 edition support.
- A PostgreSQL database and suitable runtime and migration credentials.
- Persistent storage for local state, uploaded files, and encryption keys.

Run commands from the repository root. Configure these variables through the
process environment; the server also loads a local `.env` file.

| Variable | Purpose |
| --- | --- |
| `MINI_ERP_DATABASE_URL` | Required PostgreSQL connection URL for the application. |
| `MINI_ERP_MIGRATION_DATABASE_URL` | Optional schema management connection URL; defaults to the application URL. |
| `MOBILE_API_ADDR` | HTTP bind address; defaults to `0.0.0.0:8081`. |

Apply the migration set with the database URLs exported in the process environment:

```bash
make db-migrate
```

Complete the required [account credential initialization](docs/auth-credential-migration.md)
before starting the server. Account access codes are managed in PostgreSQL;
runtime login does not use password values from `.env`.

Start the development server:

```bash
cargo run --bin mini_rs_erp
```

Check HTTP availability:

```bash
curl http://127.0.0.1:8081/healthz
```

The expected response is `{"ok":true}`.

Build and run the release executable:

```bash
cargo build --release --locked --bin mini_rs_erp
./target/release/mini_rs_erp
```

Deploy the API behind HTTPS. Preserve the database, local state, uploaded files,
and encryption keys across deployments and include them in recovery procedures.
Encryption key requirements are documented in [account code storage](docs/admin-access-codes.md).
Keychain-managed Mac installations use the [Mac runtime procedure](docs/macos-postgres-access.md).

## Verification

Contract verification requires Python 3 and `uv` in addition to the Rust toolchain.

```bash
cargo fmt --check
make verify
cargo clippy --locked --bins
```

`make verify` checks HTTP contracts against the actual Axum router. Domain and
PostgreSQL behavior also have focused Rust test suites.

## Repository Structure

| Path | Responsibility |
| --- | --- |
| `src/core/` | Domain models, services, validation, and store interfaces. |
| `src/db/` | PostgreSQL repositories and persistence operations. |
| `src/http/` | HTTP routing, authentication checks, and request handlers. |
| `src/app.rs`, `src/main.rs` | Service composition and startup. |
| `crates/` | Shared domain components and the gateway package. |
| `migrations/postgres/` | Versioned database schema changes. |
| `tools/` | Verification, database maintenance, and deployment tooling. |

## Documentation

- [WIP route continuity](docs/wip-route-continuity.md)
- [Warehouse QR receiving](docs/warehouse-qr-preview.md)

## License

[Apache License 2.0](LICENSE).
