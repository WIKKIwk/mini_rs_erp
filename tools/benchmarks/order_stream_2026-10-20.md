# Order stream: measured local before/after

Date: 2026-10-20. These are real release-server, HTTP/WebSocket and PostgreSQL measurements. They are not a production capacity or phone battery claim.

## Revisions and dataset

| Item | Value |
|---|---|
| Backend before | `6987d99501eaf6010acfc2cda705a66380dbfac8` |
| Backend after | `c50e40c0d3897b4eaff759a35aa29c9bcd21480b` |
| Before binary SHA-256 | `85f14bfd2b8c3dc884d868868642641c4c0fc2ccc4a063413ff9bb00bdd36d7b` |
| After binary SHA-256 | `a57c6ff988cb37767571f9b266cacb02438133db19c0d97b7b016362f0cfdce1` |
| Input dump SHA-256 | `d8702adb9a38f366ec53c11f0b463a0c1417afeea6083aafc8b21524b46747db` |
| Source maps / workers | 508 / 40 |
| Host | macOS-27.2-arm64-arm-64bit; 10 logical CPUs; 16 GiB RAM |
| Database / connection pool | Local PostgreSQL 18.4 / 16 per server |

Both versions use fresh clones of the same dump. Original business rows and original credentials were not changed. Temporary clone credentials and two clone-only worker apparatus assignments allow three real worker principals to share one list; eight other apparatus scopes remain separate.

## Findings addressed during measurement

An intermediate candidate exposed a PostgreSQL notification regression: statement triggers fired for periodic reconciliation statements that changed zero rows, invalidating stream caches unnecessarily. The final migration uses row changes, suppresses unchanged row updates, and handles TRUNCATE separately. An exact committed event ID also deduplicates the local and PostgreSQL publication of the same preflight transaction. The final primary run keeps periodic reconciliation enabled so this background interaction remains part of the measurement.

An intermediate candidate also duplicated local and PostgreSQL work after real sequence moves. Sequence transactions now carry an exact committed identity for deduplication, including listener-first delivery. Unrelated external invalidations and updates on another server remain observable. The move and inverse are measured separately below.

Shared prepared projections preserve principal-specific controls. Administrator windows apply sparse row/property updates and metadata checkpoints; notifications carry only notification data. Recent cursors recover through bounded history with authoritative HTTP fallback.

## Status/control update fanout

Five hold/release cycles = ten actual mutation events per scenario. Latency is the time until all clients apply a substantive event, starting before the mutation HTTP request. Bootstrap and HTTP correctness oracles are outside these phases.

| Clients / principals | Before CPU ms | After CPU ms | CPU change | Before p95 ms | After p95 ms | Before WS bytes | After WS bytes |
|---|---:|---:|---:|---:|---:|---:|---:|
| 1 / 1 | 6572.6 | 6446.2 | -1.9% | 452.0 | 657.8 | 82496 | 82498 |
| 8 / 1 | 6746.6 | 6435.9 | -4.6% | 670.6 | 679.8 | 660120 | 660120 |
| 32 / 1 | 6837.0 | 6468.5 | -5.4% | 613.3 | 645.1 | 2640480 | 2640480 |
| 32 / 3 | 15667.4 | 14738.3 | -5.9% | 699.3 | 938.9 | 2778640 | 2778640 |

Single-client median latency: 407.9 → 417.5 ms; mean: 405.2 → 460.1 ms. With ten samples, the reported nearest-rank p95 is the maximum sample. The full samples are retained in the JSON; neither average nor tail latency is inferred from CPU savings.

## Administrator loaded windows

Ten status changes per window. Bytes combine WebSocket frames and required gzip HTTP reload response bodies; HTTP headers are excluded.

| Window | Before reload GETs | After reload GETs | Before bytes | After bytes | Before CPU ms | After CPU ms | Before p95 ms | After p95 ms |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| admin_sequence_40 | 20 | 0 | 174465 | 67770 | 6407.9 | 6355.6 | 752.4 | 665.1 |
| admin_orders_60 | 30 | 0 | 369472 | 39155 | 10352.5 | 10276.5 | 1217.8 | 1040.3 |

## Actual order move and inverse

Three worker principals and a 40-row administrator window receive each version-checked mutation. These single-event phases include required page reloads and exclude subsequent exact HTTP oracle checks.

| Action | Before CPU ms | After CPU ms | Before all-view ms | After all-view ms | Before reload GETs | After reload GETs |
|---|---:|---:|---:|---:|---:|---:|
| Move | 1395.5 | 1510.0 | 822.6 | 499.6 | 2 | 0 |
| Inverse | 1420.6 | 1486.9 | 786.6 | 496.2 | 2 | 0 |

Move server CPU changed by +8.2% and inverse-move CPU by +4.7%. All-view delivery time changed by -39.3% and -36.9% respectively.

## Controlled latency repeat

A separate matched repeat uses fresh clones, the same binaries, 20 hold/release cycles (40 events) per scenario and no SQL logging. This repeat disables periodic reconciliation after its initial pass, to separate timer alignment from stream latency. The primary default-30-second results and their slower tail samples above are retained unchanged. Held/released permissions and final stream state are still checked against authorized HTTP.

| Clients / principals | Before CPU ms | After CPU ms | Before median ms | After median ms | Before mean ms | After mean ms | Before p95 ms | After p95 ms |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 / 1 | 25895.8 | 26153.1 | 394.3 | 406.1 | 394.5 | 404.4 | 440.0 | 456.4 |
| 32 / 3 | 59776.5 | 59766.8 | 654.9 | 642.3 | 647.4 | 628.7 | 781.1 | 716.3 |

Repeat peak server RSS: 867.8 → 849.5 MiB. Recorded recoverable stream errors: 0 → 0. These are additional observations, not a replacement for the primary run.

## Other measured behavior

| Scenario | Before | After |
|---|---:|---:|
| Notification bootstrap JSON bytes | 5332449 | 120 |
| Notification bootstrap full order maps | 508 | 0 |
| Reconnect snapshot GETs | 1 | 0 |
| Reconnect replayed events | 0 | 2 |
| Recoverable stream errors / required snapshot GETs | 0 / 0 | 0 / 0 |
| Other-apparatus data events (two changes) | 16 | 8 |
| Other-apparatus WS frame bytes (two changes) | 13826 | 12706 |
| Idle HTTP GETs / data events (30 s) | 0 / 0 | 0 / 0 |
| Idle WS received / sent bytes (30 s) | 128 / 384 | 64 / 192 |
| Idle server CPU, ms | 154.0 | 148.6 |
| Other-apparatus server CPU, ms | 10155.7 | 9574.2 |
| Peak observed server RSS across timed phases, MiB | 1434.4 | 1443.1 |

Overall primary-run peak RSS changed by +0.6%.

## Separate SQL diagnostic

One hold/release pair per scenario with SQLx query logging enabled. Counts are observed SQLx statement events, including incidental server maintenance in that interval. Diagnostic timings are intentionally not used above. Periodic order reconciliation stops after its initial run in this SQL diagnostic, as in the controlled repeat; primary timings retain its default 30-second interval. PostgreSQL transaction/tuple/buffer counters remain available in the JSON and are not labeled as SQL counts.

| Scenario | Before SQLx events | After SQLx events |
|---|---:|---:|
| same_principal_1 | 134 | 136 |
| same_principal_8 | 152 | 139 |
| same_principal_32 | 203 | 143 |
| shared_apparatus_workers_32 | 263 | 213 |
| other_apparatus_change | 464 | 469 |
| admin_sequence_40 | 139 | 137 |

## Correctness

Both runs passed exact authorized HTTP comparisons for all recorded scenarios, including held and released actor-specific controls, the other-apparatus views while the hold is active, current ordering, order statuses, stage states, maps, pagination metadata and cursors. A real version-checked reorder and its inverse restored the original sequence with no lost rows. Reconnect state matched fresh HTTP.

Two initial baseline attempts aborted during active-hold checks: first an untyped stream error, then a reproduced `store_failed` error. The underlying storage error was not exposed by the baseline logs. Those partial attempts are excluded from the measured pair. The published complete pair permits the same explicit authorized HTTP/reconnect recovery in both versions and records each error and its extra GETs. Successful deltas are still compared exactly; mismatches never trigger a hidden oracle repair.

- Before: 22 recorded exact checks.
- After: 22 recorded exact checks.

Separate focused Rust checks also passed: page-live unit checks 6/6, actual WebSocket `worker_queue_window` integration checks 7/7, state feed/hub/replay checks 21/21, sequence checks 4/4, realtime checks 3/3, isolated PostgreSQL reorder checks 4/4 and listener/migration check 1/1. These groups overlap and are not summed.

The broader Rust GET test `production_map_sequence_returns_backend_visible_order_ids` fails at `src/http/admin_route_tests/production_map_basic.rs:256` on both the immutable baseline and candidate with the same empty-apparatus assertion. That existing failure was reproduced independently; these results do not claim a completely passing full backend suite.

## Two independent server processes

A separate correctness run starts two OS processes against one disposable PostgreSQL clone. Process B logs in independently as the acting worker, another worker and an administrator, then subscribes to worker state and a 40-row administrator window. Real hold, release, reorder and inverse reorder requests go through process A. All four actions converge to the exact authorized HTTP content on both processes; original ordering is restored. Static role/config files are copied explicitly for this test.

Two-process tested binary SHA-256: `a57c6ff988cb37767571f9b266cacb02438133db19c0d97b7b016362f0cfdce1`.

## Limits and reproduction

See [the harness methodology](order_stream_README.md). This is one local paired run with 32 sockets maximum, not a capacity forecast. Primary fanout/admin p95 uses ten samples; the controlled repeat uses forty. Other role/capability combinations and broader deployment stress need their own tests beyond the admin/aparatchi scopes here. CPU is the application server process only; database and load-client CPU are excluded. No WAN, TLS, battery, physical mobile rendering or reconnect storm is measured.

Replay is bounded per feed: state history retains at most 64 entries / 256 KiB for 90 seconds; page history retains at most 8 window versions / 512 KiB for 90 seconds. Missing or expired history falls back to authoritative HTTP. Role/config files are not automatically shared between deployment nodes.

## Reference methods

- [Centrifuge prepared fanout payloads](https://github.com/centrifugal/centrifuge/blob/7ac037196c955bf7140f2a48f8d35fd4de4b77d9/hub.go#L1168): prepare reusable wire data once for compatible subscribers.
- [Centrifugo bounded stream recovery](https://centrifugal.dev/docs/server/history_and_recovery) and [Dart subscriber recovery](https://github.com/centrifugal/centrifuge-dart/blob/380cb2e4cb4fc43960daa8ffda6fea621d0efa8b/lib/src/subscription.dart): prove epoch/cursor continuity or reload.
- [PowerSync watched queries](https://github.com/powersync-ja/powersync.dart/blob/1961d8ac7652fd3789d50a4c28a0e35034383ba2/packages/powersync/lib/src/database/powersync_database.dart#L611): react to relevant changes and coalesce work.
- [Flutter build cost guidance](https://docs.flutter.dev/perf/best-practices#control-build-cost): bound rebuild work to changing content.

These are reference methods adapted to the existing system. No external realtime platform or replacement project was introduced.

## Local test cleanup

All benchmark server processes were stopped. Private dumps, binaries, metadata and logs remain in `/private/tmp/accord-order-stream-20261020` with protected filesystem permissions for reproducibility. The disposable `bench_order_stream_before_*` and `bench_order_stream_after_*` databases remain in local PostgreSQL.

PostgreSQL after testing: `restored_stopped`.
