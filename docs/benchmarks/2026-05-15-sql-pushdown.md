# SQL Pushdown Benchmark - 2026-05-15

This benchmark compares the current raw-row Rust aggregation style against SQL
pushdown on the restored local ERPNext bench.

## Environment

- Bench: `/Volumes/Samsung990P/local.git/erpnext_n1/erp`
- Site: `erpfresh.localhost`
- Database: restored production backup
- Backend repo: `accord_mobile_server_rs`
- Probe: `cargo run --locked --bin sql_pushdown_bench`
- Iterations per case: 300 measured runs after warmup

The probe is read-only. It does not mutate ERPNext data.

## Correctness Check

The probe first compares raw-row/Rust results with SQL-pushdown results.

Result:

- equality: ok
- summary: `pending=73 confirmed=0 returned=0`
- pending records: `73`

Important behavior captured by the probe:

- `summary` and `pending` hide unannounced pending purchase receipts, matching
  the current Rust builders;
- `status_breakdown` follows the current builder behavior and does not apply
  that hidden-receipt exclusion;
- no summary/count/breakdown path uses sampling or pre-filter limits.

## Results

| Case | Raw/Rust median | SQL pushdown median | Speedup |
| --- | ---: | ---: | ---: |
| `summary` | 1.210 ms | 0.143 ms | 8.5x |
| `pending` | 0.799 ms | 0.555 ms | 1.4x |
| `status_breakdown:pending` | 1.353 ms | 0.362 ms | 3.7x |
| `status_breakdown:confirmed` | 0.864 ms | 0.212 ms | 4.1x |
| `status_breakdown:returned` | 0.875 ms | 0.179 ms | 4.9x |

Full run output:

```text
summary_raw_rows_rust_count: avg=1.269ms median=1.210ms p95=1.652ms min=0.964ms
summary_sql_pushdown: avg=0.173ms median=0.143ms p95=0.315ms min=0.117ms
pending_raw_rows_rust_filter: avg=0.804ms median=0.799ms p95=0.856ms min=0.762ms
pending_sql_pushdown: avg=0.570ms median=0.555ms p95=0.603ms min=0.531ms
breakdown_raw_rows_rust_group:pending: avg=1.397ms median=1.353ms p95=1.768ms min=1.053ms
breakdown_sql_pushdown:pending: avg=0.370ms median=0.362ms p95=0.549ms min=0.303ms
breakdown_raw_rows_rust_group:confirmed: avg=0.882ms median=0.864ms p95=0.968ms min=0.774ms
breakdown_sql_pushdown:confirmed: avg=0.330ms median=0.212ms p95=0.787ms min=0.151ms
breakdown_raw_rows_rust_group:returned: avg=0.906ms median=0.875ms p95=1.154ms min=0.782ms
breakdown_sql_pushdown:returned: avg=0.192ms median=0.179ms p95=0.268ms min=0.154ms
```

## Conclusion

SQL pushdown is the better path for `summary` and `status_breakdown`.

`pending` also improves, but less dramatically, because the endpoint still must
return the full matching pending record list. The correct rule remains:

- compute global counts/groups over the full matching dataset;
- use `LIMIT` only after the correct filter and order are applied for a paged
  response;
- rely on Accord custom fields such as `accord_flow_state` and
  `accord_customer_state` for Delivery Note state.
