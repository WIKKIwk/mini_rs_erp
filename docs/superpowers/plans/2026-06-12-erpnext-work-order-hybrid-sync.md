# ERPNext Work Order Hybrid Sync Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Read RS-created ERPNext Work Orders back into the RS production map cache while keeping SQLite for UI queue/cache state.

**Architecture:** ERPNext becomes the authoritative read source for opened production orders that contain `RS map id:` in the Work Order description. RS periodically fetches those Work Orders through official REST API, rebuilds `ProductionMapDefinition` entries from Work Order operations, and upserts them into the existing SQLite-backed `ProductionMapService` without deleting local state.

**Tech Stack:** Rust, Axum service state, reqwest ERPNext client, ERPNext/Frappe REST API, existing production map SQLite cache.

---

### Task 1: Work Order To Production Map Conversion

**Files:**
- Modify: `src/erpnext/production_order.rs`

- [x] Write a failing unit test for converting a Work Order with two operations into `ProductionMapDefinition`.
- [x] Implement Work Order response structs and conversion helper.
- [x] Verify the focused unit test passes.

### Task 2: ERPNext Read Source

**Files:**
- Modify: `src/erpnext/production_order.rs`

- [x] Add `ProductionOrderErpSource` trait and noop source.
- [x] Implement source for `ErpnextClient` using `GET /api/resource/Work Order`.
- [x] Fetch detail documents by name and convert only rows containing `RS map id:`.
- [x] Verify focused tests still pass.

### Task 3: RS Cache Sync Loop

**Files:**
- Modify: `src/core/production_map/mod.rs`
- Modify: `src/app.rs`
- Modify: `src/http/admin_route_tests.rs`

- [x] Add `ProductionMapService::upsert_maps_batch`.
- [x] Add AppState background sync loop.
- [x] Configure interval with `ERP_WORK_ORDER_SYNC_INTERVAL_SECONDS`; `0` means run once.
- [x] Keep SQLite queue states and sequences untouched.
- [x] Verify `cargo fmt`, focused tests, `cargo check`, and clippy dead-code check.

### Task 4: Commit And Deploy

**Files:**
- Modified files from Tasks 1-3

- [x] Commit the implementation.
- [x] Push `main`.
- [x] Build Linux amd64 release through Docker.
- [x] Deploy to Fedora and verify service health.
