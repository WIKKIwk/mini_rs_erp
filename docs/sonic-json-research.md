# Sonic JSON research

Experimental, opt-in comparison of `serde_json` 1.0.149 and `sonic-rs` 0.5.10.
Production performance has not been established. Both codec switches default to
`serde_json`; the Sonic dependency is excluded unless `sonic-json-research` is
enabled. No deployment is part of this research.

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
   Sonic can round some numeric literals to a different final f64 bit. The
   harness includes a deterministic 10,000-number comparison and reports raw
   Sonic separately from the installed request decoder. These inputs are
   synthetic; mismatch counts are not production error rates.

The experimental decoder first scans structure while respecting quoted strings
and escapes. At 128 levels, on a decimal/exponent literal, or at 19 integer
digits it delegates to the existing Serde parser. This preserves its depth,
ignored-field and numeric behavior. Otherwise it uses the checked Sonic parser.
The guard and fallback are included in decoder timings; slower outcomes count.
This is bounded compatibility evidence, not a universal equivalence proof.

## Configuration

Build with `--features sonic-json-research`, then choose independently:

```sh
MINI_ERP_RESEARCH_JSON_ENCODER=sonic_rs
MINI_ERP_RESEARCH_JSON_DECODER=serde_json
```

Both accept `serde_json` or `sonic_rs`. Omitted values select Serde. Unknown
values and selecting Sonic without its Cargo feature fail startup. Settings
are read once per process; restarting is required to change them. Reset both
variables to `serde_json` to use the original codecs in a research build.

## Reproduce the benchmark

Build both codecs into the same binary with the same optimizer and CPU target:

```sh
CARGO_TARGET_DIR=target/sonic-json-research \
CARGO_BUILD_JOBS=2 CARGO_PROFILE_RELEASE_DEBUG=0 \
RUSTFLAGS='-C target-cpu=native' \
cargo build --release --locked --features sonic-json-research,verification \
  --example json_codec_research

python3 tools/benchmarks/json_codec_research.py \
  --binary target/sonic-json-research/release/examples/json_codec_research \
  --output garbage/sonic-json-research \
  --orders 20 285 1000 --samples 31 --batch 10 --repeats 3 \
  '--build-flags=-C target-cpu=native; release; jobs=2'
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

No WAN, Flutter, real PostgreSQL, production concurrency, battery usage or
production throughput improvement is claimed by this fixture.

## Results

Results and the selected research configuration are recorded after measurement.
