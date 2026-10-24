# Production output statistics API

`GET /v1/mobile/admin/production-maps/output-stats` provides the Accord home
chart with server-side aggregation. It returns meters and **net** kilograms
together. The frontend does not need to download orders, sessions or WIP lists.
This addition does not change production commands or the frontend.

## Request and access

Send the existing session token as `Authorization: Bearer <token>`.

| Query | Required | Meaning |
| --- | --- | --- |
| `from` | yes | RFC3339 instant, inclusive |
| `to` | yes | RFC3339 instant, exclusive |
| `bucket` | yes | Exactly `hour`, `day` or `week` |
| `timezone` | yes | IANA zone, for example `Asia/Tashkent`, `America/New_York` or `UTC` |
| `apparatus` | no | Exact canonical ID, for example `apparatus:default:asset-010` |

Dates must have an offset, years 0001–9999, no leap seconds, and at most
microsecond precision (extra trailing zeroes are allowed). Sub-microsecond
bounds are rejected instead of silently rounded. Unknown/repeated query keys
and empty apparatus IDs are rejected.

Access follows the existing production-map/WIP read policy:

- `admin.access` or `production.map.manage`: all producer apparatuses.
- `apparatus.queue.read`: only the principal's assigned canonical apparatus IDs.
- An explicitly requested apparatus outside that scope returns 403.
- An unassigned queue reader returns 403. Authentication is required even for an
  empty range. Scope is applied before aggregation and coverage accounting.

Without `apparatus`, series include accessible catalog apparatuses, including
zero-output apparatuses, plus historical producer IDs present in the requested
range. The catalog is left-joined: a missing/blank name falls back to the ID.
An explicitly requested historical ID can be read even without a catalog row.
A well-formed ID absent from both catalog and output history returns 400.

## Response

Complete response fixtures for frontend integration:
[complete](examples/output-stats-complete.json),
[partial quantities](examples/output-stats-partial.json),
[empty observed day](examples/output-stats-empty.json). These are illustrative
fixtures, not extracts of live production data.

The final JSON is serialized by PostgreSQL so NUMERIC sums do not pass through
floating-point arithmetic on the server. JSON number values use ERP's six
decimal places without rounding individual outputs before summation.

| Field | Meaning |
| --- | --- |
| `ok` | `true` on a successful report |
| `from`, `to` | Normalized UTC query bounds |
| `timezone`, `bucket` | Requested calendar grouping |
| `as_of` | One database transaction observation instant, in UTC |
| `quantity_basis` | Always `latest_revision` |
| `coverage.complete` | No selected real outputs were excluded for unknown production time |
| `coverage.approximate_timestamp_count` | Included outputs with reconstructed/non-exact completion provenance |
| `coverage.excluded_unknown_timestamp_count` | Selected outputs excluded because their production time is unknowable |
| `coverage.warnings` | Objects containing a stable `code` and `count` |
| `series[].apparatus_id`, `apparatus_name` | Original canonical producer and current display name/fallback ID |
| `series[].totals` | Counts and quantities across the included points |
| `series[].points` | Chronologically ordered calendar buckets |
| `points[].start`, `end` | Actual bucket boundaries with their real UTC offsets |
| `points[].is_partial` | Query cuts a bucket, or the bucket has not ended at `as_of` |

Every totals/point object contains `batch_count`, `meters` and `net_kg`.
Every metric contains:

- `value`: sum if every counted output has this measurement; otherwise `null`.
- `known_value`: sum of measurements that are known, including genuine zeroes.
- `missing_count`: number of counted outputs missing this measurement.

`batch_count` counts distinct persisted physical outputs, even when one or both
measurements are unknown. A missing quantity does **not** change
`coverage.complete`; use that metric's `missing_count`.

### Complete quantities

Illustrative one-apparatus, one-day result:

```json
{
  "ok": true,
  "from": "2026-10-18T19:00:00Z",
  "to": "2026-10-19T19:00:00Z",
  "timezone": "Asia/Tashkent",
  "bucket": "day",
  "as_of": "2026-10-24T07:00:00Z",
  "quantity_basis": "latest_revision",
  "coverage": {"complete": true, "approximate_timestamp_count": 0, "excluded_unknown_timestamp_count": 0, "warnings": []},
  "series": [{
    "apparatus_id": "apparatus:default:asset-010",
    "apparatus_name": "Rezka",
    "totals": {
      "batch_count": 2,
      "meters": {"value": 12500, "known_value": 12500, "missing_count": 0},
      "net_kg": {"value": 860.5, "known_value": 860.5, "missing_count": 0}
    },
    "points": [{
      "start": "2026-10-19T00:00:00+05:00",
      "end": "2026-10-20T00:00:00+05:00",
      "is_partial": false,
      "batch_count": 2,
      "meters": {"value": 12500, "known_value": 12500, "missing_count": 0},
      "net_kg": {"value": 860.5, "known_value": 860.5, "missing_count": 0}
    }]
  }]
}
```

### Partial quantities

For the same response envelope and boundaries, if only one of two outputs has
known net weight, both `totals` and the point contain:

```json
{
  "batch_count": 2,
  "meters": {"value": 12500, "known_value": 12500, "missing_count": 0},
  "net_kg": {"value": null, "known_value": 430.25, "missing_count": 1}
}
```

Coverage remains complete. The UI can show “at least 430.25 kg”, independently
of the fully known meter total. Do not replace the null value with zero.

### Empty observed day

For an accessible apparatus with no dated outputs, the same envelope contains
one zero point and these zero totals (coverage is complete unless there are
unknown-time exclusions):

```json
{
  "apparatus_id": "apparatus:default:asset-010",
  "apparatus_name": "Rezka",
  "totals": {
    "batch_count": 0,
    "meters": {"value": 0, "known_value": 0, "missing_count": 0},
    "net_kg": {"value": 0, "known_value": 0, "missing_count": 0}
  },
  "points": [{
    "start": "2026-10-19T00:00:00+05:00",
    "end": "2026-10-20T00:00:00+05:00",
    "is_partial": false,
    "batch_count": 0,
    "meters": {"value": 0, "known_value": 0, "missing_count": 0},
    "net_kg": {"value": 0, "known_value": 0, "missing_count": 0}
  }]
}
```

A wholly future interval returns empty `points` arrays and zero observed totals,
without manufactured future zero points. An empty accessible catalog/history
returns `series: []`.

## Which records count

The source is `mini_progress_batches`, one row per `batch_id`:

- Pause with status `paused`/`resumed`, detach with `roll_detached`/`resumed`,
  and roll-complete/complete with status `completed` are actual output rows.
  This mirrors `OrderProgressBatch::has_consistent_action_status`.
- A pause without physical output, handoff, resume event, print/reprint and
  correction audit is not itself a new batch. Existing resumed batch identity
  is counted once. Distinct Rezka frame outputs in one session each count once.
- `payload_json.origin = layer_remainder` is existing returned material, excluded
  even when its timestamp/producer is inherited from its parent.
- Opening balances live in `mini_opening_wip_*`, which are not queried.
  Training workspace tables are not queried. Reserved `training-`/`demo-` order
  prefixes are also excluded from the source as a defensive boundary.
- No current WIP status, current/next apparatus, warehouse receipt, active
  session or current order lifecycle filter is used. Consumption, movement,
  receiving or archival cannot relocate/remove the original output's history.
- `canonical_apparatus_id` is the producer. Catalog/map/session joins never
  decide whether an output exists. Parent input lineage is not another copy of
  an output; no join expands it into extra rows.

This is **processed volume per apparatus**. The same material processed by two
machines contributes to both. Summing machine series does not measure unique
finished product leaving the factory.

### Production time and coverage

Use only persisted immutable `production_completed_at` from migration 0151 and
the output writer. Never use session/batch `updated_at`, correction time,
warehouse time, current time or a session-wide spread of quantities.

- `production_timing_source.completion = output_event` is exact.
- `batch_created_at` and other non-exact persisted provenance are included at
  the saved instant and counted in `approximate_timestamp_count`, with warning
  `approximate_production_timestamps`. Approximation alone does not omit rows.
- Modern output writes without a provenance object are trusted: the inspected
  `progress_batch_record`/`put_order_progress_batch_tx` path explicitly persists
  the output's completion instant. Migration 0151 marks repaired legacy rows.
- NULL, non-finite, epoch-zero/negative timestamps or explicit completion
  provenance `unknown` are excluded. No substitute date is invented.
  `excluded_unknown_production_timestamps` is reported and `complete=false`.

Unknown-time exclusions are counted over **all history of the selected producer
scope**, since such rows cannot truthfully be placed inside or outside a date
range. Unselected apparatuses cannot affect these counts. Empty buckets with
`complete=false` mean “no dated observed output”, with the coverage warning
showing that undated output was omitted.

Exact historical time cannot be recovered from a creation-time approximation.
Deleted source history and unmarked imported/demo data indistinguishable from
real output cannot be reconstructed/classified by this report.

### Meters, gross, tare and corrections

Meter precedence follows the existing WIP/layer-remainder/merge quantity rules:
use `finished_goods_meter` when present; otherwise `produced_qty` only for
`m`, `meter` or `metr` (case-insensitive, trimmed). Do not add the two.
An invalid explicit meter measurement is unknown, not a fallback. Never treat
kg/pieces as meters, or derive meters from weight.

Net weight mirrors `core/production_map/paddon/weights.rs` and its PostgreSQL
correction enrichment:

1. Find the latest audit up to the batch's current revision that **changes kg**
   at ERP precision. Its new `finished_goods_kg` is authoritative gross; an
   explicit removal remains unknown. Later note/meter/tare-only audits do not
   supersede that measured gross.
2. Without a kg-changing audit, use non-null payload `gross_qty` when present;
   otherwise use the column `finished_goods_kg`. `produced_qty/uom=kg` is not a
   measured-gross substitute under the existing paddon contract.
3. Subtract explicitly known `bobina_kg`. Missing/invalid tare, invalid gross,
   or tare greater than gross makes net kg unknown. Nothing is clamped to zero.
   Gross zero is an unknown measurement in the existing domain; gross equal to
   a positive known tare is a genuine **net zero**. Known zero tare is valid.

Current batch columns supply corrected meters/tare. Audited gross is checked
against the same batch revision within the same read snapshot. Thus
`quantity_basis=latest_revision` means a correction made today updates its
original production date, without adding a new output on the correction date.
This API is not a historical “quantities as they were known then” audit.

## Calendar and observation rules

The data filter is `[from, min(to, as_of))`. Calendar points retain their full
real boundaries even when the query selects only part of a bucket. Day starts
at local midnight; week starts Monday midnight. Hourly DST repetitions are
separate points with distinct offsets; skipped wall times create no phantom
hour. Half-hour transitions and skipped dates are handled too. Day/week length
therefore need not be 24/168 hours.

Only points starting before `as_of` and overlapping the observed query interval
are returned. Current unfinished buckets are partial; future points are omitted.
No output is split across hours. Past observed empty buckets return known zeroes.

The report uses a PostgreSQL **REPEATABLE READ, READ ONLY** transaction, one
observation timestamp and one quantity aggregation pipeline. Totals are sums of
those same point aggregates. There is no per-order query, 200/500-row list limit,
response cache, or cross-user result sharing. Responses set `Cache-Control:
no-store`.

For comparisons, call this endpoint twice with explicit intervals. For example,
“today so far” and “yesterday to the same local time” can use different `to`
bounds. During DST, calculate these as calendar dates in the chosen timezone.
The two calls have their own `as_of`; they are not an atomic pair of snapshots.

## Limits and errors

- Maximum interval: 366 × 24 hours.
- Maximum requested buckets per apparatus: 1,500 (including future buckets).
- Maximum requested apparatus × bucket points: 50,000, including zero/future
  points. Results are never truncated to satisfy a limit.
- Each report SQL statement has an 8-second timeout. A timeout fails the report.
- IANA timezone rules are bundled by `chrono-tz`; upgrade that dependency when
  timezone authorities change civil-time rules.

Errors follow the existing JSON shape `{"error":"code"}`:

| HTTP | Code |
| --- | --- |
| 401 | `unauthorized` |
| 403 | `forbidden` |
| 400 | `output_stats_invalid_query` (malformed/unknown/repeated query key) |
| 400 | `output_stats_invalid_from`, `output_stats_invalid_to` |
| 400 | `output_stats_invalid_interval` (`from >= to`, or interval >366 days) |
| 400 | `output_stats_invalid_bucket`, `output_stats_invalid_timezone` |
| 400 | `output_stats_invalid_apparatus`, `output_stats_unknown_apparatus` |
| 400 | `output_stats_too_many_buckets`, `output_stats_too_many_points` |
| 400 | `output_stats_invalid_timezone_boundary` (unresolvable civil-time gap) |
| 500 | `output_stats_store_failed` (including unavailable DB/timeout) |

Mutating HTTP methods receive the router's 405 response; standard HEAD requests
use the same read handler without a response body. Database errors never
become empty successful reports. Numeric aggregates stay numeric in JSON;
clients needing exact six-place arithmetic for extremely large totals should
use a decimal-aware JSON reader.

## Curl integration examples

Set `ACCORD_API_BASE` to the HTTPS backend origin and `ACCORD_TOKEN` to an existing
session token. These calls do not modify production data.

Daily view, hourly chart (19 October in Tashkent):

```bash
curl --get "$ACCORD_API_BASE/v1/mobile/admin/production-maps/output-stats" \
  -H "Authorization: Bearer $ACCORD_TOKEN" \
  --data-urlencode 'from=2026-10-18T19:00:00Z' \
  --data-urlencode 'to=2026-10-19T19:00:00Z' \
  --data-urlencode 'bucket=hour' --data-urlencode 'timezone=Asia/Tashkent'
```

Weekly view, daily chart (19–25 October in Tashkent):

```bash
curl --get "$ACCORD_API_BASE/v1/mobile/admin/production-maps/output-stats" \
  -H "Authorization: Bearer $ACCORD_TOKEN" \
  --data-urlencode 'from=2026-10-18T19:00:00Z' \
  --data-urlencode 'to=2026-10-25T19:00:00Z' \
  --data-urlencode 'bucket=day' --data-urlencode 'timezone=Asia/Tashkent'
```

One apparatus, longer period grouped by Monday weeks:

```bash
curl --get "$ACCORD_API_BASE/v1/mobile/admin/production-maps/output-stats" \
  -H "Authorization: Bearer $ACCORD_TOKEN" \
  --data-urlencode 'from=2026-09-27T19:00:00Z' \
  --data-urlencode 'to=2026-10-25T19:00:00Z' \
  --data-urlencode 'bucket=week' --data-urlencode 'timezone=Asia/Tashkent' \
  --data-urlencode 'apparatus=apparatus:default:asset-010'
```

Migration `0164_production_output_stats_indexes` adds only report indexes. The
existing migration runner applies it during normal backend deployment. Index
DDL has a 2-second lock timeout and 30-second statement timeout, and rolls back
on failure. No live database migration is part of developing this change.

## Verification

Focused calendar, HTTP authorization/validation and disposable PostgreSQL
contract tests live in the three `output_stats` test modules. The DB test is
explicitly ignored by default to prevent accidental use of a live database:

```bash
cargo check --lib --locked
cargo test --lib output_stats --locked
OUTPUT_STATS_TEST_DATABASE_URL=postgresql://test_user@127.0.0.1:PORT/postgres \
  cargo test --lib output_stats_postgres_contracts --locked -- --ignored
```

The explicit database URL must point at a disposable test server with database
creation privileges. The test creates its own random database, applies the
actual registered schema migrations and deletes that database afterward.
The existing schema migrations also require a `mini_rs_erp` runtime role on
that isolated server; a `NOLOGIN` role suffices for these tests.

Verified on 2026-10-24:

- `cargo check --lib --locked`: passed (existing unrelated compiler warnings).
- Focused `output_stats` tests, including the explicitly enabled PostgreSQL
  test: **11 passed, 0 failed, 0 ignored**. The disposable database used the
  actual migration set through 0164.
- The database test covers 601 outputs, exact micro-unit sums and large decimal
  totals, correction provenance, output identity, moved/received WIP, opening
  balances, training/demo exclusions, missing measurements, DST buckets,
  historical catalog gaps, undated-output coverage and report limits. Business
  table fingerprints before/after the reporting read matched.
- Formatting of all new Rust files, `git diff --check`, JSON examples, route
  snapshot/contract loading and generated-contract registry checks passed
  (109 generated contracts across seven existing migration bundles).

Performance verification uses the 601-output fixture; production-size latency
has not been measured. The full unrelated application test suite and full
verifier scenario runner were not run for this addition.
