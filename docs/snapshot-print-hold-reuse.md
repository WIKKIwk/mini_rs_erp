# Snapshot-local print-preflight hold reuse

The canonical snapshot builder loads print-preflight holds for sequence versions.
It now moves that same owned vector into queue-control projection instead of
performing a second identical store read. The vector is local to one attempted
rebuild; it is neither persisted nor cached independently.

## Preserved behavior

- Sequence versions still use raw apparatus equality and `is_live_at(0)`.
- Queue controls still filter with `is_live_at(now)`, trim apparatus keys, and
  resolve duplicate apparatus keys by the last live hold in source order.
- `is_live_at` remains status-only, including live holds with past expiry values.
- Standalone and apparatus-scoped controls still fetch fresh holds concurrently
  with their other inputs, preserving the existing error precedence.
- Revision invalidation, discarded-build retry, cache sharing, mutations,
  transactions, HTTP responses, and live notifications are unchanged.
- A failed first hold read still fails the rebuild and does not populate the
  cache. The deliberately removed second read can no longer independently fail.

## Evidence

Tests link the real production library with the `verification` feature; they do
not reimplement its snapshot evaluator. Baseline fingerprints were captured
before the change from local `ba2e0ac4553d21d479e8c1b26251d6513f151ba6`
(the same source tree as published `0f5e3c1eb86ff42e4b17ee99f143213009a5c708`).

- All 12 complete serialized snapshot fingerprints match: zero, one and 285
  maps; all six hold statuses; mixed statuses; duplicate apparatus keys; raw,
  whitespace, blank and duplicate IDs. Every live fixture has a past expiry.
- Each cold rebuild reads holds **once instead of twice**, including zero maps.
- Cache hits perform no hold reads; concurrent readers share one rebuild.
- Invalidation after hold capture discards the result and loads fresh holds on
  retry. Failed reads are retried, with existing errors and precedence preserved.
- Actual HTTP print-preflight mutation + `include_control` ACK + next live
  snapshot agree. A failed post-commit presentation read keeps `ok=true` and
  `control_state=null`; recovery and repeated mutation remain idempotent.

Run the focused regression set:

```sh
cargo test --locked --features verification \
  --test snapshot_print_hold_reuse --test snapshot_print_hold_ack \
  --test active_paddon_snapshot_cache --test queue_opening_projection
```

The measured benefit is one fewer StorePort hold read per rebuild. Source
inspection shows this removes one PostgreSQL active-holds SELECT and its row
decoding; SQL runtime was not measured.
No endpoint latency, total worker latency, RAM percentage, PostgreSQL integration,
production-data safety proof, or full-suite pass is claimed by these memory-store
tests. No SQL, schema, mobile, lockfile or transaction code changed.
