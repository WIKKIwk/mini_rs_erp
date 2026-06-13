# Mini RS ERP PostgreSQL Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the first clean PostgreSQL and RS engine foundation for the mini ERP without copying ERPNext doctypes into the new schema.

**Architecture:** Keep business rules in `src/core`, transactional safety in `src/engine`, and PostgreSQL wiring in `src/db/postgres`. The first increment creates migration/config/engine primitives only; existing SQLite/LMDB stores remain untouched until later migrations.

**Tech Stack:** Rust 2024, Tokio, SQLx PostgreSQL, SQL migration files, existing Cargo test harness.

---

### Task 1: PostgreSQL Foundation

**Files:**
- Create: `migrations/postgres/0001_mini_erp_foundation.sql`
- Create: `src/db/mod.rs`
- Create: `src/db/postgres.rs`
- Modify: `src/main.rs`
- Modify: `Cargo.toml`

- [x] **Step 1: Write failing tests for PostgreSQL config and migration content**

Run: `cargo test postgres_config_uses_mini_erp_database_url postgres_foundation_migration_defines_core_tables --quiet`

- [ ] **Step 2: Implement PostgreSQL config parser and migration constants**

Add `PostgresConfig` that reads `MINI_ERP_DATABASE_URL` and safe pool defaults.

- [ ] **Step 3: Add migration for first mini ERP tables**

Create tables for orders, quick templates, production maps, apparatus, queue state, engine events, and idempotency keys.

### Task 2: RS Engine Foundation

**Files:**
- Create: `src/engine/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write failing tests for idempotency key validation and engine event creation**

Run: `cargo test engine_context_rejects_blank_idempotency_key engine_event_records_domain_and_action --quiet`

- [ ] **Step 2: Implement minimal engine primitives**

Add `EngineCommandContext`, `EngineEventDraft`, `EngineError`, and validation helpers.

### Task 3: Verification

**Files:**
- All modified files

- [ ] **Step 1: Run focused tests**

Run: `cargo test postgres_ engine_ --quiet`

- [ ] **Step 2: Run compile check**

Run: `cargo check --quiet`

- [ ] **Step 3: Commit**

Run:

```bash
git add Cargo.toml Cargo.lock src/db src/engine src/main.rs migrations/postgres docs/superpowers/plans/2026-06-13-mini-rs-erp-psql-engine.md
git commit -m "Add mini ERP PostgreSQL engine foundation"
```
