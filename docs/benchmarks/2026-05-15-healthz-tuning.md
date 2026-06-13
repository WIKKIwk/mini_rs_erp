# Healthz Tuning Benchmark - 2026-05-15

## Scope

This follow-up tunes the Rust `/healthz` endpoint after the Go vs Rust stress
run showed good median/p95 latency but unstable tail latency on Rust health
checks.

Changes:

- `/healthz` no longer extracts `AppState`.
- `/healthz` returns a static JSON body: `{"ok":true}`.
- `/healthz` is routed outside the API `TraceLayer`.
- Incoming TCP streams enable `TCP_NODELAY`.

The response contract stayed the same:

```text
HTTP 200
content-type: application/json
{"ok":true}
```

Raw result root:

```text
/tmp/accord_healthz_tuned.AloIIl
```

## Baseline From Previous Stress

Previous Rust health-only run:

```text
ab -q -s 60 -n 20000 -c 500 /healthz
```

| Run | RPS | Median | p95 | p99 | Longest | Failed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Baseline | 4602.24 | 12ms | 18ms | 2680ms | 4342ms | 0 |

## Tuned No Keep-Alive

Same benchmark command, repeated three times:

```text
ab -q -s 60 -n 20000 -c 500 /healthz
```

| Run | RPS | Median | p95 | p99 | Longest | Failed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Tuned 1 | 7418.82 | 13ms | 17ms | 28ms | 2694ms | 0 |
| Tuned 2 | 7306.74 | 12ms | 17ms | 37ms | 2710ms | 0 |
| Tuned 3 | 7247.31 | 13ms | 17ms | 49ms | 2726ms | 0 |

The p99 tail became much more stable, dropping from the previous `2680ms`
outlier to `28-49ms` across repeated no-keep-alive runs.

## Tuned Keep-Alive

Keep-alive benchmark:

```text
ab -q -k -s 60 -n 20000 -c 500 /healthz
```

| Run | RPS | Median | p95 | p99 | Longest | Failed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Tuned keep-alive | 173097.23 | 1ms | 1ms | 1ms | 23ms | 0 |

## Readout

- Healthz response logic did not change.
- The hot path is now independent of application state and API tracing.
- No-keep-alive p99 stabilized substantially.
- No-keep-alive still shows longest-request outliers around `2.7s`; this looks
  tied to high connection churn rather than handler work.
- Keep-alive health checks are extremely stable and fast.
