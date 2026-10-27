# bbtree evaluation — 2026-10-25

## Decision

Keep every bbtree experiment opt-in. The measured WIP split/merge regression is substantial, snapshot program reuse shows no reliable gain, and apparatus-cache results change direction with size. The large-input audit validation phase is a follow-up candidate, with a memory tradeoff; this is not evidence to enable bbtree throughout the backend.

The dependency is exactly `bbtree = 0.1.5`. Default builds retain std collections. No SQL, migrations, HTTP contracts, permission checks, production actions, public DTO collection types or persisted formats were changed. No database, deployment or physical device was used by this experiment.

## Changes

| Feature | Changed private container | Preserved boundaries |
| --- | --- | --- |
| `bbtree-wip` | Six per-call lineage maps: batch/session/input indexes, parent/child adjacency, degrees | Public std sets, lineage rules, output ordering and store reads |
| `bbtree-audit` | Per-call order/session/batch/QR/active-work indexes | Original query/computation order, validation rules, report order and DTOs |
| `bbtree-snapshot` | Prior-program lookup in `compile_saved_maps_reusing_programs` | Full-definition equality, normalization, last duplicate wins, compilation fallback |
| `bbtree-apparatus` | Two cache maps inside their existing Tokio RwLocks | Both locks, generation checks, invalidation ordering and Arc ownership |

All four use `bbtree::unsync::BBTreeMap`; the concurrent `bbtree::BBTreeMap` is not introduced. In particular, the apparatus experiment changes the container while retaining the synchronization protocol. No `InlineKey` implementation was added for custom or tuple keys; these retain ordinary `Ord` comparisons. Public `BTreeSet` and Serde-based maps retain their existing types.

`bbtree-experiment` enables all four candidates. `bbtree-bench` adds synthetic workloads only. `bbtree-alloc` builds separate allocation-instrumented executables. No feature loads `.env` or starts the ERP server as part of benchmarking.

## Measurements

Host: Apple M6, arm64, 12 logical CPUs, 32 GiB RAM, macOS 27.0.1; Rust/Cargo 1.99.1. Portable `release` builds, without `target-cpu=native`. The [raw results](../tools/benchmarks/bbtree_results_2026-10-25.json) contain every sample, binary/source hash, run order and host load observation.

There are 41 deterministic cases. Each latency case warms once and runs for at least 200 ms, over six rounds alternating which process runs first. The table reports the **median of six per-run mean iteration times**, not per-request p50/p95. Negative time change means less time; positive means slower. All samples and outliers are retained.

Fixture construction, database reads and fingerprints are outside timing. All fixture cases remain resident. Each measured result is fingerprinted in full, and std, bbtree, and both separately instrumented executables must produce the same fingerprints before timing. No compilation or test task was scheduled by this evaluation during the measurement phase; background activity on the shared desktop was not controlled.

| Workload | Size | std ms | bbtree ms | Time change |
| --- | ---: | ---: | ---: | ---: |
| `wip_lineage_chain` | 512 | 1.8169 | 1.7004 | -6.41% |
| `wip_lineage_split_merge` | 1537 | 8.8684 | 13.4313 | +51.45% |
| `wip_lineage_split_merge_permuted` | 1537 | 8.9849 | 13.5051 | +50.31% |
| `audit_loaded_validation_clean` | 5000 | 17.1336 | 16.1253 | -5.89% |
| `audit_loaded_validation_mixed` | 5000 | 18.2454 | 17.0361 | -6.63% |
| `audit_batch_index_build_clone` | 5000 | 2.1003 | 2.0556 | -2.13% |
| `snapshot_program_reuse_mixed` | 391 | 0.6794 | 0.6758 | -0.54% |
| `snapshot_program_reuse_mixed` | 5000 | 11.1289 | 11.3477 | +1.97% |
| `apparatus_warmed_runtime_and_configuration` | 64 | 0.0032 | 0.0030 | -6.61% |
| `apparatus_warmed_runtime_and_configuration` | 512 | 0.0339 | 0.0398 | +17.48% |

### Interpretation

- **WIP:** the 512-row chain uses 6.41% less time, but the 1,537-row split/merge takes 51.45% more time; its permuted form takes 50.31% more. Both large split/merge variants lose all six paired rounds and their timing ranges do not overlap. A global WIP switch is therefore rejected. The helper includes repeated full-index scans; no CPU profile was taken to attribute the regression to a particular bbtree primitive.
- **Audit:** loaded validation at 5,000 rows saves 5.89%/6.63% time, consistently in all six paired rounds. This is roughly 1.0–1.2 ms in this synthetic phase. Its additional peak live allocation grows by 9.44%/8.86%. The separate batch-index construction saves 2.13% at 5,000 rows, with overlapping timing ranges, and is slower at 100/1,000 rows. Full audit latency remains unmeasured.
- **Snapshot:** the 391-map mixed case changes by -0.54%; the unchanged case by +0.62%. Results overlap and alternate direction. At 5,000 mixed maps the median is 1.97% slower, with one 14.577 ms candidate run retained. There is no reliable gain to justify enabling the feature.
- **Apparatus:** a warmed batch touching both caches for 64 IDs saves 6.61%, approximately 0.215 microseconds per batch of 128 lookups. At 512 IDs it takes 17.48% more time, losing all six paired rounds. These are single-task warmed hits, including RwLock/Arc/Vec/current-thread runtime overhead, not a contention benchmark. Counters prove that no repository read occurs inside a measured hit batch.

### Allocation tradeoffs

Allocation counts and bytes come from separately compiled executables running three fixed iterations. Their instrumented timings are **excluded** from speed results. Requested bytes are Rust allocation/reallocation requests, not allocator overhead or OS RSS. Peak growth is additional requested live memory over warmed fixtures/previous output; cold-cache footprint, total process RAM and memory retention under production churn are unmeasured.

| Workload | Size | std peak growth KiB | bbtree peak growth KiB | Requested-byte change |
| --- | ---: | ---: | ---: | ---: |
| `wip_lineage_chain` | 512 | 771.56 | 813.75 | +1.78% |
| `wip_lineage_split_merge` | 1537 | 2270.21 | 2378.29 | +1.42% |
| `audit_loaded_validation_clean` | 5000 | 784.28 | 858.35 | +0.47% |
| `audit_loaded_validation_mixed` | 5000 | 915.71 | 996.81 | +0.46% |
| `audit_batch_index_build_clone` | 5000 | 7663.28 | 7455.77 | -2.71% |
| `snapshot_program_reuse_mixed` | 391 | 2983.56 | 2987.13 | +0.10% |
| `apparatus_warmed_runtime_and_configuration` | 64 | 1.00 | 1.00 | +0.00% |

Fewer allocation calls do not imply lower memory usage: the WIP/audit validation candidates request slightly more bytes and have higher temporary peaks. Warmed apparatus hits allocate the same vectors in both variants; this does not measure the two caches' stored map footprint. The benchmark executable grows from approximately 3,022.5 KiB to 3,289.6 KiB (+8.8%); this is not a server/APK size measurement. Cargo.lock adds bbtree and seven supporting packages, without upgrading existing dependencies.

## Correctness and existing failures

The [validation record](../tools/benchmarks/bbtree_validation_2026-10-25.json) preserves failure names and all 12 current complete-snapshot hashes.

| Focused check | std | bbtree experiment |
| --- | --- | --- |
| New lineage/audit/program-reuse/cache regressions | 27 passed | 27 passed |
| Existing QR-report unit/HTTP cases | 8 passed | 8 passed |
| Existing audit-page case | 1 passed | 1 passed |
| Existing worker-snapshot scope/revision cases | 13 passed | 13 passed |
| Existing snapshot reuse/retry/error cases | 6 passed | 6 passed |
| Existing stage-topology cases | 5 passed; 1 benchmark ignored | 5 passed; 1 benchmark ignored |
| Existing warehouse QR cases | 5 passed | 5 passed |
| Existing order-scan bootstrap suite | 19 passed; 15 failed | 19 passed; same 15 failed |
| Historical full-snapshot golden | 1 failed | Same failure and same actual hash |
| Additional current full-snapshot parity research | 12 fixtures match cached/rebuilt | Same 12 complete hashes |

The 27 new cases include missing/corrupt/cyclic provenance, split sibling exclusion, real merge inputs, remainder parents, warehouse boundaries, duplicate/trimmed batch and session IDs, invalid graphs, stale/duplicate program-cache entries, report ordering, cache Arc identity and in-flight stale reads across both cache invalidations.

Two existing test problems were reproduced with bbtree **disabled**:

1. `tests/order_scan_bootstrap.rs`'s `CountedQolipStore` does not implement `product_by_code`. The existing Qolip service selects that exact lookup for nonempty product codes; the port default returns `StoreFailed`. This causes the same 15 HTTP-500/gate-timeout failures in both variants. Production PostgreSQL and memory stores implement the port. Original fixture/assertions were not changed.
2. `tests/snapshot_print_hold_reuse.rs` compares with historical pre-optimization goldens. Its first `zero` fixture expects `8f307a0cefa918e43bc07923b2c706c8ad3e33fe70d18e643d216b0de12a72fa`, whereas **both** current variants return `1dc5ddf0ef806f36ce83568d655166aa4660b5c3e797d8dd3fcd05770e8a1aa1`. The historical assertion remains. A separate ignored research test was appended to compare every current full fixture across variants, including cached reads and explicit invalidation/rebuild; all 12 match exactly.

This is focused evidence, not a completely green full-suite claim. The selected no-database checks do not establish PostgreSQL load capacity, long-running churn, production p99, mobile/WAN latency or device behavior.

## Reproduction

From the backend repository:

```sh
cargo test --locked --lib bbtree_
cargo test --locked --lib --features bbtree-experiment bbtree_

cargo test --locked --features verification --test snapshot_print_hold_reuse \
  bbtree_current_snapshot_fixture_fingerprints -- --ignored --nocapture
cargo test --locked --features verification,bbtree-experiment \
  --test snapshot_print_hold_reuse bbtree_current_snapshot_fixture_fingerprints \
  -- --ignored --nocapture

python3 tools/benchmarks/bbtree_research.py --rounds 6 --target-ms 200
```

The runner builds and copies std/bbtree and separate allocation binaries before measuring. `--skip-build` requires those saved binaries and verifies their hashes and relevant tracked source fingerprint. Workload-only inspection is available through `target/bbtree-research/std --list`.

The raw report's build commit is `3a1708bf`; report-time HEAD is `bcfb4dd8`. Intervening changes affect the runner metadata and the appended integration research test, not compiled runtime/fixture sources; all four saved binaries carry the same tracked-source fingerprint. The report truthfully records `source_dirty: true` because an unrelated untracked standalone example appeared in the shared workspace. Broader `working_tree_status` also records Finder metadata. These files were not compiled into the named benchmark and are excluded from this task's commits. Source/binary hashes are the reproducibility authority; the environment was a shared desktop.

## Full case table

| Workload | Size | std ms | bbtree ms | Time change | std range ms | bbtree range ms |
| --- | ---: | ---: | ---: | ---: | --- | --- |
| `wip_lineage_chain` | 64 | 0.1062 | 0.1037 | -2.42% | 0.1055–0.1099 | 0.1022–0.1080 |
| `wip_lineage_chain_reverse` | 64 | 0.1065 | 0.1045 | -1.85% | 0.1059–0.1079 | 0.1021–0.1066 |
| `wip_lineage_chain` | 512 | 1.8169 | 1.7004 | -6.41% | 1.7628–1.8862 | 1.6491–1.7279 |
| `wip_lineage_chain_reverse` | 512 | 1.7733 | 1.6708 | -5.78% | 1.7413–2.1827 | 1.6445–1.7236 |
| `wip_lineage_chain` | 2048 | 18.1608 | 18.1240 | -0.20% | 17.0953–20.2054 | 17.1709–18.8998 |
| `wip_lineage_chain_reverse` | 2048 | 18.4134 | 18.0977 | -1.71% | 17.1390–30.5329 | 17.1471–18.8852 |
| `wip_lineage_split_merge` | 49 | 0.0737 | 0.0853 | +15.71% | 0.0719–0.0923 | 0.0849–0.0859 |
| `wip_lineage_split_merge_permuted` | 49 | 0.0739 | 0.0861 | +16.59% | 0.0722–0.0773 | 0.0855–0.1116 |
| `wip_lineage_split_merge` | 385 | 1.0349 | 1.2378 | +19.61% | 1.0067–1.0606 | 1.2322–1.2471 |
| `wip_lineage_split_merge_permuted` | 385 | 1.0113 | 1.2343 | +22.04% | 0.9990–1.0486 | 1.2220–1.2439 |
| `wip_lineage_split_merge` | 1537 | 8.8684 | 13.4313 | +51.45% | 8.7048–9.1522 | 12.9905–13.8168 |
| `wip_lineage_split_merge_permuted` | 1537 | 8.9849 | 13.5051 | +50.31% | 8.7568–9.1736 | 12.9545–13.6126 |
| `audit_loaded_validation_clean` | 100 | 0.1325 | 0.1341 | +1.18% | 0.1312–0.1360 | 0.1320–0.1371 |
| `audit_loaded_validation_mixed` | 100 | 0.1342 | 0.1354 | +0.94% | 0.1313–0.1372 | 0.1347–0.1403 |
| `audit_batch_index_build_clone` | 100 | 0.0338 | 0.0349 | +3.29% | 0.0328–0.0339 | 0.0344–0.0414 |
| `audit_loaded_validation_clean` | 1000 | 1.7692 | 1.7182 | -2.88% | 1.7199–1.8225 | 1.6826–2.1080 |
| `audit_loaded_validation_mixed` | 1000 | 1.7928 | 1.7624 | -1.69% | 1.7586–1.8219 | 1.7114–2.0852 |
| `audit_batch_index_build_clone` | 1000 | 0.3789 | 0.3909 | +3.18% | 0.3660–0.3843 | 0.3863–0.4117 |
| `audit_loaded_validation_clean` | 5000 | 17.1336 | 16.1253 | -5.89% | 16.7983–17.8659 | 15.6618–16.5158 |
| `audit_loaded_validation_mixed` | 5000 | 18.2454 | 17.0361 | -6.63% | 17.8027–18.8751 | 16.5346–17.6537 |
| `audit_batch_index_build_clone` | 5000 | 2.1003 | 2.0556 | -2.13% | 2.0097–2.1177 | 2.0293–2.0865 |
| `snapshot_program_reuse_unchanged` | 100 | 0.1441 | 0.1442 | +0.06% | 0.1418–0.1456 | 0.1424–0.1471 |
| `snapshot_program_reuse_mixed` | 100 | 0.1708 | 0.1715 | +0.40% | 0.1674–0.1736 | 0.1695–0.1989 |
| `snapshot_program_reuse_unchanged` | 391 | 0.5676 | 0.5712 | +0.62% | 0.5589–0.5802 | 0.5600–0.5838 |
| `snapshot_program_reuse_mixed` | 391 | 0.6794 | 0.6758 | -0.54% | 0.6609–0.6841 | 0.6649–0.6781 |
| `snapshot_program_reuse_unchanged` | 1000 | 1.5413 | 1.5389 | -0.16% | 1.5004–1.5625 | 1.4947–1.5717 |
| `snapshot_program_reuse_mixed` | 1000 | 1.8301 | 1.8192 | -0.59% | 1.7867–1.8473 | 1.7715–1.8394 |
| `snapshot_program_reuse_unchanged` | 5000 | 9.4668 | 9.6356 | +1.78% | 9.2983–9.6760 | 9.2560–9.8788 |
| `snapshot_program_reuse_mixed` | 5000 | 11.1289 | 11.3477 | +1.97% | 10.9675–11.4105 | 11.1845–14.5774 |
| `snapshot_program_reuse_unchanged_ordered` | 391 | 0.5533 | 0.5626 | +1.67% | 0.5476–0.6548 | 0.5562–0.5658 |
| `snapshot_program_reuse_mixed_ordered` | 391 | 0.6650 | 0.6693 | +0.65% | 0.6574–0.6842 | 0.6629–0.6759 |
| `snapshot_program_reuse_unchanged_reverse` | 391 | 0.5535 | 0.5661 | +2.29% | 0.5412–0.5590 | 0.5479–0.5673 |
| `snapshot_program_reuse_mixed_reverse` | 391 | 0.6595 | 0.6718 | +1.86% | 0.6462–0.6676 | 0.6516–0.6757 |
| `snapshot_program_reuse_unchanged_ordered` | 1000 | 1.4991 | 1.5323 | +2.21% | 1.4665–1.5193 | 1.4962–1.5506 |
| `snapshot_program_reuse_mixed_ordered` | 1000 | 1.7926 | 1.8219 | +1.64% | 1.7263–1.8195 | 1.7561–1.8496 |
| `snapshot_program_reuse_unchanged_reverse` | 1000 | 1.5210 | 1.5503 | +1.93% | 1.4855–1.5525 | 1.5005–1.5896 |
| `snapshot_program_reuse_mixed_reverse` | 1000 | 1.8239 | 1.8494 | +1.40% | 1.7560–1.8483 | 1.7927–2.0508 |
| `apparatus_warmed_runtime_and_configuration` | 16 | 0.0007 | 0.0007 | -5.98% | 0.0007–0.0007 | 0.0007–0.0007 |
| `apparatus_warmed_runtime_and_configuration` | 64 | 0.0032 | 0.0030 | -6.61% | 0.0032–0.0033 | 0.0030–0.0031 |
| `apparatus_warmed_runtime_and_configuration` | 128 | 0.0069 | 0.0066 | -3.34% | 0.0068–0.0070 | 0.0066–0.0068 |
| `apparatus_warmed_runtime_and_configuration` | 512 | 0.0339 | 0.0398 | +17.48% | 0.0336–0.0342 | 0.0377–0.0412 |
