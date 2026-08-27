# Production Order Lifecycle Design

## Goal

Persist one authoritative lifecycle state for every production order so that an apparatus completing its own operation cannot make the whole order appear complete or disappear from raw-material assignment.

## Domain boundaries

- `mini_production_maps` is the production-order header aggregate.
- `mini_queue_states` remains the operation/apparatus execution state.
- `mini_order_control_states` remains the hold/freeze control state.
- `completed_with_issue` is a completion outcome, not a lifecycle state.
- Detailed queue events, sessions, and WIP records remain audit and reconciliation evidence; normal list reads must not replay all of them to discover header lifecycle.

## Lifecycle

The persisted lifecycle values are:

- `released`: the production map exists and is available to production.
- `in_progress`: at least one required physical operation has started, paused, frozen, or completed, but the production order is not fully complete.
- `production_completed`: every required physical operation in the current production map is complete.
- `closed`: production and downstream business closure are complete. This value is reserved for an explicit closure transition; apparatus completion must never set it.
- `cancelled`: the production order was explicitly cancelled. No implicit cancellation is introduced by this change.

There is no persisted `created` state because the current map-save flow immediately publishes a map to apparatus queues. The existing behavior therefore corresponds to `released`.

Allowed automatic transitions in this change:

```text
released -> in_progress -> production_completed
released -> production_completed
```

`production_completed`, `closed`, and `cancelled` are terminal for material-assignment candidate filtering. Reopen, close, and cancel commands require explicit domain operations and are not inferred from unrelated writes.

## Persistence

Migration `0077` adds lifecycle columns to `mini_production_maps`, an indexed active-order predicate, and an append-only transition table. Existing maps are backfilled from their own required physical apparatus and queue states once during migration; requests do not repeatedly run the backfill query.

Lifecycle changes are written in the same database transaction as the queue action or approved completion request that caused them. A transition event is inserted only when the stored lifecycle value changes. Repeated idempotent writes therefore do not create duplicate lifecycle events.

## Completion rule

The completion predicate reuses the existing production-map rule for required physical apparatus, including selected alternatives and excluding virtual tasks. For an order to become `production_completed`, every required apparatus must have a `completed` queue state for that order. Missing states are not complete.

Any non-pending queue state means `in_progress` unless the full completion predicate is true. A map with only pending or missing queue states remains `released`.

## Read paths

- Raw-material assignment lists load maps and persisted lifecycle statuses once, then keep only non-terminal production orders. They must not call `order_status_detail` once per map.
- Fully completed order history starts from persisted `production_completed` or `closed` order IDs, then loads detailed audit information only for those candidates.
- `ProductionOrderStatusDetail` receives an additive `lifecycle_status` field. Existing `order_status`, `work_status`, `flow_status`, and `stock_status` remain temporarily backward-compatible display projections.
- The legacy display projection must not return whole-order `completed` merely because one queue state is completed.

## Compatibility and failure behavior

- Existing mobile clients continue to deserialize the old fields; the new field is additive.
- Invalid or unknown stored lifecycle text is a store error, never silently converted into a false terminal state.
- A queue transaction fails as a unit if lifecycle persistence fails.
- Existing freeze, WIP, raw-material, transfer, and schedule behavior remains unchanged.

## Verification

Tests must prove:

1. Completing only the first operation leaves lifecycle `in_progress` and keeps the order in material-assignment candidates.
2. Completing every required physical operation writes `production_completed`.
3. A repeated/idempotent write does not create a second transition.
4. The Postgres migration backfills released, in-progress, and fully completed maps correctly.
5. Existing completed-order audit and WIP flow tests remain green.

