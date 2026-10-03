# Worker low bandwidth optimization

## Goal and correctness boundary

Make colour trial buttons and worker state exchange fast on weak ERP servers,
tablets and unreliable networks. PostgreSQL commits and existing server business
checks remain authoritative. No optimistic completion, automatic mutation replay,
production deployment, or production business writes are part of this work.

## Baseline

Read-only worker measurement before changes: the first WebSocket snapshot was
2,646,728 uncompressed bytes, including 285 maps and 14 apparatus control scopes.
The existing HTTP queue response compressed to about 83 KB. These are baseline
measurements, not measured post-change performance.

## Completed stages

1. Checkpoint commits: ERP `02c0ee9`, mobile `e5befea`.
2. Committed colour response carries optional authoritative current controls.
   Mobile releases the action immediately after its response and does not await
   an extra control GET. Missing/invalid controls disable actions and trigger a
   read-only refresh. ERP `f1f0ed4`, mobile `8c9e539e`.
3. Opt-in `state_delta_v1`: `state_ready`, revision-bound `state_delta` and
   `state_resync`. It covers maps, queue order, barrier fingerprints, states,
   action permissions, order controls/statuses and worker notifications together.
   Removed entries are explicit. Unchanged maps are neither serialized nor parsed.
   Workers bootstrap over compressed HTTP; WS never duplicates that full frame.
   A gap, broadcast lag or patch over 64 KiB triggers HTTP reconciliation.
   Legacy clients retain their previous protocol. Client retries use jitter and a
   30 second cap; socket outages do not start a second full-read retry loop.

This stage deliberately uses the existing epoch/global snapshot revision. It does
not claim durable colour-event replay: a disconnected client or restarted server
reconciles with authoritative HTTP state. Existing durable sequence replay remains
available to the legacy sequence protocol.

## Next stages

- Project bootstrap/reconciliation and live views to the worker's authorized
  apparatus orders, retaining cross-stage state needed by those orders.
- Decode and construct large HTTP snapshot models away from the UI isolate.
- Reduce repeated server compilation/control evaluation without changing business
  predicates or removing queue/database locks.

## Verification

Stage 1: `cargo check --locked` and focused Dart analysis passed.
Stage 3: four Rust patch tests and fifteen Flutter delta/regression tests passed;
focused Dart analysis passed. A broader print-preflight test filter also selected
a PostgreSQL integration test; it failed at local database authentication before
its database work. No production database was used or deployed.

Bandwidth targets and real weak-network/tablet latency still require runtime
measurement after these changes are deployed to an authorized test environment.
