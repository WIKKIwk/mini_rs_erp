# Sonic encoder qualification — 2026-10-28

The optional encoder passed local compatibility and bounded load regressions.
Keep Serde as the default until Linux runtime/CI and staging qualification are
complete. No push, deployment or production database workload was performed.

Apple M4 / AArch64 NEON, Rust 1.98.1, release, native CPU flags, Sonic 0.5.10
with `sanitize`. All figures are synthetic; the cross-built Linux binary was
not executed. Source and binary hashes are in each run directory.

## Verification

| Build | Runtime profiles | Passed | Failed | Ignored |
| --- | --- | ---: | ---: | ---: |
| research | Serde / Sonic encode / Sonic both | 171 | 0 | 3 |
| encoder only | Serde / Sonic encode | 114 | 0 | 2 |

Each profile passed 57 test cases and completed 8,192 worker deltas, 1,024 page
deltas, 1,024 legacy snapshots and 32 reconnects with eight independent workers
and one stalled reader. The ignored item is the unrelated manual gzip CPU
experiment. Property tests also exercise 16,384 numeric combinations and 2,048
nested Unicode payloads per profile, plus guarded memory boundaries.

Default and encoder-only server release checks passed. A portable Linux x86_64
ELF server build passed; its hash is in `checks/linux-build.json`. SSH runtime
access timed out. The new Linux CI matrix has not been executed.

Focused Rust formatting, Python compilation and whitespace passed. The full
repository hygiene check still fails on legacy/unrelated files; its unfiltered
output is preserved in `checks/repository-hygiene.log`.

## Encoder-only benchmark, 285 orders

31 samples/case, batch 10 for codec microbenchmarks, three independent runs per
profile in rotating order. Values below are medians of the three run p50s;
negative percentages mean less time. The research decoder is compiled out.

| Case | Serde p50, ms | Sonic p50, ms | Time change |
| --- | ---: | ---: | ---: |
| http_full_snapshot_identity | 4.5096 | 3.6937 | -18.1% |
| http_full_snapshot_gzip | 7.6240 | 7.0864 | -7.1% |
| http_worker_list_20 | 0.0661 | 0.0605 | -8.4% |
| http_admin_page_20 | 0.9776 | 0.9717 | -0.6% |
| http_production_map_put | 0.5583 | 0.5614 | +0.6% |
| http_worker_unchanged_304 | 0.0166 | 0.0165 | -0.8% |
| ws_legacy_initial_snapshot | 6.1867 | 5.3719 | -13.2% |
| ws_legacy_update_1_clients | 11.3030 | 10.9627 | -3.0% |
| ws_worker_update_1_clients | 8.5791 | 8.1133 | -5.4% |
| ws_worker_update_8_clients | 8.3070 | 7.9349 | -4.5% |
| ws_page_update_1_clients | 4.3089 | 5.0427 | +17.0% |

Paired serialization of the initial full snapshot was 0.905 to 0.703 ms. All
256 paired payload comparisons across 32 runs (18 research-build runs at
20/285/1000 orders, six short encoder-only runs and eight longer runs at 285) produced identical bytes.
The small worker response saves about 5.6 microseconds. The page stream is
slower in this aggregate, while its direct encoding is faster (0.579 to 0.433
microseconds). Therefore stream pipeline timings cannot be attributed solely
to the serializer, and a general production latency improvement is not claimed.

## Longer confirmation of stream variability

The same encoder-only binary was run again with 101 samples/case, batch 5 and
four repeats per profile. Profile order was balanced (two runs first for each
engine). This was a response to the slower page result, not a discarded run.

| Case | Serde p50, ms | Sonic p50, ms | Time change |
| --- | ---: | ---: | ---: |
| http_full_snapshot_identity | 4.5886 | 3.9259 | -14.4% |
| http_full_snapshot_gzip | 7.5653 | 7.0374 | -7.0% |
| http_worker_list_20 | 0.0662 | 0.0609 | -8.1% |
| ws_worker_update_1_clients | 7.7741 | 8.1370 | +4.7% |
| ws_worker_update_8_clients | 8.2163 | 8.2261 | +0.1% |
| ws_page_update_1_clients | 4.7571 | 4.4012 | -7.5% |

Full HTTP retains a 14–18% improvement across the two encoder-only run sets;
gzip retains about 7%. Worker and page stream results change sign or approach
zero. The experiments therefore support a useful large-response encoder option,
not a general stream acceleration or permission to enable it in production.
Keep the existing Serde default until actual Linux/staging qualification.

## Artifact layout

- `native-research/`: all five test suites, three runtime profiles, raw logs.
- `native-encoder/`: the same suites with research decoding compiled out.
- `native-benchmark/`: hardened research binary, three fixture sizes, 18 runs.
- `native-encoder-benchmark/`: encoder-only binary, 285-order confirmation, six runs.
- `native-encoder-confirmation/`: 101-sample, balanced four-repeat confirmation, eight runs.
- `checks/`: feature graphs, build/check output, ELF hash and hygiene diagnostics.

Each comparison uses one frozen executable for both codec settings. Metadata
records source hashes and checkout state; unrelated concurrent checkout changes
are not part of the codec change. Synthetic stores, fresh working directories
and loopback ephemeral ports keep the tests independent of deployed data.
The source commit for the unchanged codec module is recorded in `summary.json`.

See [production preparation](../../sonic-json-production.md) for reproduction,
configuration, decoder restrictions and rollback.
