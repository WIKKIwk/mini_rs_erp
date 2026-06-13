# Go vs Rust LMDB v2 Stress Benchmark - 2026-05-15

## Scope

This run compares the legacy Go mobile backend with the Rust backend after the
LMDB v2 session-store hardening.

- Host: local loopback on the same machine
- Go service: `127.0.0.1:18222`
- Rust service: `127.0.0.1:18221`
- Go session store: persistent JSON
- Rust session store: LMDB v2
- Rust LMDB map size: `1024MB`
- Tool: ApacheBench (`ab`)
- Raw result root: `/tmp/accord_go_rust_lmdbv2_stress.QSwUjm`

The mutation-heavy ERPNext business endpoints were intentionally not stressed.
The login workload uses the built-in admin path, so it does not touch ERPNext
business tables. This isolates HTTP plus session creation pressure.

Request body:

```json
{"phone":"+998880000000","code":"19621978"}
```

## Preflight

Both services passed tests/builds before benchmark:

```text
go test ./...
cargo test --locked
cargo build --release --locked --bin accord_mobile_server_rs
go build -o /tmp/accord_go_core_bench ./cmd/core
```

Smoke checks returned `200` for both services:

- `GET /healthz`
- `POST /v1/mobile/auth/login`

## Healthz Stress

Sequential health-only stress used:

```text
ab -q -s 60 -n 20000 -c 500 /healthz
```

| Service | Requests | Concurrency | RPS | Median | p95 | p99 | Failed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Rust | 20000 | 500 | 4602.24 | 12ms | 18ms | 2680ms | 0 |
| Go | 20000 | 500 | 32170.74 | 14ms | 22ms | 24ms | 0 |

Go is much stronger on raw `healthz`. Rust had low median/p95 but a large tail
latency outlier in this run.

## Login Load

Login/session creation used fresh state directories before the login series.
Each service first received a warmup:

```text
ab -q -s 120 -n 500 -c 50 /v1/mobile/auth/login
```

Measured standard load:

```text
ab -q -s 180 -n 5000 -c 100 /v1/mobile/auth/login
```

| Service | Requests | Concurrency | RPS | Median | p95 | p99 | Failed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Rust LMDB v2 | 5000 | 100 | 8711.92 | 11ms | 13ms | 22ms | 0 |
| Go JSON | 5000 | 100 | 96.07 | 1043ms | 1803ms | 1839ms | 0 |

Rust was about `90.7x` faster on the standard login/session workload.

## Login Big/Stress

The big run was cumulative on top of the warmup and standard login runs.

```text
ab -q -s 180 -n 10000 -c 250 /v1/mobile/auth/login
```

| Service | Requests | Concurrency | Result | RPS | Median | p95 | p99 | Failed |
| --- | ---: | ---: | --- | ---: | ---: | ---: | ---: | ---: |
| Rust LMDB v2 | 10000 | 250 | completed | 8883.46 | 15ms | 19ms | 1044ms | 0 |
| Go JSON | 10000 | 250 | timed out at 300s | n/a | n/a | n/a | n/a | n/a |

Go did not complete the big run within the 300 second outer guard. The partial
ApacheBench output did not include a summary, but the Go session JSON file grew
to `14,626` records and `5.1MB`. After the timeout, the Go process was still
responding to health checks but continued running near `95%` CPU.

Rust was then pushed further:

```text
ab -q -s 240 -n 50000 -c 500 /v1/mobile/auth/login
```

| Service | Requests | Concurrency | RPS | Median | p95 | p99 | Failed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Rust LMDB v2 | 50000 | 500 | 9102.92 | 55ms | 60ms | 62ms | 0 |

Rust remained healthy after the `50000/500` stress run. The LMDB `data.mdb`
file was `20MB` after the cumulative login series.

## Readout

- Raw `healthz` throughput remains better in Go.
- Session creation is now heavily in Rust's favor because LMDB v2 avoids JSON
  rewrite amplification.
- Go's persistent JSON session store degrades sharply as session count grows.
- Rust LMDB v2 handled the largest login stress run with zero failed requests.
- No ERPNext business mutation endpoint was stressed in this run.
