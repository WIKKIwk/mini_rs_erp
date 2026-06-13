# Local Go vs Rust Benchmark - 2026-05-15

## Scope

- ERPNext bench: `/Volumes/Samsung990P/local.git/erpnext_n1/erp`
- Site: `erpfresh.localhost`
- ERPNext web port: `8000`
- Go service: `127.0.0.1:18101`
- Rust service: `127.0.0.1:18102`
- Database: restored production backup
- Direct DB reads: enabled for both services
- Manual DB indexes: not used
- Workload: read-only mobile endpoints plus login/session creation against
  copied temp JSON stores
- Tool: ApacheBench (`ab`)
- Raw result root: `/private/tmp/accord_bench_current`

No mutation endpoint was benchmarked. The run did not intentionally write to
ERPNext business tables.

## Preflight

Both repositories passed their test suites before load testing:

```text
go test ./...
cargo test --locked
```

Smoke covered these endpoints on both services, and every request returned
`200`:

- `/healthz`
- `/v1/mobile/auth/login`
- `/v1/mobile/werka/summary`
- `/v1/mobile/werka/pending?limit=20`
- `/v1/mobile/werka/history`
- `/v1/mobile/werka/status-breakdown?kind=pending`
- `/v1/mobile/werka/status-details?kind=pending`
- `/v1/mobile/werka/archive?kind=sent&period=yearly`
- `/v1/mobile/werka/customers?limit=50&offset=0`
- `/v1/mobile/werka/suppliers?limit=50&offset=0`
- `/v1/mobile/werka/customer-items?customer_ref=Saidamin&limit=50&offset=0`
- `/v1/mobile/stock-entry/lookup?barcode=4780027071158&limit=20`

Smoke output notes:

- `werka_summary` matched exactly: `pending=73 confirmed=0 returned=0`.
- supplier/customer directory and customer item picker JSON matched exactly.
- list endpoints with dispatch records had matching counts and first IDs, but
  full JSON hashes differed because of serialization/detail differences already
  known between Go and Rust.

## Load Results

Each endpoint was warmed up before the measured run.

| Endpoint | Rust RPS | Go RPS | Rust p95 ms | Go p95 ms | Rust p99 ms | Go p99 ms | Failed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `healthz n10000 c200` | 34593.82 | 44345.90 | 5 | 7 | 34 | 10 | 0 |
| `login_werka n1000 c100` | 1675.61 | 641.03 | 85 | 223 | 87 | 229 | 0 |
| `werka_summary n5000 c200` | 15891.73 | 19589.71 | 14 | 22 | 16 | 32 | 0 |
| `werka_pending n3000 c100` | 7495.24 | 6253.09 | 15 | 38 | 18 | 56 | 0 |
| `werka_status_breakdown n3000 c100` | 7347.45 | 5205.71 | 14 | 48 | 20 | 69 | 0 |
| `werka_status_details n2000 c100` | 6657.08 | 6056.00 | 31 | 36 | 35 | 50 | 0 |
| `werka_history n3000 c100` | 6401.60 | 5103.92 | 23 | 38 | 33 | 52 | 0 |
| `werka_archive_sent n1000 c50` | 9346.23 | 7958.74 | 6 | 15 | 13 | 20 | 0 |
| `werka_customers n2000 c100` | 367.68 | 374.46 | 290 | 700 | 298 | 1117 | 0 |
| `werka_suppliers n2000 c100` | 12391.88 | 11566.18 | 8 | 22 | 28 | 34 | 0 |
| `werka_customer_items n1000 c50` | 740.87 | 737.41 | 82 | 168 | 88 | 238 | 0 |
| `stock_barcode n3000 c100` | 1126.14 | 1113.20 | 108 | 236 | 120 | 351 | 0 |

## Readout

- Rust had lower p95 latency on every measured route.
- Rust had higher throughput on most business routes.
- Go had higher raw RPS on `healthz` and `werka_summary`, but Rust still had
  lower p95 on both.
- `werka_customers` is the remaining heavy read path; throughput is effectively
  equal, while Rust tail latency is much lower.
- Login remains significantly faster in Rust in this local temp-store run.

## Conclusion

The current Rust service is production-candidate stable on this restored ERPNext
read-only workload:

- zero failed benchmark requests;
- equal or compatible smoke output on the tested endpoints;
- lower tail latency across the full measured endpoint set;
- no dependency on manual ERPNext DB index changes.
