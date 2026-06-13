# Final Go vs Rust Production Benchmark - 2026-05-15

This is the final consolidated benchmark run after the Rust LMDB migration, HTTP accept-loop FD-pressure fix, and production `LimitNOFILE=65535` deployment.

## Environment

- Host: `fedora` mini PC via `bore.pub`
- Production service: `mobile-server-core.service` on `127.0.0.1:8081`
- Rust binary checksum: `9cbbdb275c6a74b740a1bb4c1eb1f1f50536748ed68a7c39c289044b702302f8`
- Go binary checksum: `fb49eb7fa7f241e77303e8031b7072ca7963a6585e20fb0a6cfc3b62107f7778`
- Result root on server: `/tmp/accord_final_bench_20260515_151556`
- Production health after benchmark: `{"ok":true}`
- Production restarts after deployment: `NRestarts=0`
- Production open-file limit: `LimitNOFILE=65535`

## Method

- Normal mode: service runs normally on local staging ports.
- Throttled mode: service is started under CPU throttling/pinning from the benchmark script.
- LMDB backends are used for Rust staging state with JSON fallback disabled.
- ApacheBench cases:
  - `health_20k_500`: `20,000` health requests at concurrency `500`
  - `admin_login_5k_100`: `5,000` admin login writes at concurrency `100`
  - `werka_login_local_5k_100`: `5,000` werka login attempts at concurrency `100`
  - `push_fixed_5k_100`: `5,000` push token writes at concurrency `100`
  - `read_summary_3k_100`: `3,000` summary reads at concurrency `100`
  - `read_home_3k_100`: `3,000` home reads at concurrency `100`
  - `crash_health_60k_1000`: `60,000` health requests at concurrency `1,000`
  - `crash_push_20k_500`: `20,000` push writes at concurrency `500`

## Results

| Service | Mode | Case | RPS | Failed | Median ms | P95 ms | P99 ms | CPU avg | CPU max | RSS max KB | Status |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| Rust | normal | health_20k_500 | 5727.08 | 0 | 82 | 106 | 114 | 49.2 | 55.8 | 27708 | ok |
| Rust | normal | admin_login_5k_100 | 5313.61 | 0 | 17 | 28 | 35 | 59.1 | 63.9 | 27792 | ok |
| Rust | normal | werka_login_local_5k_100 | 773.55 | 0 | 113 | 194 | 206 | 62.0 | 63.0 | 31416 | ok |
| Rust | normal | push_fixed_5k_100 | 5847.71 | 0 | 16 | 21 | 25 | 61.6 | 62.2 | 31588 | ok |
| Rust | normal | read_summary_3k_100 | 2488.98 | 0 | 37 | 55 | 60 | 62.3 | 63.0 | 31636 | ok |
| Rust | normal | read_home_3k_100 | 735.85 | 0 | 121 | 194 | 206 | 61.7 | 62.7 | 31712 | ok |
| Rust | normal | crash_health_60k_1000 | 5819.38 | 0 | 166 | 195 | 209 | 60.5 | 60.7 | 48060 | ok |
| Rust | normal | crash_push_20k_500 | 5152.66 | 0 | 90 | 141 | 163 | 60.7 | 61.1 | 40340 | ok |
| Go | normal | health_20k_500 | 5543.64 | 0 | 85 | 112 | 125 | 64.6 | 82.1 | 31708 | ok |
| Go | normal | admin_login_5k_100 | 36.52 | 0 | 2684 | 5106 | 5378 | 110.4 | 112.0 | 45148 | ok |
| Go | normal | werka_login_local_5k_100 | 0 | NA | NA | NA | NA | 111.0 | 111.0 | 62244 | fail |
| Go | normal | push_fixed_5k_100 | 500.24 | 0 | 25 | 44 | 60 | 111.0 | 111.0 | 57908 | ok |
| Go | normal | read_summary_3k_100 | 2712.03 | 0 | 32 | 73 | 103 | 111.0 | 111.0 | 33356 | ok |
| Go | normal | read_home_3k_100 | 662.19 | 0 | 126 | 340 | 508 | 111.0 | 111.0 | 36268 | ok |
| Go | normal | crash_health_60k_1000 | 5515.64 | 0 | 176 | 207 | 221 | 111.0 | 111.0 | 54356 | ok |
| Go | normal | crash_push_20k_500 | 3726.97 | 0 | 130 | 199 | 228 | 111.0 | 111.0 | 51436 | ok |
| Rust | throttled | health_20k_500 | 6057.89 | 0 | 80 | 87 | 96 | 54.3 | 61.3 | 16536 | ok |
| Rust | throttled | admin_login_5k_100 | 6119.76 | 0 | 16 | 18 | 18 | 61.7 | 63.6 | 18068 | ok |
| Rust | throttled | werka_login_local_5k_100 | 648.41 | 0 | 152 | 190 | 210 | 56.0 | 63.4 | 23248 | ok |
| Rust | throttled | push_fixed_5k_100 | 5881.02 | 0 | 17 | 20 | 23 | 52.1 | 52.6 | 23404 | ok |
| Rust | throttled | read_summary_3k_100 | 2173.52 | 0 | 47 | 59 | 63 | 52.9 | 53.2 | 23404 | ok |
| Rust | throttled | read_home_3k_100 | 652.67 | 0 | 150 | 199 | 218 | 51.4 | 52.9 | 23404 | ok |
| Go | throttled | health_20k_500 | 6042.71 | 0 | 81 | 95 | 124 | 49.6 | 63.3 | 25836 | ok |
| Go | throttled | admin_login_5k_100 | 34.80 | 0 | 2980 | 5146 | 5335 | 94.7 | 97.7 | 45608 | ok |
| Go | throttled | werka_login_local_5k_100 | 0 | NA | NA | NA | NA | 98.0 | 98.2 | 47712 | fail |
| Go | throttled | push_fixed_5k_100 | 489.51 | 0 | 36 | 41 | 43 | 98.2 | 98.2 | 56872 | ok |
| Go | throttled | read_summary_3k_100 | 2601.87 | 0 | 33 | 81 | 109 | 98.1 | 98.2 | 42288 | ok |
| Go | throttled | read_home_3k_100 | 680.04 | 0 | 124 | 329 | 436 | 97.9 | 98.1 | 42280 | ok |

## Takeaways

- Rust is production-stable under the crash/stress cases after the FD-pressure fix.
- Rust is dramatically faster for admin login and push-token writes.
- Rust uses less CPU and lower peak memory on most write and mixed workloads.
- Health throughput is effectively tied: Rust slightly wins normal crash health, Go slightly edges simple throttled health by a negligible margin.
- Go remains slightly ahead on `read_summary` in this run, while Rust is slightly ahead on `read_home` in normal mode.
- Go's werka local login stress case times out/fails in both normal and throttled modes in this benchmark, while Rust completes it successfully.

## Verdict

Rust is ready as the primary production service for this workload. The previous FD-pressure crash is resolved, LMDB-backed write paths are stable, production health survived the benchmark, and the Rust service now beats or matches Go in the important production-facing stress paths.
