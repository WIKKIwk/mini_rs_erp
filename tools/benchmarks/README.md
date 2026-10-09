# WIP printing SQL benchmark

`wip_inputs.py` benchmarks the previous `stage_execution::inputs_tx` N+1 query
sequence and the new source SQL on PostgreSQL connection-local temporary tables.
It never reads or changes existing business rows and never requests a print job.
The fixtures copy the production batch column constraints and indexes, use one
route, contain 80% waiting and 20% in-use WIPs, and have configurable JSON padding.

Use a temporary tool environment; no project dependency change is required:

```sh
python3 -m venv /tmp/mini-erp-wip-bench-venv
/tmp/mini-erp-wip-bench-venv/bin/python -m pip install 'psycopg[binary]'
python3 tools/db/macos_keychain.py db -- \
  /tmp/mini-erp-wip-bench-venv/bin/python tools/benchmarks/wip_inputs.py \
  --mode before --sizes 10 100 1000 10000 --repetitions 7 \
  --output /tmp/mini-erp-wip-benchmark-before.json
```

For an after comparison, use the actual source and migration:

```sh
python3 tools/db/macos_keychain.py db -- \
  /tmp/mini-erp-wip-bench-venv/bin/python tools/benchmarks/wip_inputs.py \
  --mode both --sizes 10 100 1000 10000 --repetitions 7 \
  --migration migrations/postgres/0138_incremental_wip_lifecycle.sql \
  --new-source src/db/postgres_production_map/stage_execution.rs \
  --output /tmp/mini-erp-wip-benchmark-comparison.json
```

Alternatively, `--new-sql-json` accepts a JSON array of the actual `inputs_tx`
SELECT strings (PostgreSQL `$1` placeholders are accepted). Source SQL and
migration SHA-256 hashes are recorded. The benchmark runs the baseline first,
backfills the migration against the same temporary fixtures, then runs the new
queries. Temporary function names are explicitly qualified with `pg_temp`
because PostgreSQL does not search temporary namespaces for functions.
It also compares full-table
versus projected lifecycle counters and the costs of INSERT, UPDATE and DELETE,
each rolled back. Backfill counters and rollback preservation are asserted.
Schema-qualified migrations require a separately isolated database and are
deliberately refused by this temporary-table harness.

For a bounded million-row scale check, use `--mode after --sizes 1000000
--payload-bytes 0` with the same source and migration arguments. The tool permits
this size only for after-only, zero-padding fixtures. Each SQL statement has a
120-second timeout, and all temporary objects disappear when the connection
closes. The million-row check does not execute a million N+1 queries.

The first measured invocation is reported separately; it is **not** a cold disk
cache measurement. The remaining invocations produce a warm median and nearest
rank p95. With seven samples p95 is the largest sample, so treat it as a small
local comparison, not a production service-level estimate. Query counts exclude
BEGIN/COMMIT and fixture/backfill work. Memory is Python client traced allocation
in a separate untimed invocation, not PostgreSQL memory or Rust process RSS.

This measures the PostgreSQL stage-input and WIP-counter components. It excludes
Rust route resolution, unrelated lifecycle history, HTTP calls, mobile rendering,
printer connection and physical print time. The synthetic row counts do not
establish production printer throughput or capacity for billions of records.

## Actual service concurrency benchmark

The manual ignored Rust test
`benchmark_postgres_service_eight_apparatuses_and_selected_paddons` exercises the
actual canonical resolver and `ProductionMapService` prepare/commit path against
two equivalent disposable PostgreSQL databases. It runs eight Cut apparatuses
with distinct orders and selected paddons, four recorded cards per device, and
one fresh retry per saved card. Both modes use sixteen warmed connections, a
500 ms acquisition timeout and an eight-thread Tokio runtime.

The baseline wraps each action in a simulated old global mutex while retaining
the **current optimized service and SQL**. This isolates parallelism. Timings
include waiting for that gate and the actual service/database action; fixture
creation and connection warmup are excluded. HTTP, background live snapshots,
mobile UI and physical printing are excluded. The result asserts exact unique
WIPs, QRs and selected-paddon memberships, with no duplicate produced quantity.
Fresh retries can append existing zero-quantity audit events without creating
another WIP. Both disposable databases are dropped before result assertions.

Run from the repository root with the existing local Keychain maintenance
wrapper. The child constructs the credential-bearing test URL only in memory;
the URL is never printed or written to a result artifact:

```sh
python3 tools/db/macos_keychain.py --admin db -- python3 - <<'PY'
import os
from urllib.parse import quote

test_env = os.environ.copy()
host = test_env["PGHOST"]
if ":" in host:
    host = f"[{host}]"
test_env["MINI_ERP_TEST_ADMIN_DATABASE_URL"] = (
    "postgres://"
    + quote(test_env["PGUSER"], safe="") + ":"
    + quote(test_env["PGPASSWORD"], safe="") + "@"
    + host + ":" + test_env["PGPORT"] + "/"
    + quote(test_env["PGDATABASE"], safe="")
)
os.execvpe("cargo", [
    "cargo", "test", "--lib", "benchmark_postgres_service_eight",
    "--", "--ignored", "--nocapture",
], test_env)
PY
```

The test prints `WIP_SERVICE_STRESS` followed by its JSON result. Verified local
results and scope are saved in `logs/wip_optimization/service-stress.json` and
`logs/wip_optimization/report.txt`. These are local comparison samples, not a
production latency guarantee or a measurement of physical printer throughput.

The service stress produces 32 rolls per mode; the million-row SQL benchmark is
a separate fixture. The card-specific authorized GET bypasses full-history
pre-print bootstrap, while initial/full-queue/live snapshots can still fetch
historical WIPs. These results do not prove overall billion-record ERP capacity.
