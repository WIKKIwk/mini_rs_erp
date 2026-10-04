# Stage topology projection verification

## Shipped scope

`stage_work_statuses` resolves each eligible session's immutable graph occurrence
once per call, then retains the existing per-operation reduction. Source order,
raw order/apparatus identity, ambiguous legacy-occurrence rejection, alternative
resolution, and the strict `>` latest-session tie rule are unchanged.

No SQL, transaction, persistence, route, or payload contract changed.
`queue/service.rs`, `Cargo.toml`, and `Cargo.lock` are byte-identical to baseline
`f4286c7620a0372b64a23c64f293091bdd3cf9b7`.

The proposed raw-order opening-WIP index was **excluded**. Its small-selection /
few-import break-even was not established, so this change ships only the session
topology hoist. The queue/opening-WIP fixture still verifies the resulting full
queue controls against the original library.

The hoist retains one resolved node ID and session reference per accepted session
until the call returns. This is an **O(history) temporary-memory tradeoff**, not a
claim of lower peak RAM. It is per evaluated map, not a persistent cache.

## Correctness evidence

- 768 old/new evaluator comparisons over 0/1/3/6 operations, 0/1/20/100 sessions,
  repeated apparatus, alternatives, blank/whitespace/raw IDs, missing/deleted
  occurrence metadata, wrong-order sessions, all session states, malformed or
  missing reports, outstanding/available inputs, and ordered lifecycle events
- Explicit duplicate session IDs/timestamps with conflicting payloads prove exact
  ties retain the first source session; report-sequence ties are also compared
- 17 actual memory-store lifecycle scenarios: 12 completion/history/retry cases
  and 5 active, paused, frozen, detached, or completed-but-unreported cases that
  must remain in progress; session history is preserved
- 27 full serialized queue-control/read-call fingerprints captured from the
  original compiled library, including 0/1/285 maps, 0/100/1,000/10,000 opening
  records, raw/blank/duplicate IDs, and 0/1 selected-map apparatus-scoped reads
- Four existing active-paddon snapshot-cache regression tests remain green

`tests/support/stage_work_reference.rs` freezes the original evaluator and
lifecycle function. It was checked against the baseline source, allowing only
visibility/import adjustments and formatting. The actual side calls the real
library through `verification_stage_work`; that adapter and the resolver counter
exist only with the existing `verification` feature.

`tests/support/queue_projection_baseline.json` fingerprints every serialized
control plus read counts. Requested order IDs and returned record rows have
separate counters. All six measured baseline/candidate processes also reproduced
all 27 fingerprints.

## Commands and build profile

Run with the repository's Rust toolchain and locked dependencies:

```sh
export CARGO_BUILD_JOBS=1
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
cargo test --locked --features verification \
  --test stage_topology_projection --test queue_opening_projection \
  --test active_paddon_snapshot_cache -- --nocapture
cargo clippy --locked --features verification \
  --test stage_topology_projection --test queue_opening_projection
cargo check --locked --lib
```

The final correctness run passed 10 tests; two measurement tests are deliberately
ignored in ordinary runs. Targeted clippy exited successfully with no warnings in
the new test targets; 455 existing library warnings remain. The ordinary library
check, without `verification`, passed with two existing unused-import warnings,
confirming the shipping build excludes the bridge/counter. New-file rustfmt and
`git diff --check` also passed. The measurement entry points are:

```sh
cargo test --locked --features verification --test stage_topology_projection \
  -- --ignored --exact repeated_compiled_projection_workload --nocapture
cargo test --locked --features verification --test queue_opening_projection \
  -- --ignored --exact capture_projection_baseline --nocapture
```

The second entry point prints fingerprints and service elapsed times; it does not
rewrite the stored oracle. The original baseline used the exact two evaluator
files from `f4286c7`, with the same verification adapter and fixtures. Compilation
time was excluded by executing the saved test binaries for the paired process
measurements. Baseline and candidate execution order alternated across three runs.

## Work and timing observations

Actual resolver calls, counted in the compiled library, changed as follows:

| Operations | Sessions | Original | Hoisted |
| --- | --- | --- | --- |
| 0 | 100 | 0 | 0 |
| 1 | 100 | 100 | 100 |
| 3 | 100 | 300 | 100 |
| 6 | 100 | 600 | 100 |

These counts are the primary work-reduction evidence. Tests assert the full
0/1/3/6-operation × 0/1/20/100-history matrix.

The unoptimized test build's direct evaluator workload used 285 calls per sample,
one warm-up and six measured rounds with alternating old/new order. Reported upper
medians included 3 operations/100 sessions: 2.653 s → 0.900 s; 6 operations/100
sessions: 14.677 s → 2.546 s. These are in-process **elapsed** measurements.

Across the same 27 full queue fixtures, three paired compiled-process medians were:

| Measurement | Original | Hoisted |
| --- | --- | --- |
| Process user CPU, including fixture setup/assertions | 5.670 s | 4.498 s |
| Process elapsed | 5.811 s | 4.610 s |
| Process max RSS | 137,104 KiB | 137,152 KiB |
| Service elapsed: 20 maps, 3 operations, 100 sessions/map | 196.9 ms | 75.7 ms |
| Service elapsed: 20 maps, 6 operations, 100 sessions/map | 1,049.4 ms | 229.1 ms |

No-history timings were noisy rather than uniformly faster. For example, the
285-map/10,000-opening-record service median was 118.0 ms → 141.5 ms. Do not treat
these few synthetic samples as a universal latency improvement or a production
speed estimate. The counter is disabled during timing, but the verification TLS
check still executes on every resolver call and therefore biases timing in favor
of fewer calls. Max-RSS measurements cover the entire fixture process and do not
isolate the small per-map temporary vector.

No PostgreSQL integration or production-database benchmark was run: the available
server's Unix-socket access was denied. This evidence establishes pure evaluator
and memory-store parity, not a blanket production-data guarantee.
