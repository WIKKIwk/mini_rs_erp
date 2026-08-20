# Apparatus Standardization Coordination

This file is the coordination baseline for future apparatus-standardization work in this isolated candidate. It is an inventory and ownership map only. No apparatus standard, business-logic refactor, or migration change is implemented here.

## Baseline and isolation

- Source: `/Users/wikki/Desktop/Accord_project/mini_rs_erp`
- Candidate: `/Users/wikki/Desktop/Accord_project/mini_rs_erp_apparatus_standard`
- Baseline `HEAD`: `0c9ab7a1db47fd6eb677769124121694efdcfd57`
- Baseline commit: `refactor: sanitize ERP module layout without behavior changes`
- Source cleanliness at copy time: clean working tree. `git status --short --branch` reported only `## main...origin/main [ahead 3]`; it reported no staged, unstaged, or untracked changes.
- Copy method: local Git clone preserving the repository history and current `HEAD`. The source repository was not modified.
- Current scope: read-only inventory plus this document. Do not add implementation or migration changes as part of this coordination task.

## Apparatus-domain inventory

The inventory was obtained from the target by searching `src/`, `migrations/postgres/`, and tests for apparatus, queue, worker, capacity, schedule, production-map, material, WIP, and training references, then checking the registered admin routes.

### Catalog, master data, workers, and location assignment

- Core contracts and services: `src/core/apparatus_groups.rs`, `src/core/workers.rs`, `src/core/worker_groups.rs`, `src/core/worker_groups/normalize.rs`, `src/core/worker_groups/store.rs`, `src/core/factory_locations.rs`.
- SQLite persistence: `src/store/apparatus_group_store.rs`.
- PostgreSQL persistence: `src/db/postgres_apparatus_group.rs`, `src/db/postgres_worker.rs`, `src/db/postgres_worker_group.rs`, `src/db/postgres_factory_location.rs`.
- Admin handlers: `src/http/handlers/admin/system/apparatus.rs`, `src/http/handlers/admin/workers.rs`, `src/http/handlers/admin/workers_profiles.rs`, `src/http/handlers/admin/system/factory_locations.rs`.
- Application wiring and adjacent auth/role surfaces: `src/app/builders.rs`, `src/app.rs`, `src/core/auth/models.rs`, `src/core/auth/service/worker_login.rs`, `src/core/authz/models.rs`, `src/core/authz/normalize.rs`, `src/core/authz/queries.rs`, `src/core/authz_catalog.rs`, `src/core/worker_groups/tests.rs`.

### Production-map, queue, execution, progress, WIP, and transfer

- Apparatus identity and queue contracts: `src/core/production_map/apparatus.rs` (includes `queue/apparatus.rs`), `src/core/production_map/queue/apparatus_identity.rs`, `src/core/production_map/queue_state.rs`, `src/core/production_map/queue_state/apparatus.rs`.
- Queue lifecycle: `src/core/production_map/queue/{actions,apparatus,apparatus_identity,execution,mod,sequence,service,state,support,types}.rs`.
- Map/catalog/compiler contracts that carry apparatus nodes: `src/core/production_map/{mod,service,store_port,types}.rs`, `src/core/production_map/types/*.rs`, `src/core/production_map/catalog/*.rs`, `src/core/production_map/chain/*.rs`, `src/core/production_map/compiler/*.rs`, `src/core/production_map/formula*.rs`.
- Execution and downstream flow: `src/core/production_map/progress_session/*.rs`, `src/core/production_map/service_order_control.rs`, `src/core/production_map/order_control/service.rs`, `src/core/production_map/service_transfer.rs`, `src/core/production_map/transfer/service.rs`, `src/core/production_map/service_wip.rs`, `src/core/production_map/wip/service.rs`, `src/core/production_map/service_astatka.rs`, `src/core/production_map/astatka/service.rs`.
- PostgreSQL persistence: `src/db/postgres_production_map.rs` and its `catalog/`, `queue/`, `progress/`, `order_control/`, `completion/`, `transfer/`, `wip/`, `qolip/`, `astatka/`, and `capacity/` helper files.
- SQLite/legacy production store: `src/store/production_map_store.rs` and its `capacity.rs`, `maps.rs`, `map_helpers.rs`, `queue.rs`, `materials/unsupported.rs`, `unsupported_materials.rs`, and `migration.rs` helpers.
- Admin handlers: `src/http/handlers/admin/production_maps.rs`, `production_maps/{helpers,move_run,queue_actions,queue_action_completion_support,completion,progress_qr,wip,astatka,raw_material_details,raw_material_reprint,raw_materials,raw_materials_history,qolip_validation,qolip_order_notes,paddons,order_control}.rs`, and `src/http/handlers/admin/production_maps_save_helpers.rs`.

### Capacity and scheduling

- Core: `src/core/production_map/capacity/{mod,memory_store,scheduler,service,tests}.rs`, `src/core/production_map/service_capacity.rs`, `src/core/production_map/service_capacity_scheduler.rs`.
- Persistence: `src/db/postgres_production_map/capacity_helpers.rs`, `src/db/postgres_production_map/capacity/helpers.rs`.
- HTTP: the capacity and schedule handlers in `src/http/handlers/admin/production_maps.rs` and the registrations listed below.

### Materials, specialized stage flows, returned paint, and training

- Material rules/stock: `src/core/production_map/materials/*.rs`, `src/db/postgres_production_map/materials/*.rs`, `src/db/postgres_production_map/material_helpers.rs`, `src/db/postgres_production_map/raw_material_stock_helpers.rs`, `src/db/postgres_raw_material_events.rs`, `src/http/handlers/admin/production_maps/raw_material*.rs`.
- Stage-specific reports: `src/core/production_map/astatka/service.rs`, `src/core/production_map/service_astatka.rs`, `src/db/postgres_production_map/astatka*.rs`, `src/http/handlers/admin/production_maps/astatka.rs`, `src/core/rezka/*.rs`, `src/core/returned_paint*.rs`, `src/db/postgres_returned_paint.rs`, `src/http/handlers/returned_paint.rs`.
- Training workspace: `src/http/handlers/admin/training.rs`, `src/db/postgres_training_workspace.rs`, `src/db/postgres_training_workspace_delete.rs`, and the training-specific tests and migrations listed below.

## Apparatus-related database tables and migration ownership

The table inventory is grouped by the migration that establishes or materially changes the contract.

- Foundation and map execution in `migrations/postgres/0001_mini_erp_foundation.sql`: `mini_production_maps`, `mini_production_map_nodes`, `mini_production_map_edges`, `mini_apparatus_groups`, `mini_apparatus`, `mini_workers`, `mini_worker_groups`, `mini_queue_sequences`, `mini_queue_states`, `mini_apparatus_queue_policies`, `mini_queue_action_events`, `mini_order_run_sessions`, `mini_order_progress_events`, `mini_progress_batches`, `mini_apparatus_material_rules`, `mini_raw_material_assignments`, `mini_raw_material_stock`, `mini_raw_material_events`.
- Apparatus-linked location: `mini_factory_locations` and `mini_factory_location_apparatus_links` in `migrations/postgres/0028_factory_locations.sql`.
- Durable transfer: `mini_apparatus_order_transfers` in `migrations/postgres/0032_apparatus_order_transfers.sql`.
- Capacity/scheduling: `mini_apparatus_capacity_profiles`, `mini_apparatus_downtimes`, and `mini_apparatus_schedule_reservations` in `migrations/postgres/0034_apparatus_capacity_scheduling.sql`; paused scheduling status in `0035_apparatus_schedule_paused_status.sql`; identity constraints/FKs in `0055_apparatus_capacity_identity.sql`.
- Master metadata: payload updates to `mini_apparatus` in `migrations/postgres/0033_apparatus_master_metadata.sql`.
- Apparatus-specific outputs: `mini_laminatsiya_astatka_reports` (`0040_laminatsiya_astatka_reports.sql`), `mini_rezka_astatka_reports` (`0041_rezka_astatka_reports.sql`), `mini_returned_paint_requests` (`0006_boyoqchi_returned_paint.sql`), and `mini_returned_paint_images` (`0010_returned_paint_image_workflow.sql`).
- Training mirror/workspace: `mini_training_production_maps`, `mini_training_quick_order_templates`, `mini_training_raw_material_assignments`, `mini_training_apparatus_modes`, `mini_training_order_images` (`0052_training_workspace.sql`), `mini_training_queue_states` (`0053_training_queue_states.sql`), `mini_training_returned_paint_reports` (`0054_training_returned_paint.sql`), `mini_training_queue_events` (`0056_training_queue_events.sql`), `mini_training_input_batches` (`0057_training_input_batches.sql`), and `mini_training_progress_batches` (`0058_training_progress_batches.sql`), with input-batch-set changes in `0059_training_input_batch_sets.sql`.
- Cross-cutting queue contract changes: `0060_frozen_order_queue_state.sql` changes allowed states/actions on `mini_queue_states`, `mini_queue_action_events`, `mini_order_run_sessions`, and `mini_order_progress_events`.

Migration files are append-only, ordered production contracts. No parallel worker may add, reorder, or edit a migration without integrator ownership and an explicit integration decision.

## Registered apparatus-related routes

Routes are registered in `src/http/router/mobile/admin.rs` unless noted otherwise.

### Catalog and assignment

```text
/v1/mobile/admin/apparatus
/v1/mobile/admin/apparatus/options
/v1/mobile/admin/apparatus-groups
/v1/mobile/admin/workers
/v1/mobile/admin/workers/delete-check
/v1/mobile/admin/workers/detail
/v1/mobile/admin/workers/profile-detail
/v1/mobile/admin/workers/code/regenerate
/v1/mobile/admin/worker-groups
/v1/mobile/admin/factory-locations/{id}/apparatus
```

### Map, queue, execution, and scheduling

```text
/v1/mobile/admin/production-maps
/v1/mobile/admin/production-maps/run
/v1/mobile/admin/production-maps/with-order
/v1/mobile/admin/production-maps/audit
/v1/mobile/admin/production-maps/capacity
/v1/mobile/admin/production-maps/capacity/downtime
/v1/mobile/admin/production-maps/schedule
/v1/mobile/admin/production-maps/schedule/cancel
/v1/mobile/admin/production-maps/move
/v1/mobile/admin/production-maps/move-batch
/v1/mobile/admin/production-maps/apparatus-transfer
/v1/mobile/admin/production-maps/sequence
/v1/mobile/admin/production-maps/queue-policies
/v1/mobile/admin/production-maps/live
/v1/mobile/admin/production-maps/queue-action
/v1/mobile/admin/production-maps/completed-orders
/v1/mobile/admin/production-maps/closed-orders
/v1/mobile/admin/production-maps/completion-requests
/v1/mobile/admin/production-maps/completion-requests/decision
/v1/mobile/admin/production-maps/completion-request-decisions
/v1/mobile/admin/production-maps/progress-qr/lookup
/v1/mobile/admin/production-maps/progress-qr/history
/v1/mobile/admin/production-maps/progress-qr/correct
/v1/mobile/admin/production-maps/progress-qr/report
/v1/mobile/admin/production-maps/progress-qr/reprint
/v1/mobile/admin/production-maps/wip-batches
/v1/mobile/admin/production-maps/finished-goods/receive
```

### Materials, stage-specific flows, and training

```text
/v1/mobile/admin/raw-material-rules
/v1/mobile/admin/raw-material-start-requirements
/v1/mobile/admin/raw-material-assignments
/v1/mobile/admin/raw-material-assignments/lookup
/v1/mobile/admin/raw-material-assignments/orders
/v1/mobile/admin/raw-material-assignments/candidates
/v1/mobile/admin/raw-material-assignments/candidate-orders
/v1/mobile/admin/raw-material-intake
/v1/mobile/admin/raw-material-intake-candidates
/v1/mobile/admin/raw-material-history
/v1/mobile/admin/raw-material-stock
/v1/mobile/admin/raw-material-stock/reprint/prepare
/v1/mobile/admin/raw-material-stock/reprint/confirm
/v1/mobile/admin/production-maps/laminatsiya-astatka
/v1/mobile/admin/production-maps/rezka-astatka
/v1/mobile/admin/training/apparatus
/v1/mobile/admin/training/production-maps
/v1/mobile/admin/training/production-maps/with-order
/v1/mobile/admin/training/input-batches
/v1/mobile/admin/training/raw-material-assignments
/v1/mobile/admin/training/completed-orders
/v1/mobile/admin/training/statuses
/v1/mobile/admin/training/restart
/v1/mobile/admin/training/images
/v1/mobile/admin/training/images/view
```

The training route list above includes only paths confirmed by the router; if a path is not registered in this baseline, workers must not invent it.

## Tests covering the apparatus surface

- Core behavior: `src/core/production_map/capacity/tests.rs`, `src/core/production_map/queue_state/tests.rs`, `src/core/production_map/chain/tests.rs`, `src/core/production_map/tests/{audit,fixtures,map_edit,order_control,service_flow}.rs`, `src/core/worker_groups/tests.rs`, `src/core/returned_paint_inline_tests.rs`.
- Database behavior: `src/db/tests/apparatus_group.rs`, `src/db/tests/apparatus_identity.rs`, `src/db/tests/production_map.rs`, `src/db/tests/worker.rs`, `src/db/tests/worker_group.rs`, `src/db/tests/training_workspace.rs`, `src/db/tests.rs`, `src/db/postgres_inline_tests.rs`.
- HTTP behavior: `src/http/admin_route_tests/warehouses_groups.rs`, `workers.rs`, `qolipchi_workers.rs`, `factory_locations.rs`, `production_map_basic.rs`, `production_map_save_order.rs`, `production_map_validation.rs`, `batch_move_basic.rs`, `batch_move_advanced.rs`, `queue_history.rs`, `queue_progress/*.rs`, `completion_rejections.rs`, `completion_requests.rs`, `run_capabilities.rs`, `raw_materials*.rs`, `raw_materials/*.rs`, `qolip_checkout.rs`, `boyoqchi_returned_paint.rs`, and `src/http/router_tests/core_routes.rs`.
- End-to-end reset/cleanup coverage: `tests/order_reset_e2e.rs`.

## Single-writer shared/core files

These files are shared contracts or registries. They must have one integrator owner for the duration of a parallel change set; workers may read them and submit requested edits, but must not edit them directly:

- Module and application wiring: `src/core/mod.rs`, `src/core/production_map/mod.rs`, `src/db/mod.rs`, `src/store/mod.rs`, `src/http/mod.rs`, `src/http/handlers/mod.rs`, `src/http/handlers/admin/mod.rs`, `src/http/handlers/admin/system.rs`, `src/http/router.rs`, `src/http/router/mobile.rs`, `src/http/router/mobile/admin.rs`, `src/app.rs`, `src/app/builders.rs`.
- Shared production contracts: `src/core/production_map/service.rs`, `src/core/production_map/store_port.rs`, `src/core/production_map/types.rs`, `src/core/production_map/types/*.rs`, `src/core/production_map/errors.rs`, and shared auth capability definitions in `src/core/authz*.rs`.
- Shared PostgreSQL dispatch/wiring: `src/db/postgres.rs`, `src/db/postgres_production_map.rs`.
- Migration sequence and shared schema: all files under `migrations/postgres/`, especially `0001_mini_erp_foundation.sql`, `0032_apparatus_order_transfers.sql`, `0033_apparatus_master_metadata.sql`, `0034_apparatus_capacity_scheduling.sql`, `0035_apparatus_schedule_paused_status.sql`, `0055_apparatus_capacity_identity.sql`, and `0060_frozen_order_queue_state.sql`.
- Test/module aggregators: `src/db/tests.rs`, `src/http/admin_route_tests.rs`, and any shared test fixtures/helpers used by more than one assigned subsystem.

If a change needs one of these files, the worker must surface the request to the integrator with the intended contract and affected owners. Do not make a private parallel copy of a shared contract.

## Proposed non-overlapping ownership map

Each worker owns only the paths in one row. Shared files above remain integrator-owned and are excluded from worker scopes.

| Worker | Exclusive ownership | Primary verification |
| --- | --- | --- |
| Catalog/master identity | `src/core/apparatus_groups.rs`; `src/store/apparatus_group_store.rs`; `src/db/postgres_apparatus_group.rs`; `src/http/handlers/admin/system/apparatus.rs`; `src/db/tests/apparatus_group.rs`; `src/db/tests/apparatus_identity.rs`; apparatus cases in `src/http/admin_route_tests/warehouses_groups.rs` | Catalog/group and metadata route tests |
| Workforce and factory assignment | `src/core/workers.rs`; `src/core/worker_groups/**`; `src/core/factory_locations.rs`; `src/db/postgres_worker.rs`; `src/db/postgres_worker_group.rs`; `src/db/postgres_factory_location.rs`; `src/http/handlers/admin/workers.rs`; `src/http/handlers/admin/workers_profiles.rs`; `src/http/handlers/admin/system/factory_locations.rs`; worker/location tests | `src/db/tests/{worker,worker_group}.rs`; `src/http/admin_route_tests/{workers,qolipchi_workers,factory_locations}.rs` |
| Map definition and compiler | `src/core/production_map/catalog/**`; `src/core/production_map/chain/**`; `src/core/production_map/compiler/**`; `src/core/production_map/formula*.rs`; `src/db/postgres_production_map/catalog/**`; map-definition portions of `src/http/handlers/admin/production_maps.rs` and `production_maps_save_helpers.rs`; map tests | `src/core/production_map/tests/{fixtures,map_edit,service_flow}.rs`; `src/db/tests/production_map.rs`; map route tests |
| Queue, execution, progress, WIP, and transfer | `src/core/production_map/queue/**`; `src/core/production_map/queue_state/**`; `src/core/production_map/progress_session/**`; `src/core/production_map/order_control/**`; `src/core/production_map/transfer/**`; `src/core/production_map/wip/**`; `src/core/production_map/service_order_control.rs`; `service_transfer.rs`; `service_wip.rs`; queue/progress/order-control/transfer/WIP DB helpers; matching queue, completion, progress, move, and WIP handlers; matching route tests | Queue-state, service-flow, queue-progress, completion, move, WIP, and `tests/order_reset_e2e.rs` coverage |
| Capacity and scheduling | `src/core/production_map/capacity/**`; `src/core/production_map/service_capacity*.rs`; `src/db/postgres_production_map/capacity/**`; capacity/schedule handler functions in `src/http/handlers/admin/production_maps.rs`; `src/http/admin_route_tests/run_capabilities.rs`; `src/core/production_map/capacity/tests.rs`; migration review for `0034`, `0035`, and `0055` is integrator-requested, not worker-owned | Capacity and capability tests |
| Materials and specialized apparatus flows | `src/core/production_map/materials/**`; `src/db/postgres_production_map/materials/**`; `src/db/postgres_production_map/material_helpers.rs`; `raw_material_stock_helpers.rs`; `src/db/postgres_raw_material_events.rs`; raw-material handlers/tests; `src/core/production_map/astatka/**`; `service_astatka.rs`; `src/core/rezka/**`; `src/core/returned_paint*.rs`; corresponding DB/HTTP handlers and tests | Raw-material, astatka, rezka, returned-paint, and WIP integration tests |
| Training workspace | `src/http/handlers/admin/training.rs`; `src/db/postgres_training_workspace.rs`; `src/db/postgres_training_workspace_delete.rs`; `src/db/tests/training_workspace.rs`; training route tests; migrations `0052`–`0059` only through integrator-coordinated proposals | Training workspace tests |

The ownership rows are intentionally exclusive. A worker may request an integrator change to a shared file or another subsystem, but may not silently make that change in its own branch/worktree.

## Known high-conflict files and surfaces

- `src/http/router/mobile/admin.rs`: all admin route registration; route additions/removals collide across catalog, queue, capacity, materials, and training work.
- `src/http/handlers/admin/mod.rs`, `src/http/handlers/admin/system.rs`, and `src/http/handlers/admin/production_maps.rs`: handler exports, shared request/response types, and dispatch glue.
- `src/core/production_map/{mod.rs,service.rs,store_port.rs,types.rs}` and `src/core/production_map/types/*.rs`: shared domain contracts used by nearly every production subsystem.
- `src/db/{mod.rs,postgres.rs,postgres_production_map.rs}`: persistence dispatch and shared transaction/query boundaries.
- `src/app/{app.rs,builders.rs}`: service/store construction and dependency wiring.
- `migrations/postgres/0001_mini_erp_foundation.sql` and later apparatus migrations: shared schema and append-only ordering.
- `src/http/admin_route_tests.rs`, `src/db/tests.rs`, and shared test fixtures: test registration and fixtures are common merge points.
- `mini_apparatus` identity is cross-cutting: catalog IDs/names are referenced by worker assignment, map nodes, queues, scheduling FKs, transfers, materials, and training. Any identity-contract proposal must be reviewed by all affected owners.

## Rules for every future worker

1. Every future worker is one of many parallel workers. Assume other workers are changing adjacent subsystems concurrently.
2. Edit only files inside the assigned ownership row. Do not edit shared/core single-writer files directly.
3. Do not invent alternate route paths, payload shapes, IDs, table names, state values, capability names, or other contracts. Reuse the baseline contracts and registered routes.
4. Surface cross-scope requirements to the integrator before changing another owner’s files. Include the requested file, contract change, affected owners, and compatibility impact.
5. Do not refactor unrelated business logic, alter migration history, or add a second implementation of an existing apparatus concept.
6. Keep changes isolated and report exact files, tests, and any unresolved integration requirement.
