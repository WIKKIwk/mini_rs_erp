# Snapshot cache reuse: measured comparison

Date: 2026-10-20. This compares the **previous optimized release** `c50e40c0` (reported at `98147693`) with the new cache optimization. It does not compare against original `main`.

Both sides use modern worker/page/notification protocols, `include_control=true`, a 16-connection pool and the default 30-second reconciler during timing. No diagnostic tracing is enabled.

## Provenance

| Item | Value |
|---|---|
| Before source | `c50e40c0d3897b4eaff759a35aa29c9bcd21480b` |
| After source | `b5400bdd9ce83b180e692623a5403677f193b707` |
| Before binary SHA-256 | `a57c6ff988cb37767571f9b266cacb02438133db19c0d97b7b016362f0cfdce1` |
| After binary SHA-256 | `d06a174a64c84f6f5758dbd29c2e9551a0736356c41ca1bd43a8f24d912c93db` |
| Input dump SHA-256 | `d8702adb9a38f366ec53c11f0b463a0c1417afeea6083aafc8b21524b46747db` |
| Harness SHA-256 | `23a3fed87a229b703353de7f89ba7ae000fe744980166d4a8b43aed9514755f0` |
| Host | macOS-27.2-arm64-arm-64bit; 10 logical CPUs; 16 GiB RAM |

Every run restores the same private dump. Actor, apparatus, order and all participating principal fingerprints match across primary, reverse and SQL runs. Original business records are unchanged. Timing order is baseline → candidate for the full primary run, then candidate → baseline for the independent status repeat. Each status condition has 20 hold/release cycles (40 events). Results are reported separately.

## Regression found before the final candidate

The first cache-only candidate `460f1d54` reduced CPU but returned null controls in **28/40** mixed-worker actions, versus **2/40** on the baseline. These results are retained as an intermediate failure, not the final comparison. The existing 250 ms optional control deadline was too short for joining an in-flight shared snapshot. The final candidate allows a bounded 1-second shared cache wait while preserving authorization and revision checks. This cooperative async timeout cannot preempt synchronous CPU work and is not a hard ceiling for total POST latency. Final and reverse-repeat null counts and POST latency are shown below; the deadline tradeoff is not hidden.

## End-to-end status latency

Latency starts before receiver submission and POST, ending when every client applies the exact generated hold or its matching release. It includes Python scheduling/reducer work. Raw socket completion, decode and reducer timings are retained in the JSON. Nearest-rank p95 of 40 samples is the third-largest sample; these are local measurements, not a production service-level guarantee.

| Run / sockets / principals | Before median ms | After median ms | Before p95 ms | After p95 ms | p95 change | Before max ms | After max ms |
|---|---:|---:|---:|---:|---:|---:|---:|
| Primary / 1 / 1 | 415.68 | 368.81 | 541.29 | 411.61 | -24.0% | 653.38 | 416.54 |
| Primary / 32 / 1 | 584.96 | 530.18 | 724.06 | 569.01 | -21.4% | 847.09 | 573.29 |
| Primary / 32 / 3 | 621.70 | 609.73 | 751.96 | 653.41 | -13.1% | 870.76 | 736.35 |
| Reverse repeat / 1 / 1 | 404.86 | 362.47 | 456.77 | 407.61 | -10.8% | 629.45 | 408.92 |
| Reverse repeat / 32 / 3 | 629.82 | 616.63 | 665.98 | 728.95 | +9.5% | 771.24 | 753.07 |

**Mixed-worker p95 is not consistently improved:** the primary run changes by -13.1%, while the reverse repeat changes by +9.5%. CPU and control-return improvements repeat, but these measurements do not show that the remaining latency tail is solved.

The reverse mixed-worker run has 2 → 4 events above 700 ms. Its aggregate p95 uses the third-largest sample. A late principal group occurs in both versions; these user-space receive timestamps include client scheduling and do not prove the cause of the remaining delay.

## Server cost and acknowledgement controls

CPU is process CPU over the complete phase; memory is sampled RSS peak. Control latency below is the complete HTTP response arrival time, including the optional control when present; it is not an isolated server control-build timer. The harness records null controls but does not simulate Flutter's follow-up scoped bootstrap/section GETs after a null control; those costs are excluded from these POST/stream phases.

| Run / sockets / principals | CPU ms before → after | CPU change | Peak RSS MiB before → after | POST p95 ms before → after | Control null before → after | Stream recovery GETs before → after |
|---|---:|---:|---:|---:|---:|---:|
| Primary / 1 / 1 | 25864.3 → 13043.2 | -49.6% | 650.6 → 423.1 | 449.05 → 406.09 | 0 → 0 | 0 → 0 |
| Primary / 32 / 1 | 26276.3 → 13123.2 | -50.1% | 657.1 → 493.2 | 483.18 → 405.31 | 0 → 0 | 0 → 0 |
| Primary / 32 / 3 | 58413.2 → 42889.9 | -26.6% | 843.9 → 728.4 | 540.90 → 575.47 | 2 → 0 | 0 → 0 |
| Reverse repeat / 1 / 1 | 26563.2 → 13057.4 | -50.8% | 651.6 → 474.1 | 449.49 → 402.11 | 0 → 0 | 0 → 0 |
| Reverse repeat / 32 / 3 | 59096.2 → 43231.6 | -26.8% | 850.5 → 735.4 | 628.28 → 612.67 | 2 → 0 | 0 → 0 |

Overall primary peak RSS: 1365.9 → 1139.1 MiB (-16.6%). Each status condition makes 40 POSTs; the non-null control count is 40 minus the null count above.

The primary mixed-worker POST p95 rises by 34.57 ms (+6.4%) while null controls fall from 2/40 to 0/40. Waiting for the shared control can keep the initiating action pending longer; avoided Flutter recovery GETs are not included in these measured costs.

## Separate SQL-count diagnostic

One hold/release cycle per status condition. Query logging and period0 are used only here. Counts include SQLx query events during each phase; their timing is not mixed with primary latency.

| Phase | Before statements | After statements | Change |
|---|---:|---:|---:|
| same_principal_1 | 136 | 106 | -22.1% |
| same_principal_32 | 140 | 110 | -21.4% |
| shared_apparatus_workers_32 | 212 | 174 | -17.9% |
| other_apparatus_change | 466 | 375 | -19.5% |
| admin_sequence_40 | 137 | 103 | -24.8% |

## Retained behavior checks

| Scenario | Before | After |
|---|---:|---:|
| admin_sequence_40: reload GETs / p95 ms | 0 / 465.87 | 0 / 414.15 |
| admin_orders_60: reload GETs / p95 ms | 0 / 1086.59 | 0 / 910.74 |
| Reorder: delivery ms / reload GETs | 617.85 / 0 | 458.82 / 0 |
| Reorder inverse: delivery ms / reload GETs | 673.84 / 0 | 472.01 / 0 |

**Before:** 21 exact REST checks; 0 stream errors; idle GETs/data events 0/0; reconnect snapshot GETs 0; notification bytes 121.

**After:** 21 exact REST checks; 0 stream errors; idle GETs/data events 0/0; reconnect snapshot GETs 0; notification bytes 121.

| Other primary phase | CPU ms before → after | CPU change | Peak RSS MiB before → after |
|---|---:|---:|---:|
| idle_worker_streams | 146.0 → 149.6 | +2.5% | 272.3 → 238.1 |
| other_apparatus_change | 9913.0 → 6461.7 | -34.8% | 1149.8 → 913.5 |
| admin_sequence_40 | 6669.7 → 3316.3 | -50.3% | 1291.1 → 967.1 |
| admin_orders_60 | 10494.5 → 10454.3 | -0.4% | 1365.9 → 1139.1 |
| reorder_0 | 1439.5 → 1082.8 | -24.8% | 1250.8 → 1106.3 |
| reorder_1 | 1444.0 → 1069.7 | -25.9% | 1269.5 → 1107.3 |
| notifications_bootstrap | 0.2 → 0.3 | +10.4% | 1269.5 → 980.5 |

Held and released controls are checked for each shared worker and other-apparatus scope. Administrator loaded windows and sequence move/inverse match fresh HTTP, including pagination. A separate candidate two-OS-process check verifies PostgreSQL invalidation and exact authorized content across hold, release, move and inverse; it is not a distributed capacity test.

## Focused verification

25 focused backend checks passed: 18 selected library checks (11 snapshot reuse, 4 existing regressions and 3 actual HTTP shared-wait cases), plus 7 worker_queue_window WebSocket integration checks. The HTTP cases cover successful shared waiting, timeout fallback and authorization revocation before timeout.

52 existing focused Flutter checks passed: 27 widget acknowledgement/live/recovery/synchronization cases and 25 queue cursor/state-delta contracts. Mobile source was unchanged for this cache optimization.

These are focused checks, not a full-suite pass claim.

Commands and retained logs:

- `cargo test --lib -- queue_action_control_wait core::production_map::service::worker_snapshot::tests sequence_keeps_frozen_colour_detail_controls_in_full_and_scoped_reads worker_completed_orders_are_actor_scoped_and_latest_first closed_orders_return_only_fully_completed_maps_with_action_logs same_revision_assignment_roundtrip_cannot_reuse_or_replay_other_scope`
- `/tmp/accord-control-shared-wait-regression-tests.log (18 passed)`
- `cargo test --features verification --test worker_queue_window`
- `/tmp/accord-control-shared-wait-window-tests.log (7 passed)`
- `/Users/wikki/Desktop/Accord_project/.tools/flutter-sdk/flutter/bin/flutter test --no-pub test/admin_production_map_test_screen_test.dart --name 'worker colour|worker sync optimization:' --reporter expanded`
- `/tmp/accord-mobile-cache-review-widgets-20261020.log (27 passed)`
- `/Users/wikki/Desktop/Accord_project/.tools/flutter-sdk/flutter/bin/flutter test --no-pub test/admin_queue_action_cursor_test.dart test/admin_production_map_state_delta_test.dart --reporter expanded`
- `/tmp/accord-mobile-cache-review-contracts-20261020.log (25 passed)`

## Limits and cleanup

Loopback, one local PostgreSQL dataset and at most 32 sockets. No WAN, mobile rendering, battery, TLS or high-scale capacity claim. Both raw runs and outliers are retained. SQL logging, diagnostic tracing and period0 results are not substituted for default-configuration timing.

Private artifacts: `/private/tmp/accord-order-stream-cache-20261020`. Cleanup evidence is included in the JSON. Benchmark servers stopped: **True**. PostgreSQL cleanup state: **restored_stopped**. Independent PostgreSQL sessions before cleanup: **0**.
