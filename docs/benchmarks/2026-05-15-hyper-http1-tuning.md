# Hyper HTTP/1 Tuning Benchmark - 2026-05-15

## Scope

This run checks the Rust service after replacing `axum::serve` with an explicit
Hyper HTTP/1 accept loop.

The change keeps the existing Axum router and business handlers intact. It only
changes how accepted TCP streams are handed to Hyper:

- custom `socket2` bind/reuseport/backlog remains in place;
- `TCP_NODELAY` is still enabled per accepted stream;
- Axum `Router` is bridged to Hyper through `TowerToHyperService`;
- HTTP/1 keep-alive remains enabled.

Runtime state used isolated temporary LMDB directories under:

```text
/tmp/accord_hyper_tuning.0pu5AK
```

Rust service:

```text
127.0.0.1:18331
```

## Verification

Before runtime benchmark:

```text
cargo check --locked
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked --release
git diff --check
```

Result:

```text
330 tests passed
0 failed
```

Smoke checks returned HTTP `200`:

- `GET /healthz`
- `POST /v1/mobile/auth/login` for Werka
- `POST /v1/mobile/push/token` with the Werka token

## Healthz No Keep-Alive

Command:

```text
ab -q -s 60 -n 20000 -c 500 /healthz
```

| Run | RPS | Median | p95 | p99 | Longest | Failed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 32769.80 | 13ms | 29ms | 58ms | 75ms | 0 |
| 2 | 37112.64 | 13ms | 16ms | 33ms | 36ms | 0 |
| 3 | 35994.79 | 13ms | 21ms | 25ms | 26ms | 0 |

Compared with the previous post-listener-tuning Rust range of roughly
`21k-24k` RPS, the manual Hyper HTTP/1 loop raises no-keep-alive health checks
to roughly `33k-37k` RPS and removes the multi-second tail outliers seen in
earlier runs.

## Healthz Keep-Alive

Command:

```text
ab -q -k -s 60 -n 50000 -c 500 /healthz
```

| RPS | Median | p95 | p99 | Longest | Failed |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 207407.78 | 2ms | 3ms | 8ms | 15ms | 0 |

## Push Token No Keep-Alive

Command:

```text
ab -q -s 180 -n 5000 -c 100 /v1/mobile/push/token
```

| Run | RPS | Median | p95 | p99 | Longest | Failed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 40537.36 | 2ms | 4ms | 5ms | 6ms | 0 |
| 2 | 39192.02 | 2ms | 5ms | 7ms | 7ms | 0 |
| 3 | 45222.68 | 2ms | 3ms | 4ms | 5ms | 0 |

The previous post-push-token-tuning Rust range was roughly `10k-12k` RPS for
this fixed-token no-keep-alive path. This run reaches roughly `39k-45k` RPS.

## Push Token Stress

No keep-alive:

```text
ab -q -s 180 -n 20000 -c 500 /v1/mobile/push/token
```

| RPS | Median | p95 | p99 | Longest | Failed |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 36917.81 | 13ms | 17ms | 20ms | 24ms | 0 |

Keep-alive:

```text
ab -q -k -s 180 -n 20000 -c 500 /v1/mobile/push/token
```

| RPS | Median | p95 | p99 | Longest | Failed |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 69484.98 | 6ms | 11ms | 13ms | 15ms | 0 |

## Login Sanity

Command:

```text
ab -q -s 180 -n 5000 -c 100 /v1/mobile/auth/login
```

| RPS | Median | p95 | p99 | Longest | Failed |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 26396.37 | 4ms | 5ms | 6ms | 8ms | 0 |

## Readout

- Business logic stayed untouched: the Axum router and handlers are reused.
- The previous weak no-keep-alive paths improved sharply.
- Rust now matches or beats the last recorded Go numbers for the two weak paths:
  - healthz no keep-alive: Rust `33k-37k` RPS vs previous Go `35.5k` RPS;
  - push/token fixed write: Rust `39k-45k` RPS vs previous Go `24.5k` RPS.
- All measured runs returned zero failed requests.
