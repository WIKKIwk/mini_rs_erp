# ERPNext Manufacturing Order Log Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Save opened RS production-map orders into ERPNext Manufacturing Work Order through official ERPNext REST API.

**Architecture:** Add a non-blocking ERP manufacturing sink beside the Google Sheets sink. The sink builds a Work Order payload from `ProductionMapDefinition` and `CalculateOrderTemplate`, using Work Order operations for apparatus steps and operation descriptions for size/rubber/layer metadata.

**Tech Stack:** Rust, Axum server, reqwest ERPNext client, ERPNext/Frappe REST API, existing RS test harness.

---

### Task 1: Payload Builder

**Files:**
- Create: `src/erpnext/production_order.rs`
- Modify: `src/erpnext/mod.rs`

- [x] Write a failing unit test that maps one order with two apparatus nodes into a Work Order payload.
- [x] Run the focused test and verify it fails because the builder does not exist.
- [x] Implement `build_work_order_payload` with fields: `production_item`, `qty`, `description`, `operations`, `sequence_id`, `workstation`, `operation`, `batch_size`.
- [x] Run the focused test and verify it passes.

### Task 2: ERP Sink Integration

**Files:**
- Modify: `src/erpnext/production_order.rs`
- Modify: `src/app.rs`
- Modify: `src/http/handlers/admin/production_maps.rs`

- [x] Write a failing unit test proving the save handler calls the ERP sink for a new real order without blocking the response on sink error.
- [x] Run the test and verify it fails.
- [x] Add `ProductionOrderErpSink` trait, noop sink, and `ErpnextClient` implementation using `POST /api/resource/Work Order`.
- [x] Wire `AppState` to hold the sink and call it after successful order save.
- [x] Run the focused tests and verify they pass.

### Task 3: Verification And Deploy

**Files:**
- Modified files from tasks 1-2

- [x] Run `cargo fmt`, `cargo check`, focused tests, and clippy dead-code check.
- [x] Commit the changes.
- [x] Build linux x86_64 release binary through Docker/OrbStack.
- [x] Deploy to Fedora and verify service logs.
