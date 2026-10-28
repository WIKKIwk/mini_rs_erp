# Sonic JSON research

Experimental, opt-in comparison of `serde_json` 1.0.149 and `sonic-rs` 0.5.10.
Production performance has not been established. Both codec switches default to
`serde_json`; the Sonic dependency is excluded unless `sonic-json` or
`sonic-json-research` is enabled. No deployment is part of this research.

The subsequent encoder qualification uses Sonic's `sanitize` feature and an
encoder-only build option. See [production preparation](sonic-json-production.md).
The original results below describe the earlier, unsanitized research binary;
they are historical measurements, not timings of the hardened encoder.

## Boundaries

- The administrator response helper covers production queue snapshots, order
  pages/details, catalogs, supply windows and other existing helper callers.
- Production state/page deltas, worker list patches, legacy full snapshots,
  notification snapshots and supply-task streams use the selected encoder.
- Preparation order lists/details and their live messages use the same encoder.
- Administrator `parse_json` requests use the independently selected decoder.
- Domain `serde_json::Value`, PostgreSQL JSONB, persisted documents, cursor/scope
  identities, WIP fingerprints, reactivation tokens and canonical AASX bytes
  retain their existing encoding. Codec selection is not a new wire protocol.
- Existing authorization, domain projections, patch construction, replay bounds,
  frame limits, prepared-frame caches and compression remain the measured path.

The default HTTP branch still delegates to Axum `Json<T>`. The Sonic branch
returns an owned byte buffer, the same `application/json` success content type,
and a plain-text HTTP 500 on serialization failure. Wire values are serialized
through their existing Serde implementations. No `sonic_rs::Value` migration or
unchecked parsing API is used.

## Decoder compatibility

Two concrete differences prevent an unconditional parser replacement:

1. Sonic 0.5.10 permits deeper typed JSON than the current Serde default and has
   known unbounded recursion paths. A depth fix was merged after this release:
   [upstream PR 247](https://github.com/cloudwego/sonic-rs/pull/247).
2. The current `serde_json` dependency does not enable `float_roundtrip`. Raw
   Sonic can round some numeric literals to a different final f64 bit and loses
   the sign when deserializing the valid integer token `-0` as f64. The
   harness includes a deterministic 10,000-number comparison and reports raw
   Sonic separately from the installed request decoder. These inputs are
   synthetic; mismatch counts are not production error rates.

The experimental decoder first scans structure while respecting quoted strings
and escapes. At 128 levels, on a negative/decimal/exponent literal, or at 19 integer
digits it delegates to the existing Serde parser. This preserves its depth,
ignored-field and numeric behavior. Otherwise it uses the checked Sonic parser.
The guard and fallback are included in decoder timings; slower outcomes count.
This is bounded compatibility evidence, not a universal equivalence proof.

## Configuration

Build with `--features sonic-json-research`, then choose independently:

```sh
export MINI_ERP_RESEARCH_JSON_ENCODER=sonic_rs
export MINI_ERP_RESEARCH_JSON_DECODER=serde_json
```

When using the legacy research encoder variable, omit `MINI_ERP_JSON_ENCODER`;
conflicting old/new encoder settings fail startup. The production candidate uses
`--features sonic-json` and `MINI_ERP_JSON_ENCODER=sonic_rs`, which cannot enable
Sonic request decoding.

Both accept `serde_json` or `sonic_rs`. Omitted values select Serde. Unknown
values and selecting Sonic without its Cargo feature fail startup. Settings
are read once per process; restarting is required to change them. Reset both
variables to `serde_json` to use the original codecs in a research build.

## Reproduce the benchmark

Build both codecs into the same binary with the same optimizer and CPU target:

```sh
CARGO_TARGET_DIR=target/sonic-json-research \
CARGO_BUILD_JOBS=4 CARGO_PROFILE_RELEASE_DEBUG=0 \
RUSTFLAGS='-C target-cpu=native' \
cargo build --release --locked --features sonic-json-research,verification \
  --example json_codec_research

python3 tools/benchmarks/json_codec_research.py \
  --binary target/sonic-json-research/release/examples/json_codec_research \
  --output garbage/sonic-json-research \
  --orders 20 285 1000 --samples 31 --batch 10 --repeats 3 \
  '--build-flags=-C target-cpu=native; release; jobs=4'
```

`target-cpu=native` is local benchmark configuration, not a global project flag.
The resulting binary must run on a compatible CPU. x86_64 requires its own
measurement; an AArch64 result is not an x86 server prediction.

The runner rotates the three profiles (`serde_json`, Sonic encoding only, Sonic
encoding plus guarded decoding), each in a fresh temporary working directory.
The fixture uses the real Router/domain services with in-memory production,
session and permission stores. Local supporting state is temporary. It opens
only a loopback socket and neither connects to a database nor loads `.env`.
The runner removes application/deployment environment overrides from children.

Measurements include:

- Alternating, warmed codec serialization of exact HTTP list/snapshot values,
  typed domain snapshots/maps and actual captured WebSocket update payloads.
- Checked typed request decoding, including the compatibility guard/fallback.
- Real Router responses with unchanged domain/DOM construction, both identity
  and gzip HTTP output; worker/admin windows and bodyless `304` reads.
- Full initial legacy WebSocket snapshot delivery, worker deltas for one/eight
  connections, administrator page deltas and legacy snapshot updates.
- A real map PUT, including authorization, parsing, validation and persistence
  into isolated memory stores. This is not a parser-only timing.

Every measured sample, p50/p95, payload byte count, process peak RSS, host/CPU,
Rust version, binary/lockfile/source hashes and profile are saved as JSON. Raw
codec variants must decode to identical JSON meaning. Initial/stream frame
types and revisions are asserted. Results preserve regressions as well as wins.
The baseline is the original Serde codec path in the same binary, including
the tiny research dispatcher, rather than a separately built historical binary.
Domain/read caches are warmed. WebSocket bootstrap measures a new connection
against that warm view; eight connections share one worker identity and scope.
HTTP gzip uses the existing level-8 zlib-rs setting. WebSocket compression is
disabled in the loopback fixture to expose message preparation cost.

No WAN, Flutter, real PostgreSQL, production concurrency, battery usage or
production throughput improvement is claimed by this fixture.

## Results

Measured on 2026-10-28: Apple M4, 10 CPU cores, 16 GiB RAM, AArch64 NEON,
Rust 1.98.1, release with `target-cpu=native`. The final source is `241081cd`.
There are 27 process runs: 20/285/1000 orders, three profiles, three repetitions,
31 samples per case. Codec samples batch ten operations and alternate engines.

For 285 orders (1,353,262 bytes in the full initial snapshot):

| Case | Serde p50 | Sonic encoder p50 | Elapsed-time change |
| --- | ---: | ---: | ---: |
| Encode the same snapshot value | 0.878 ms | 0.578 ms | -34.2% |
| Actual full HTTP response, identity | 4.372 ms | 3.376 ms | -22.8% |
| Actual full HTTP response, gzip 8 | 7.217 ms | 6.270 ms | -13.1% |
| Actual worker list HTTP, 20 rows | 0.0650 ms | 0.0585 ms | -9.9% |
| Legacy WebSocket initial snapshot | 5.135 ms | 4.866 ms | -5.2% |
| Worker delta delivery, 1 connection | 7.379 ms | 7.329 ms | -0.7% |
| Worker delta delivery, 8 connections | 7.424 ms | 7.419 ms | -0.1% |

Codec rows use the median of the nine paired run medians for that dataset.
Router/WS rows compare the median of three per-profile run medians. Full HTTP
identity responses improve by 20.5–24.3% across the three order counts. The
Sonic HTTP path also uses an owned Vec instead of Axum's BytesMut writer;
the paired codec benchmark compares Vec output on both sides separately.

Small delta delivery and page updates show no convincing improvement. The
actual worker delta is about 474 bytes; its encoding takes roughly 0.3 µs,
while view/patch construction and delivery take milliseconds. Bodyless 304
reads show timing variation despite doing no payload encoding. Small gains or
regressions in these whole-path timings are not established codec effects.

The guarded decoder is slower: the 1,212-byte map request takes 2.338 µs on
Serde versus 2.571 µs through the guarded Sonic setting (+10.0%). The 82-byte
integer-only request takes 0.204 µs versus 0.233 µs (+14.3%). Mutation endpoint
results have no consistent gain. The useful research configuration on this
host is therefore **Sonic encoding with Serde decoding**. Both defaults remain
Serde. Production behavior remains unmeasured.

All 216 paired encoder comparisons are byte-identical in these fixtures. Raw
Sonic differs on 3,242/10,000 synthetic float literals; the integrated guard
has zero differences on that probe and preserves signed zero. No reliable
memory reduction is established by the whole-fixture peak RSS results.

All cases, slower outcomes, raw samples, source/binary hashes and RSS data are
in [the complete results](research/sonic-json-20261028/results.md), with
[machine/build metadata](research/sonic-json-20261028/metadata.json) and
[the raw summary](research/sonic-json-20261028/summary.json).

## Verification

The final codec suite passes 8/8 and the snapshot-cache regression suite passes
4/4 under both Serde and Sonic encoder/decoder settings. The existing worker
queue/stream suite passes 34, fails 2 and ignores 1 under both settings, also
when executed from isolated temporary working directories. The two matching
failures are:

- `all_window_keeps_the_workers_complete_history`: 61 maps versus expected 60.
- `worker_detail_history_fallback_requires_this_actors_persisted_history`:
  HTTP 404 versus expected 200.

These history assertions were not rewritten. No full-suite green claim is
made. Logs and binary/source hashes are in
[verification.json](research/sonic-json-20261028/verification.json).

The release server checks pass with and without the optional Sonic feature.
Focused rustfmt for the new codec, tests and benchmark sources and `git diff
--check` pass. Commands and logs are recorded in
[checks.json](research/sonic-json-20261028/checks.json).
