# Optional Sonic encoder: production preparation

Scope: response and WebSocket encoding. Request parsing remains Serde in the
`sonic-json` build. The separate `sonic-json-research` feature still permits the
experimental decoder; its observed numeric/depth differences and slower guarded
request parsing are documented in [the research report](sonic-json-research.md).

Both build profiles default to Serde. No deployment or remote push is included.

## Hardening

- `sonic-json` includes only the optional encoder. Selecting Sonic decoding
  without `sonic-json-research` fails startup and direct decode calls return an
  error. Persisted JSON, JSONB, hashes, cursors and protocol construction retain
  their existing implementation.
- `sonic-rs` stays pinned to 0.5.10 and now enables `sanitize`. Inspection of
  upstream `src/util/string.rs` showed that normal optimized tail handling can
  load past a string's allocation while remaining inside a mapped page. This
  feature copies the bounded tail before SIMD loading instead. It applies in
  release builds too, and its cost is included in the new measurements.
- `MINI_ERP_JSON_ENCODER` selects the response/stream encoder. The old
  `MINI_ERP_RESEARCH_JSON_ENCODER` remains supported; conflicting selections,
  unknown names, non-UTF-8 settings and unsupported features fail startup before
  the database is opened. Settings remain fixed until restart.
- Simultaneous initialization with identical settings now succeeds using a
  single `OnceLock` initialization. Tests exercise 32 concurrent callers in
  isolated child processes without unsafely mutating the test environment.

## Qualification coverage

`tests/json_codec_qualification.rs` compares actual encoded bytes, not only JSON
meaning. Deterministic property tests cover 16,384 combinations of arbitrary
f64/f32 bit patterns and signed/unsigned 128-bit integers, plus 2,048 recursively
generated Unicode/nested payloads. Additional cases cover nonfinite values,
Serde representations, map keys, escaping, large strings, concurrent buffers
and SIMD tails adjacent to inaccessible memory pages.

`tests/json_codec_stream_load.rs` uses the actual Router and TCP/WebSocket
implementation with 285 synthetic orders, eight independent worker identities,
an admin page stream, a legacy stream and a client that never consumes its large
snapshot. It checks 1,024 worker deltas, 128 page deltas, 128 legacy snapshots,
four reconnect/replay cycles, actor cursor isolation, unrelated-apparatus
isolation and continued progress while a reader is stalled. This is a bounded
load regression, not a long-duration production soak.

Existing HTTP/gzip, queue window, pagination, replay, detail authorization and
active-paddon cache suites run in separate processes for each codec profile.
Two pre-existing history fixture mistakes were corrected to model the current
assigned-apparatus authorization rule: other-worker history on CUT is visible;
an actor's history on an unrelated apparatus does not authorize CUT detail.
Production authorization/business logic was not changed to satisfy these tests.

## Repeat locally

Build and run from the repository:

```sh
mkdir -p target
CARGO_TARGET_DIR=target/sonic-json-qualification \
CARGO_BUILD_JOBS=4 CARGO_PROFILE_RELEASE_DEBUG=0 \
cargo test --release --locked --features sonic-json,verification \
  --test json_codec_research --test json_codec_qualification \
  --test active_paddon_snapshot_cache --test worker_queue_window \
  --test json_codec_stream_load --no-run --message-format=json \
  > target/json-codec-artifacts.jsonl

python3 tools/benchmarks/qualify_json_codec.py \
  --artifacts target/json-codec-artifacts.jsonl \
  --output garbage/json-codec-qualification --profiles serde_json sonic_encode
```

The runner removes inherited app/database settings and uses a fresh temporary
working directory for each executable. Test fixtures use memory stores and
loopback ephemeral ports. It saves binary/source hashes, host details and raw
test output. No production credentials or database are needed.

The Linux x86_64 workflow in `.github/workflows/json-codec.yml` covers default,
portable encoder, native encoder and research-decoder builds in release mode.
It uploads the complete qualification reports. Adding a workflow is not evidence
of a successful CI run; results must come from an executed run.

## Selecting and rolling back

Build the server with locked dependencies and `--features sonic-json`. Leave
`MINI_ERP_JSON_ENCODER=serde_json` for the existing behavior. Set it to `sonic_rs`
and restart to enable the optional encoder. Clear the legacy encoder variable
when using the new name. Do not compile production with the research feature.

Rollback is `MINI_ERP_JSON_ENCODER=serde_json` and a restart of the same binary.
No protocol or persisted-data migration is needed. CPU-specific benchmark flags
are not global project settings; build portable binaries for mixed CPU fleets.

Target-platform qualification and a staging run of the actual deployed binary
with PostgreSQL, compression/proxy settings and Flutter reconnect/list flows are
required before enabling Sonic in production. Local synthetic results alone do
not establish production latency, memory use or long-term stability.
