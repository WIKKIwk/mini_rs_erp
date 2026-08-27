# Production Order Lifecycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist an authoritative production-order header lifecycle and use it for material and completed-order filtering without confusing operation completion with whole-order completion.

**Architecture:** `mini_production_maps` owns the lifecycle projection, while queue states and order-control states keep their existing responsibilities. Queue writes calculate the affected order's lifecycle from the current map plus the post-action queue-state snapshot, then persist the projection and transition event atomically through the existing store transaction.

**Tech Stack:** Rust, Tokio, Axum, SQLx, PostgreSQL, serde, existing in-memory production-map test store.

**Spec:** `docs/superpowers/specs/2026-08-26-production-order-lifecycle-design.md`

## Global Constraints

- Do not change the meaning of apparatus queue states.
- Do not merge freeze/hold into lifecycle.
- Do not infer `closed` or `cancelled` from queue actions.
- Keep current HTTP payloads backward compatible by adding fields only.
- Preserve the dirty worktree and do not commit, push, deploy, install, or run migrations against real data.

---

### Task 1: Core lifecycle projection and failing regression

**Files:**
- Create: `src/core/production_map/types/lifecycle.rs`
- Modify: `src/core/production_map/types.rs`
- Modify: `src/core/production_map/progress_session/closed_orders.rs`
- Modify: `src/core/production_map/progress_session/progress_status.rs`
- Test: `src/core/production_map/tests/service_flow.rs`

**Interfaces:**
- Produces: `ProductionOrderLifecycleStatus::{Released, InProgress, ProductionCompleted, Closed, Cancelled}`.
- Produces: `derive_production_order_lifecycle(map, queue_states) -> Result<ProductionOrderLifecycleStatus, ProductionMapError>`.
- Produces: additive `ProductionOrderStatusDetail.lifecycle_status: ProductionOrderLifecycleStatus`.

- [ ] **Step 1: Write the failing first-stage-complete regression test**

Create a two-stage map, mark only the first stage completed, and assert that `raw_material_assignment_orders()` still contains the order and that its lifecycle is not `production_completed`.

- [ ] **Step 2: Run the focused test and verify RED**

Run: `cargo test core::production_map::tests::service_flow::<new_test_name> -- --exact --nocapture`

Expected: failure because persisted lifecycle storage/API does not exist and the legacy projection can hide the order.

- [ ] **Step 3: Add the typed lifecycle and pure derivation**

The derivation returns `ProductionCompleted` only when every required physical apparatus returned by the existing closed-order apparatus rule has a completed state; otherwise any non-pending state returns `InProgress`, and all-pending/missing returns `Released`.

- [ ] **Step 4: Remove the unsafe legacy completion fallback**

Change the `completed_queue_count > 0` fallback in `derive_order_status` to a non-terminal display state so one completed operation cannot emit whole-order `completed`.

- [ ] **Step 5: Run the focused test and verify GREEN**

Run the exact test from Step 2 and require PASS.

### Task 2: Store contract and in-memory transactional behavior

**Files:**
- Modify: `src/core/production_map/store_port_parts/part_01.rs`
- Modify: `src/core/production_map/store_port_parts/part_02.rs`
- Modify: `src/core/production_map/memory_store/state.rs`
- Modify: `src/core/production_map/catalog/memory_store.rs`
- Modify: `src/core/production_map/memory_store/queue.rs`
- Modify: `src/core/production_map/memory_store_trait_impl.rs`
- Test: `src/core/production_map/tests/service_flow.rs`

**Interfaces:**
- Produces: `ProductionOrderLifecycleRecord { order_id, status, completion_outcome, transitioned_at_unix }`.
- Produces: `ProductionMapStorePort::production_order_lifecycles(&[String])`.
- Consumes: a lifecycle transition carried in `QueueActionProgressWrite` and `CompletionRequestStateResolution`.

- [ ] **Step 1: Add a failing full-order completion test**

Complete both stages and assert `production_completed`; also assert that a repeated read does not change transition history.

- [ ] **Step 2: Verify RED**

Run the exact new test and require the missing lifecycle assertion to fail.

- [ ] **Step 3: Add lifecycle storage to the memory store**

Initialize a map as `released`, apply lifecycle updates at the existing queue progress commit boundary, and expose batched lifecycle reads.

- [ ] **Step 4: Pass the post-action lifecycle through queue preparation and completion approval**

Overlay the affected apparatus's post-action states onto the all-apparatus snapshot, derive one lifecycle record for the affected order, and include it in the existing atomic write object.

- [ ] **Step 5: Verify GREEN and run production-map core tests**

Run: `cargo test core::production_map::tests -- --nocapture`

Expected: PASS.

### Task 3: PostgreSQL schema and atomic persistence

**Files:**
- Create: `migrations/postgres/0077_production_order_lifecycle.sql`
- Modify: `src/db/postgres_parts/part_01.rs`
- Create: `src/db/postgres_production_map/lifecycle.rs`
- Modify: `src/db/postgres_production_map.rs`
- Modify: `src/db/postgres_production_map_impl_parts/part_01.rs`
- Modify: `src/db/postgres_production_map_impl_parts/part_02.rs`
- Modify: `src/db/postgres_production_map/completion/requests.rs`
- Modify: `src/db/postgres_production_map_trait_impl.rs`
- Test: `src/db/tests/production_map.rs`

**Interfaces:**
- Produces columns `lifecycle_status`, `completion_outcome`, `lifecycle_changed_at`, `production_completed_at`, `closed_at`, and `lifecycle_version` on `mini_production_maps`.
- Produces append-only `mini_production_order_lifecycle_events`.
- Produces batched SQL lifecycle reads by order ID and terminal/active status.

- [ ] **Step 1: Add a failing Postgres lifecycle persistence test**

Save a two-stage map, persist first-stage completion, and assert the map row is `in_progress`; then complete the final stage and assert `production_completed` plus exactly one matching transition event.

- [ ] **Step 2: Verify RED against the local test database**

Run the repository's existing exact Postgres test command for the new test; expected failure is missing migration columns/table.

- [ ] **Step 3: Add migration and one-time backfill**

Backfill each existing map from required physical apparatus and queue states, add status constraints and partial indexes, and register migration `0077` after the already-present `0076` entry without modifying `0076`.

- [ ] **Step 4: Persist lifecycle in existing transactions**

Update the map row and append an event only when the status changes. Apply this in normal queue progress commits and completion-request approval commits before transaction commit.

- [ ] **Step 5: Verify GREEN**

Re-run the exact Postgres test and require PASS.

### Task 4: Indexed candidate and completed-order reads

**Files:**
- Modify: `src/core/production_map/materials/implementation_impl_parts/part_01.rs`
- Modify: `src/core/production_map/catalog/service_parts/part_02.rs`
- Modify: `src/core/production_map/store_port_parts/part_02.rs`
- Modify: `src/core/production_map/catalog/memory_store.rs`
- Modify: `src/db/postgres_production_map/lifecycle.rs`
- Test: `src/http/admin_route_tests/raw_materials/assignment_flow.rs`
- Test: `src/core/production_map/tests/order_control.rs`

**Interfaces:**
- Consumes: one batched lifecycle map for candidate filtering.
- Consumes: terminal lifecycle IDs for completed-order audit hydration.

- [ ] **Step 1: Add route regression for first-stage completion**

Assert that the material candidate endpoint still returns an order whose first operation is complete and whose later required operation is incomplete.

- [ ] **Step 2: Verify RED**

Run the exact route test and require the current candidate omission to fail.

- [ ] **Step 3: Replace N+1 derived filtering**

Load lifecycle records once and filter only `production_completed`, `closed`, and `cancelled`; do not call `order_status_detail` inside the map loop.

- [ ] **Step 4: Start completed-order hydration from persisted lifecycle**

Select only `production_completed` or `closed` candidates, while preserving existing audit-log and progress-batch response fields.

- [ ] **Step 5: Verify GREEN**

Run the focused route, material, and fully-completed-order tests and require PASS.

### Task 5: Compatibility and regression verification

**Files:**
- Modify only if required by compiler: `accord_mobile_v2/lib/src/core/api/admin/mobile_api_admin.dart`
- Test only if mobile model changes: the existing focused production-order status tests.

**Interfaces:**
- Consumes: additive JSON `lifecycle_status`.
- Preserves: all existing JSON keys and legacy status display behavior except the incorrect one-operation whole-order completion fallback.

- [ ] **Step 1: Compile-check all Rust consumers**

Run: `cargo check`

Expected: PASS without new warnings.

- [ ] **Step 2: Run targeted backend regression suites**

Run the production-map core tests, raw-material assignment route tests, and Postgres production-map tests sequentially.

- [ ] **Step 3: Check mobile compatibility**

If no Dart model edit is required, run no Flutter mutation. If the typed mobile model is extended, run its targeted tests and `flutter analyze` only for the changed scope using the repository SDK.

- [ ] **Step 4: Review the final diff**

Confirm no unrelated dirty files were modified, no production environment was accessed, and no commit/push/deploy occurred.

