# Local order stream comparison

`order_stream_fixture.py` and `order_stream_benchmark.py` exercise the actual
release server, PostgreSQL, HTTP mutations and WebSocket protocols. They are
macOS tools, not a simulated throughput model or a Flutter frame benchmark.

## Isolation

Run through `tools/db/macos_keychain.py --admin db`. The fixture only accepts a
loopback PostgreSQL host. It takes one read-only `pg_dump`, then restores separate
`bench_order_stream_before_*` and `bench_order_stream_after_*` databases.
Every API mutation targets private localhost ports 18091/18092. Original orders,
roles, credentials and assignments are not changed.

The local access-code vault may use an older encryption key. The fixture seeds
temporary credentials inside the clones, preserving principal IDs and roles.
The harness gives two otherwise unassigned existing workers an apparatus
assignment in each clone, through the real administrator endpoint. This permits
distinct-worker fanout and actor-specific permission checks when the source
dataset has only one assigned worker per apparatus.

The private run directory has mode 0700; database dumps, metadata, test credential
seed, auxiliary data and logs have mode 0600. Credentials/tokens are never written
to result files, command-line arguments or tool output. Results contain only
counts, timings, fingerprints and verification outcomes.

Server processes receive a minimal environment: local database settings,
encryption key, local HTTP bind, pool size and log filter. The project `.env` and
external integration credentials are not loaded. Stop uses a recorded PID and
checks its exact private executable path before sending a signal.

## Reproduction

Preserve the baseline source with `git archive <commit>` before edits. Build it
with a separate `CARGO_TARGET_DIR`, `cargo build --release --locked --offline
--bin mini_rs_erp`. Build the candidate in a separate target directory too.
Finish compilation before measuring either version.

Create a Python virtual environment with `cryptography` and `argon2-cffi`; the
HTTP/WebSocket client itself uses only the Python standard library. Replace the
paths and commits below with your local paths:

```sh
python3 tools/db/macos_keychain.py --admin db -- "$BENCH_PYTHON" \
  tools/benchmarks/order_stream_fixture.py --run-dir "$BENCH_DIR" \
  prepare --baseline-revision "$BASELINE_COMMIT"

python3 tools/db/macos_keychain.py --admin db -- "$BENCH_PYTHON" \
  tools/benchmarks/order_stream_fixture.py --run-dir "$BENCH_DIR" \
  start --mode before --binary "$BASELINE_BINARY" \
  --source-revision "$BASELINE_COMMIT"

python3 tools/db/macos_keychain.py --admin db -- "$BENCH_PYTHON" \
  tools/benchmarks/order_stream_benchmark.py --run-dir "$BENCH_DIR" \
  --mode before --clients 1,8,32 --cycles 5
```

Repeat `start` and the benchmark with `--mode after`, the candidate binary and its
commit. If new migrations require the maintenance role, pass
`--migration-binary /absolute/path/to/mini_rs_migrate` to `start`. Both modes
restore the same saved dump and auxiliary data. Each `start` discards that mode's
previous clone; result files persist until a subsequent benchmark uses the same
output suffix.

Run SQL-count diagnostics separately: restore/start with `--query-log`, then run
the benchmark with `--query-counts-only --cycles 1 --output-suffix sql-counts`.
Query logging changes runtime cost, so those timing numbers are not the primary
performance results. Each diagnostic phase counts `sqlx::query` statement events
from the private log without copying SQL or bind data into the report.
The diagnostic sets `MINI_ORDER_SYNC_INTERVAL_SECONDS=0`: the normal initial
order reconciliation runs once, then periodic reconciliation stops. This keeps
unrelated 30-second background SQL out of the small per-mutation statement count.
Primary timing and idle runs retain the production-default 30-second reconciler.

For the separate two-OS-process check, stop the `before` server and run
`order_stream_crossprocess.py --run-dir "$BENCH_DIR"` through the same maintenance
wrapper while the candidate `after` server is ready. It starts a second candidate
process on port 18091 with independent logins, shared clone PostgreSQL data and an
explicit copy of the role/config files. Worker and administrator streams on that
process must match both servers' authorized HTTP content after hold, release,
reorder and inverse reorder requests made through port 18092. The second process
is stopped in `finally`. Restore again before collecting primary timing results.

Stop private servers using the fixture's `stop --mode before` / `stop --mode
after` commands. Database clones and private artifacts are intentionally retained
for follow-up diagnosis; remove them explicitly once they are no longer needed.

## Measurements and assertions

- Warm stream fanout with 1, 8 and 32 clients: one worker principal, then three
  different worker principals sharing an apparatus. Five actual print-preflight
  hold/release cycles change status/control values.
- A held-order correctness check compares each distinct worker's reduced stream
  state with a fresh authorized HTTP response while the hold is active and after
  release. This catches accidentally shared actor-specific controls.
- Different apparatus scopes receive the same mutation. Missing events are only
  accepted when the resulting authorized REST view is unchanged. This accounts
  for legitimate upstream/downstream dependencies.
- Thirty seconds of idle connected worker streams measures process CPU,
  ping/pong bytes and data messages across a complete 25-second heartbeat and a
  normal reconciliation interval. This does not invent a periodic HTTP poll that
  the current mobile worker screen does not perform.
- Reconnect with a recent old cursor checks ordered replay or a safe HTTP
  fallback, then exact REST equality.
- A `store_failed` stream error explicitly closes that stream, fetches an
  authorized HTTP snapshot and reconnects. Both versions use this same bounded
  recovery, recording the error and its additional GET/body/frame costs.
  Successful stream deltas that disagree with HTTP fail the check; they are
  never silently replaced from the oracle. Unknown errors and more than three
  recoveries on one client fail the run.
- Administrator windows retain 40 sequence rows and 60 order rows. The baseline
  invalidation protocol reloads the loaded prefix; the candidate sparse patches
  and metadata checkpoints are applied directly. Both are checked against exact
  fresh HTTP pages, including versions and pagination cursors.
- A real version-checked sequence move and its inverse verify worker ordering,
  administrator pages, controls, no lost rows and restoration of original order.
- Administrator notification bootstrap records actual payload size and how many
  full order maps were transmitted.

Darwin `proc_pid_rusage` provides server CPU, RSS and physical footprint. CPU
ticks are converted using `mach_timebase_info`; CPU percent can exceed 100% for a
multithreaded server. A 50 ms sampler captures process memory peaks. PostgreSQL
statistics are read before/after each phase and are labeled as transactions,
buffer hits/reads and tuple counters, **not** SQL query counts. The statistics
flush wait is outside the CPU/latency measurement window.

WebSocket counts include frame headers but exclude the HTTP upgrade handshake;
HTTP byte counts include response bodies (gzip where negotiated) but exclude
HTTP headers. Network traffic is loopback, without TLS or WebSocket compression.
Latency begins before the mutation HTTP request and ends after a client applies
its response event. Bootstrap and exact-REST verification requests are outside
fanout timing phases. Administrator required reload GETs are inside their phases.

These results establish behavior and relative local server cost for the captured
dataset. They do not measure device battery, mobile rendering, WAN loss, TLS,
distributed throughput, or unlimited production capacity. Multiple-process
correctness is established separately by the real two-process harness and the
PostgreSQL listener tests; it is not a distributed load benchmark.
