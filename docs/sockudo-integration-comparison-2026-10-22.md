# Sockudo integration comparison — 2026-10-22

## Result and scope

On the real isolated ERP dataset, worker WebSocket update traffic fell **95.29–95.49%** across the measured 1/8-client cases. Both versions matched authoritative REST state in **21 checks each**. The final implementation also reduced administrator HTTP bootstrap bodies by **12.45–13.56%**, after detecting and fixing an initial gzip backend regression.

These percentages describe the measured bytes below. They are not a claim that all mobile Internet usage or server computation fell by the same amount.

## Provenance and isolation

- Backend baseline: `95d1863597b73fe4183cab33b2060362db0da6c4`.
- Mobile baseline: `703cfd8d5ff81e9c3b5263e834fb80eb5b985736`.
- Both repositories were clean and already committed before engineering changes.
- The baseline backend was built from an exact Git archive; all **1,304 tracked source blobs** were independently checked against that commit.
- Baseline release binary SHA256: `1b5b6712c3d2afeb91a0f8bc7727354a98ef099205322b9d5b1e4e53db6867ac`.
- Final release binary SHA256: `4f8a9e2c1a767b4a1b0beb0be2ba82e8afc7b2e4eb5f97535ed8bcd17dbbea5a`.
- Final release build was followed by an unchanged `--locked` build completing in 0.47 seconds, before measurements.
- Dataset: **508 production maps, 40 workers**; the same consistent read-only source dump restored into two private databases. Dump SHA256: `74647afca61ced54f3f2c77d8dc5b1671897b0ab1b2884f61517e824d8199fe5`.
- Selected principal/apparatus/order fingerprints matched exactly between runs. Twelve test workers authenticated; two additional apparatus assignments existed only in each disposable clone.
- The fixture copied no Telegram/FCM service credentials. No source database writes or live-user notifications were performed.
- Both timed servers used the same controlled setting: the order reconciler ran initially, then `MINI_ORDER_SYNC_INTERVAL_SECONDS=0` disabled background reconciliation. Production default 30-second reconciliation was not part of these timings.
- Release binaries, compiler details, tracked working diff hashes and relevant file hashes are recorded in [baseline provenance](/Users/wikki/Desktop/Accord_project/outputs/sockudo_integration_20261022/baseline-source.json), [final provenance](/Users/wikki/Desktop/Accord_project/outputs/sockudo_integration_20261022/current-source.json), and [mobile provenance](/Users/wikki/Desktop/Accord_project/outputs/sockudo_integration_20261022/mobile-source.json).

## Real ERP: worker streams

Three cycles produced six print-preflight hold/release mutations per case. Before used the current baseline's **state_delta_v1**, not a historical full-snapshot protocol. After used **state_delta_v2 + negotiated permessage-deflate**. Counts below include delivered WebSocket data-frame headers and payloads, summed across the listed clients. They exclude the initial HTTP snapshot, HTTP mutation responses, TCP/IP and TLS overhead.

| Scenario | Before bytes | After bytes | Reduction |
| --- | ---: | ---: | ---: |
| same_principal_1 | 49,536 | 2,282 | 95.39% |
| same_principal_8 | 396,344 | 18,656 | 95.29% |
| shared_apparatus_workers_8 | 427,386 | 19,267 | 95.49% |

The shared-worker case used three distinct principals across eight connections. Authorized views, exact target transitions and unrelated-apparatus isolation were checked against REST.

### CPU and latency

| Scenario | Before p95 ms | After p95 ms | Before server CPU ms | After server CPU ms |
| --- | ---: | ---: | ---: | ---: |
| same_principal_1 | 435.20 | 447.33 | 1981.67 | 1975.22 |
| same_principal_8 | 474.66 | 473.61 | 1969.47 | 1984.12 |
| shared_apparatus_workers_8 | 702.99 | 651.22 | 6508.57 | 6440.39 |

Each p95 here has only **six mutation samples**. One-client p95 rose slightly; other cases improved. This run establishes a large bandwidth reduction, not a statistically established latency improvement. CPU remains dominated by existing application/snapshot work and stayed broadly comparable. Compression does not remove that computation.

### Disconnect recovery

Both versions replayed **two events with zero HTTP snapshot reloads** after the short disconnect. Replay WebSocket bytes fell from **16,693 to 1,521 (90.89%)**. Resume behavior was exercised on the real application, separately from synthetic fresh connections.

## Real ERP: administrator pages

Administrator pages continue to negotiate **page_delta_v1**. Sockudo compression applies to their existing protocol. Each main case below includes six updates, with no page reloads; separate cases also checked every intermediate page against REST.

| Page workload | Initial HTTP gzip bytes | Subsequent WS/reload bytes | Initial + six updates | Reduction for this combination |
| --- | ---: | ---: | ---: | ---: |
| admin_sequence_40 | 17,298 → 15,145 | 40,701 → 3,000 | 57,999 → 18,145 | 68.71% |
| admin_orders_60 | 36,835 → 31,841 | 23,493 → 2,020 | 60,328 → 33,861 | 43.87% |

The combined column includes page HTTP bodies and WS updates only; mutation responses, HTTP headers and handshakes are excluded. It is deliberately smaller than the worker update-only percentage because the initial page still has to be downloaded.

Two actual reorder operations also restored the original order, matched all three worker principals and matched authoritative administrator pages without reloads.

## Optional minimal reorder ACK

This capability was measured **within the same final binary**, using a complete canonical baseline of **53 IDs**:

| Response mode | Decoded JSON bytes | Actual HTTP gzip bytes |
| --- | ---: | ---: |
| Existing full ACK | 1,115 | 382 |
| `Prefer: return=minimal` | 535 | 277 |

Decoded response reduction: **52.02%**; gzip response reduction: **27.49%**. Canonical server operations, base/version, count and fingerprint reconstructed exactly the REST sequence; the original order was restored.

Paged administrator clients retain full ACKs unless they possess a complete canonical baseline. The request is opt-in, and the browser CORS allowlist/exposed headers support `Prefer`/`Preference-Applied`.

## HTTP gzip regression caught and corrected

Sockudo enables the `zlib-rs` flate2 backend through Cargo feature unification. At the old HTTP compression level 3, administrator bootstrap bodies initially grew by approximately **23–25%**. This was measured and not accepted as a final result.

The calibrator then compressed **the exact same decoded clone response bodies** with the preserved baseline miniz backend and the new zlib-rs backend. Bodies were passed in memory over stdin; only byte counts/timings were retained. Every level round-tripped correctly. Twenty-one warmed samples were taken per level per page.

| Same HTTP input | Old miniz level 3 bytes | New zlib-rs level 8 bytes | Old sum of page median ms | New sum of page median ms |
| --- | ---: | ---: | ---: | ---: |
| sequence, 389,108 decoded bytes | 19,065 | 16,770 | 1.018 | 0.919 |
| orders, 806,355 decoded bytes | 37,007 | 31,979 | 0.960 | 1.972 |

Level 8 produced smaller output than level 9 for both measured inputs, with less CPU. Final HTTP default is **8**, configurable through `MINI_ERP_HTTP_GZIP_LEVEL` (0–9). The final fresh-clone rerun above confirmed smaller bootstrap traffic than the original committed application. These sums are per-page compression medians, not end-to-end API latency.

## Synthetic transport/protocol separation

The dedicated loopback fixture compares actual Axum/tungstenite 0.29.0 and the final Sockudo application adapter in the same release executable, on identical pre-serialized messages. It directly includes the production Rust sequence-edit helper. An independent client applies each result and compares it against a separately generated expected sequence.

Final matrix: **336 cases, 25,704 validated bootstrap/update states**, queue sizes **1/10/1,000/10,000**, **1/8 concurrent clients**, sixteen updates, and reorder/replace/status/snapshot scenarios. A one-ID reorder is a no-op coverage case. Production/disabled/Dedicated 1/3/6/Shared 3 compression profiles were included. Compile-time source hashes matched checkout before and after the run, including the final safe-buffer core SHA256 `9348716694c71f102667e239d87d3b21aef5c0df12ed1faec39d23c4ca67a3b8`.

### Ten-thousand-ID reorder, eight clients

| Transport and application protocol | Mean frame bytes per update/client | Server transport CPU ms for 128 updates |
| --- | ---: | ---: |
| Baseline tungstenite / v1 | 780262.81 | 21.01 |
| Sockudo / same v1 / PMD3 | 56241.06 | 124.41 |
| Sockudo / v2 / compression off | 560.69 | 2.45 |
| Sockudo / v2 / PMD3 | 84.19 | 3.74 |

Overall reduction for this synthetic case: **99.9892%**. The intermediate rows show that semantic list edits and transport compression contribute separately. Application mutation/diff/serialization timings are recorded separately in the raw results; server transport CPU above excludes dataset preparation and database/application work.

### Compression tuning on that v2 workload

| Mode | Level | Mean update frame bytes | CPU ms / 128 updates | Initial synthetic snapshot bytes/client |
| --- | ---: | ---: | ---: | ---: |
| dedicated | 1 | 87.94 | 3.15 | 133,359 |
| dedicated | 3 | 84.19 | 3.59 | 84,606 |
| dedicated | 6 | 83.69 | 4.62 | 84,599 |
| shared | 3 | 232.25 | 4.11 | 84,606 |

The default remains **Dedicated, level 3, threshold 128 bytes, 15-bit (32 KiB) LZ window, context takeover enabled**. Level 6 saved about half a byte per update in this case, while costing more CPU. Level 1 enlarged the initial snapshot substantially. Shared mode trades compressor memory pooling for loss of the per-connection dictionary; it sent larger repeated small deltas here. Window size is not total connection RAM.

These are synthetic UUID lists over localhost. Their WS snapshot bootstrap is a transport experiment; production state streams bootstrap through HTTP. Concurrent fixture clients request their own next message, so this is not a single-business-event broadcast capacity test. Python validation and scheduling affect the recorded loopback RTT. TCP/IP/TLS headers, WAN behavior, mobile battery use and large compressed client uploads were not measured. RSS includes shared fixture messages, application caches and allocator retention; no isolated per-connection RAM claim is made.

## Real Dart interoperability and focused verification

- Real `dart:io WebSocket.connect` used its default compression options with both production/Dedicated and Shared transport profiles, for v1 and v2.
- **68 decoded frames** from 1,000-ID queues matched expected revisions/order using the actual production mobile sequence reducer.
- A **650 ms application-idle phase** with native Dart `pingInterval=100 ms` kept each connection healthy; subsequent updates and close completed.
- The probe records the actual HTTP extension response, because this installed SDK's `WebSocket.extensions` getter returns an empty string even when compression is active.
- Engineering verification reported **124 vendor Rust tests + 87 focused root Rust tests**, and **70 focused Flutter tests** passing. Focused analysis was clean; Dart VM and optimized JavaScript sequence fingerprint/reducer goldens matched. This is not a full platform release-build claim.
- Known pre-existing failure, reproduced on both exact baseline and current code: `http::admin_route_tests::production_map_basic::production_map_sequence_returns_backend_visible_order_ids`, [production_map_basic.rs:256](/Users/wikki/Desktop/Accord_project/mini_rs_erp/src/http/admin_route_tests/production_map_basic.rs:256). The GET assertion failed singly on both versions; its unrelated behavior was not changed. No claim that the full Rust suite passed is made.

## Reproduction and retained artifacts

Build and run the synthetic comparison from the backend checkout:

```sh
cargo build --locked --release --bin mini_rs_erp --example sockudo_stream_fixture
python3 tools/benchmarks/sockudo_stream_benchmark.py --self-test
python3 tools/benchmarks/sockudo_stream_benchmark.py --binary target/release/examples/sockudo_stream_fixture --output /tmp/accord-sockudo-synthetic.json --counts 1,10,1000,10000 --clients 1,8 --updates 16 --repeats 1
dart tools/benchmarks/sockudo_dart_probe.dart --binary target/release/examples/sockudo_stream_fixture
```

The Dart probe requires the sibling `accord_mobile_v2` checkout and a compatible Dart SDK. The full ERP comparison uses the existing [guarded private-clone fixture](/Users/wikki/Desktop/Accord_project/mini_rs_erp/tools/benchmarks/order_stream_fixture.py) through the macOS Keychain maintenance wrapper. Before uses `--protocols modern`; after uses `--protocols modern-v2 --websocket-compression --minimal-ack`. Both used `--clients 1,8 --cycles 3`. Use a fresh private run directory to prepare new clones; a new source dump may differ from the historical dataset identified by its saved hash.

Artifacts contain aggregate metrics, hashes and configuration, not business payloads:

- [Real baseline metrics](/Users/wikki/Desktop/Accord_project/outputs/sockudo_integration_20261022/erp-clones/before-sockudo-comparison.json)
- [Real final metrics](/Users/wikki/Desktop/Accord_project/outputs/sockudo_integration_20261022/erp-clones/after-sockudo-comparison.json)
- [Final synthetic matrix](/Users/wikki/Desktop/Accord_project/outputs/sockudo_integration_20261022/synthetic-matrix.json)
- [Final native Dart probe](/Users/wikki/Desktop/Accord_project/outputs/sockudo_integration_20261022/dart-probe.json)
- [Same-input gzip calibration](/Users/wikki/Desktop/Accord_project/outputs/sockudo_integration_20261022/http-gzip-same-input-calibration.json)
- [Vendored source provenance and patches](/Users/wikki/Desktop/Accord_project/mini_rs_erp/third_party/sockudo-ws/ACCORD_PATCHES.md)
- [Cleanup record](/Users/wikki/Desktop/Accord_project/outputs/sockudo_integration_20261022/cleanup.json)

Both owned clone servers were stopped and **only the two databases created for this run were dropped**. The **five pre-existing benchmark databases** remain. The private source dump, credential seed, copied auxiliary business data, clone data directories and clone logs were removed. Redacted metrics, source/binary provenance and reproducible benchmark code remain.
