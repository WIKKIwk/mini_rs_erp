# Absolute sequence positions and live delta streaming

Measured on 2026-10-23 (Asia/Tashkent). The synthetic benchmark uses the real
Axum HTTP and WebSocket routes with an in-memory production-map store and a
loopback listener on an ephemeral port. It does not load `.env`, connect to
PostgreSQL, use production sessions, or mutate business data. Other fixture
stores use the existing test helpers' temporary paths.

## Reproduce

```sh
cargo test sequence_position_stream_benchmark --lib -- --ignored --nocapture --test-threads=1
```

The ignored test is
`src/http/admin_route_tests/sequence_position_stream_benchmark.rs`. It prints
JSON after the common sequential workload and after all scenarios pass. A
panic or missing final JSON means the run did not pass.

For comparison, the same benchmark workload was copied into a temporary,
tracked-source-only export of baseline commit
`57ef1e81508eacf18bed80f287615d9e21181e0e`. Run that **baseline source only**
with `ACCORD_BENCH_EXPECT_POSITIONS=0`; this disables assertions for the new
field while retaining stream continuity, payload bounds, and exact REST
comparison assertions. It must not be used to validate the new implementation.

Both test executables use the unoptimized debug profile. Six final measured
runs alternate baseline, after, after, baseline, baseline, after, without
compiler or Flutter test jobs running concurrently. The saved baseline executable predates the
addition of phase labels and the intermediate JSON print, which do not change
the fixture, mutations, timing boundaries, or assertions used for comparison.

## Workload and assertions

- 1,000 synthetic orders in a known apparatus queue; two simultaneous live
  windows load 20 unfiltered rows and one search result respectively.
- The search result starts at absolute position 4. Swapping rows 4 and 5 tests
  both 4 → 5 and 5 → 4 while the search result remains the only returned row.
- 100 writes wait for both windows on every write. All 200 expected updates
  must arrive. Every resulting page is compared exactly with fresh HTTP JSON;
  every returned absolute position is checked against the full canonical queue.
- 513 writes run in a burst while the client delays reading. An independent,
  deliberately unread subscriber proves the burst exceeds the 256-entry source
  ring. The client must apply a valid delta chain and reach the exact final
  HTTP state. This witness does not prove the socket receiver itself encountered
  a `Lagged` result.
- One intentional clean disconnect is followed by an offline queue write.
  Reconnecting with retained versions must yield a delta and ready confirmation
  without a REST reload, then correctly deliver 20 further sequential writes.
- Every applied delta requires the exact previous `base_version`, unchanged
  epoch, and increasing revision. Rank-only filtered updates must contain no
  `maps` operation and must stay below 2 KiB. Position metadata must contain
  only the loaded rows. All incoming frames are capped below 256 KiB.

Burst revision coalescing is permitted by this protocol: the server may replace
many intermediate revisions with one patch from the client's retained version
to the latest canonical state. It is reported separately from an illegal
baseline gap or lost final state. Revision-coalescing counts sum across both
windows; they are not counts of dropped network packets.

## Scope of the evidence

The benchmark's Tokio runtime has one thread. Its timings include the domain
queue write, projections, and actual loopback WebSocket delivery to both
windows; fresh REST comparisons run outside each per-write latency measurement.
This is an isolated regression and local performance comparison, not a
production PostgreSQL load test, mobile-network soak, CPU/RSS profile, or a
claim about all future traffic. Zero failures means no failures were observed
in the stated passing scenarios.


## Results

The machine-readable report is
`order_sequence_positions_2026-10-23.json`; it retains every measured round,
including the noisy second round. Backend source after both fixes is
`a5db5d3c`; the rank contract was added in `5a055b47`.

| Round | Version | Write-to-both-windows p50 (ms) | p95 (ms) | max (ms) |
| --- | --- | ---: | ---: | ---: |
| 1 | baseline | 152.424 | 158.522 | 169.688 |
| 2 | after | 175.688 | 220.531 | 427.192 |
| 3 | after | 152.699 | 155.169 | 204.979 |
| 4 | baseline | 151.565 | 153.691 | 170.810 |
| 5 | baseline | 149.951 | 152.070 | 201.953 |
| 6 | after | 151.557 | 154.168 | 180.257 |

These summaries are medians of three **per-run** values, not percentiles pooled
across all 300 samples:

| Metric | Baseline (ms) | After (ms) | Change |
| --- | ---: | ---: | ---: |
| p50 | 151.565 | 152.699 | +0.75% |
| p95 | 153.691 | 155.169 | +0.96% |
| max | 170.810 | 204.979 | +20.00% |

The p50/p95 median differences were under 1%. The first after run was noisier:
its p95 was 220.531 ms and max was 427.192 ms. The other after runs had p95
155.169 and 154.168 ms. Maxima also varied, so these samples do not establish
equal worst-case latency. No consistent increasing latency appeared: the last
after run's first/last twenty-write p50 values were 150.658/151.796 ms, and its
post-burst/reconnect p50 was 152.341 ms. No hardware-dependent gate is built
into the test.

Every final after run passed the 4 ↔ 5 absolute-rank assertions, all exact REST
comparisons, and all continuity checks. Across the three after runs:

- 1,902 queue writes and 732 received delta frames;
- zero illegal baseline gaps, final-state mismatches, or unexpected disconnects;
- three successful deliberate offline/reconnect recoveries;
- each 513-write burst converged using two delta frames, one per window;
- 1,024 intermediate window-revision steps were legally coalesced per run;
- burst catch-up took 209.649–212.408 ms, measured after writes finished;
- deliberate reconnect recovery took 140.770–147.902 ms.

Wire sizes are uncompressed WebSocket frame bytes; bootstrap sizes are JSON
bytes. The extra bytes carry the absolute positions required by the fix:

| Payload | Baseline (bytes) | After (bytes) | Added bytes |
| --- | ---: | ---: | ---: |
| 20-row bootstrap | 41,078 | 41,512 | 434 |
| One filtered row bootstrap | 3,782 | 3,825 | 43 |
| Unfiltered reorder delta | 1,925 | 2,067 | 142 |
| Filtered rank-only delta | 615 | 686 | 71 |
| Entire measured stream per run | 310,702 | 336,688 | 25,986 |

The filtered delta contains no map operations and remains below 2 KiB. The
client receives only its loaded row's position, rather than all 1,000 positions.

## Exploratory failure and independent retry regression

An initial exploratory run, before the pending-read retry change and while
other compilation/testing load was present, failed after 49.68 seconds: the
client received JSON without a `window` field. The original harness unwrapped
that field before printing the frame, so its exact server error was not
recorded. Three later diagnostic repeats passed (45.32, 36.37, 35.37 seconds);
those runs are excluded from the performance comparison because CPU load was
not controlled. The original unexplained error is not silently counted as a
passing run.

A separate deterministic regression then established an existing stream fault:
three reads invalidated by concurrent writes caused the old server to emit
`{"error":"order_page_changed"}` and close the connection. The recovery test
failed against the old implementation. This demonstrates that failure class;
it does not prove that it caused the initial exploratory benchmark failure.

The scoped fix retries exactly HTTP 409 `order_page_changed` after 100 ms,
keeps each subscriber's last sent baseline, and can recover without a later
notification. Tests cover both initial and established two-window streams and
check the recovered ranks and delta baseline. Fatal 500 errors and unrelated
409 errors retain their error/close behavior. The final focused backend checks
passed 20/20: 11 order-page route tests, six page-live unit tests, and three
persistent-admin recovery tests.

## Flutter checks

Mobile commit `40529538` passed 97 focused API/model/cache/delta tests and 63
focused widget tests. The separate reducer microbenchmark in
`accord_mobile_v2/test/admin_sequence_positions_test.dart` applied all 5,000
rank deltas with no lost revision and retained identical map objects. Its
isolated run measured 113 ms total, p50 9 µs, p95 36 µs, p99 92 µs. This is a
client reducer measurement, not network delivery latency; a parallel test run
was slower and was not used as the isolated figure.

To reproduce from `accord_mobile_v2`:

```sh
PUB_CACHE=../.tools/pub-cache ../.tools/flutter-sdk/flutter/bin/flutter test --no-pub test/admin_sequence_positions_test.dart --name 'benchmark:'
```

A broader widget selection also exposed 12 existing failures. They were
reproduced against the original mobile baseline; they were not fixed as part
of this task. The passing figures above refer to the explicitly focused test
selections, not the entire mobile repository.
