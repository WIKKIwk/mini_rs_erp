# Go vs Rust LMDB Default Battle - 2026-05-15

## Scope

This run compares the legacy Go mobile backend with the Rust backend after Rust
made LMDB the production default for local persistent state.

- Host: local loopback on the same machine
- Rust service: `127.0.0.1:18231`
- Go service: `127.0.0.1:18232`
- Go local state: JSON session/profile/push state
- Rust local state: LMDB session/profile/push/admin state
- Rust JSON fallback: disabled with `MOBILE_API_LOCAL_STORE_ALLOW_JSON_FALLBACK=0`
- Rust session LMDB map size: `1024MB`
- Rust profile/push/admin LMDB map size: `128MB` each
- Tool: ApacheBench (`ab`)
- Raw result root: `/tmp/accord_go_rust_lmdb_default_battle.KRrNDj`

The mutation-heavy ERPNext business endpoints were intentionally not stressed.
Admin and Werka login paths do not require ERPNext, so this isolates HTTP,
auth/session behavior, and local-state persistence.

Admin login body:

```json
{"phone":"+998880000000","code":"19621978"}
```

Werka login body:

```json
{"phone":"+99888862440","code":"20WERKA0001"}
```

## Preflight

Both services passed tests/builds before benchmark:

```text
cargo test --locked
go test ./...
cargo build --release --locked --bin accord_mobile_server_rs
go build -o /tmp/accord_go_rust_lmdb_default_battle.KRrNDj/bin/go_core ./cmd/core
```

Smoke checks returned success for both services:

- `GET /healthz`
- `POST /v1/mobile/auth/login` for Admin
- `POST /v1/mobile/auth/login` for Werka
- `PUT /v1/mobile/profile` with Werka token
- `POST /v1/mobile/push/token` with Werka token

Both services were still healthy after all stress runs.

## Healthz Stress

No keep-alive:

```text
ab -q -s 60 -n 20000 -c 500 /healthz
```

| Service | Requests | Concurrency | RPS | Median | p95 | p99 | Longest | Failed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Rust LMDB default | 20000 | 500 | 7335.24 | 12ms | 16ms | 1882ms | 2721ms | 0 |
| Go JSON | 20000 | 500 | 35509.66 | 13ms | 20ms | 25ms | 26ms | 0 |

Keep-alive:

```text
ab -q -k -s 60 -n 50000 -c 500 /healthz
```

| Service | Requests | Concurrency | RPS | Median | p95 | p99 | Longest | Failed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Rust LMDB default | 50000 | 500 | 230263.01 | 1ms | 2ms | 3ms | 32ms | 0 |
| Go JSON | 50000 | 500 | 149389.74 | 3ms | 4ms | 8ms | 14ms | 0 |

Readout:

- Go is still stronger for no-keep-alive raw connection churn.
- Rust is stronger with keep-alive, which is the more realistic production
  health-check behavior behind pooled clients/load balancers.
- Rust no-keep-alive still shows a long-tail outlier under high connection
  churn, but the handler itself stays healthy and error-free.

## Login Session Creation

Warmup:

```text
ab -q -s 120 -n 500 -c 50 /v1/mobile/auth/login
```

| Service | Requests | Concurrency | RPS | Median | p95 | p99 | Failed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Rust LMDB default | 500 | 50 | 8910.11 | 6ms | 7ms | 7ms | 0 |
| Go JSON | 500 | 50 | 1170.11 | 39ms | 74ms | 76ms | 0 |

Standard load:

```text
ab -q -s 180 -n 5000 -c 100 /v1/mobile/auth/login
```

| Service | Requests | Concurrency | RPS | Median | p95 | p99 | Failed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Rust LMDB default | 5000 | 100 | 9043.62 | 11ms | 13ms | 15ms | 0 |
| Go JSON | 5000 | 100 | 101.93 | 937ms | 1682ms | 1759ms | 0 |

Rust was about `88.7x` faster than Go on the standard login/session workload.

Big load:

```text
ab -q -s 240 -n 20000 -c 250 /v1/mobile/auth/login
```

| Service | Requests | Concurrency | Result | RPS | Median | p95 | p99 | Failed |
| --- | ---: | ---: | --- | ---: | ---: | ---: | ---: | ---: |
| Rust LMDB default | 20000 | 250 | completed | 9121.77 | 27ms | 30ms | 33ms | 0 |
| Go JSON | 20000 | 250 | timed out at 420s | n/a | n/a | n/a | n/a | n/a |

Rust deep stress:

```text
ab -q -s 300 -n 50000 -c 500 /v1/mobile/auth/login
```

| Service | Requests | Concurrency | RPS | Median | p95 | p99 | Longest | Failed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Rust LMDB default | 50000 | 500 | 7214.96 | 67ms | 77ms | 84ms | 1309ms | 0 |

After the timed-out Go big run, the Go session JSON file contained `17062`
sessions and was `6.1MB`. Rust's LMDB `data.mdb` for sessions was `23.8MB`
after the full cumulative login series.

## Profile Update Local-State Write

This uses a Werka token and repeatedly updates one profile nickname:

```text
ab -q -s 180 -n 5000 -c 100 -m PUT /v1/mobile/profile
```

| Service | Requests | Concurrency | RPS | Median | p95 | p99 | Longest | Failed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Rust LMDB default | 5000 | 100 | 7248.39 | 14ms | 16ms | 18ms | 18ms | 0 |
| Go JSON | 5000 | 100 | 15.52 | 6036ms | 7863ms | 8078ms | 8576ms | 0 |

Rust was about `467.0x` faster here. The important detail is that profile
update also updates the active session. Once Go's session JSON file had grown
from the login stress, even a small profile mutation inherited the large JSON
rewrite cost.

## Push Token Local-State Write

This uses a Werka token and repeatedly registers one fixed device token:

```text
ab -q -s 180 -n 5000 -c 100 /v1/mobile/push/token
```

| Service | Requests | Concurrency | RPS | Median | p95 | p99 | Longest | Failed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Rust LMDB default | 5000 | 100 | 9089.27 | 11ms | 12ms | 14ms | 15ms | 0 |
| Go JSON | 5000 | 100 | 24561.94 | 4ms | 6ms | 7ms | 11ms | 0 |

Go was about `2.7x` faster for this tiny fixed-token path. This endpoint only
does session authorization plus a very small push-token state write, so it does
not show the same JSON rewrite amplification as login/profile session writes.

## Readout

- Rust LMDB default is production-ready for the local-state hot path tested
  here: session creation, session update, profile preferences, and push token
  writes all returned zero failed requests.
- Rust decisively wins the session-heavy workloads:
  - `88.7x` faster on `5000/100` login.
  - Completed `20000/250` login in `2.19s`; Go timed out at `420s`.
  - Completed an additional `50000/500` login stress with zero failures.
- Rust profile update stayed stable after the session table grew; Go profile
  update became extremely slow because it still rewrites the large session JSON.
- Go remains faster on no-keep-alive health checks and this small fixed-token
  push write path.
- The next performance frontier for Rust is not correctness or persistence
  safety anymore; it is shaving raw HTTP connection churn tail latency and
  reducing LMDB transaction overhead on tiny single-key writes.
