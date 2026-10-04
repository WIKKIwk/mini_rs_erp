# Order scan bootstrap

`GET /v1/mobile/admin/production-maps/order-scan-bootstrap` consolidates the
initial order-sheet control, raw-material start requirements and Qolip start
requirements into one read-only request. Existing readers and all mutation
routes keep their behavior.

## Query

- `apparatus`: required canonical apparatus ID, not a display label
- `order_id`: required production order ID
- `material_barcodes`: optional comma-separated scans, with the same meaning as
  the existing raw-material start-requirements reader

Training orders are not supported by this endpoint and return
`400 training_order_scan_bootstrap_unsupported`. They retain their existing
per-principal training flow.

## Response

A successful top-level response contains `ok: true`, `control_state` and
`sections`. `control_state` contains:

- `apparatus`, `order_id`
- `epoch`, `rev`: the canonical production control snapshot's cursor
- `scope`: an endpoint-specific authorization fingerprint
- `control`: the complete canonical action control, including stage occurrence
- `queue_state`, `stage_states`, `order_control`

`sections` always has `materials` and `qolips`. Each has exactly one state:

- `{"status":"not_required"}`: the current canonical interaction contract
  does not require that reader; the reader was not invoked
- `{"status":"ready","data":...}`: the complete existing reader JSON
- `{"status":"error","status_code":403,"error":{"error":"forbidden"}}`:
  the existing reader's status and JSON error, or an explicit bounded-body
  conversion error

Material loading is gated by `start_materials_mode == "scan_required"`.
Qolip loading is gated by `qolip_mode == "scan_required"`. Required sections
run concurrently after the canonical control has been loaded.

The Qolip section invokes the existing **POST** Qolip-validation handler with
an internally constructed, literal empty `qolip_code`. That handler's empty-code
return occurs before preparation or checkout mutations. A caller cannot supply
a nonempty Qolip code through this bootstrap. The material section invokes the
existing GET reader. Each reader retains its own authorization checks.

## Authorization and freshness

The top-level capability alternatives match the sequence endpoint:
`AdminAccess`, `ProductionMapManage`, `ApparatusQueueRead`, `RawMaterialAssign`,
`QolipManage`, or `PreparationAccess`. A non-admin must be assigned to the
requested apparatus. A canonical action control must exist for the requested
order at that apparatus; otherwise the endpoint returns
`404 order_not_available`.

Section capabilities are intentionally distinct. For example, an assigned
read-only queue worker may receive controls and materials while the Qolip
section preserves its existing `403 forbidden` result. Section failure does not
turn an authorized control read into a top-level failure.

After all required readers finish, the endpoint authenticates the session again
and checks the principal identity, current capabilities and apparatus
assignments. Revoked authorization returns `401` or `403` without sections.
A changed but still permitted authorization scope returns
`409 order_scan_bootstrap_changed`; the request is never silently re-scoped.

The production snapshot revision is checked again after these reads. A changed
revision causes one retry, for at most two aggregate attempts. Continued
revision mismatch returns `409 order_scan_bootstrap_changed` without sections.
A ten-second deadline bounds the entire read, including the snapshot accessor's
internal rebuild retries and slow section dependencies. Expiry returns
`503 order_scan_bootstrap_timeout` without sections. Each material/Qolip
response body is limited to 4 MiB during aggregation; exceeding that limit
produces an explicit section error instead of an oversized aggregate.

Every endpoint response has `Cache-Control: no-store`. The endpoint has no
aggregate response cache, conditional `304` path or mutation replay.

## Limits

This is an order-sheet bootstrap, not an authorization token for Start.

- The production revision does **not** provide a transactional inventory or
  Qolip snapshot and does not version all separate resource catalogs
- `scope` belongs to this endpoint and must not replace a whole-worker
  snapshot/delta cursor
- Start must continue sending the actual selected resources and using all
  existing authoritative validation, ownership and transaction checks
- WIP lookup, opening-WIP loading, printing and other mutation flows are
  unchanged
- A domain `404 order_not_available`, authorization denial or conflict is not
  evidence that the endpoint is absent on an older server

## Verification

The integration suite runs the real Axum router and production library over
isolated memory-backed services. It does not compile the legacy library's
`cfg(test)` suite:

```sh
cargo check --locked --lib --features verification
CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_DEV_CODEGEN_UNITS=256 \
  cargo test --locked --features verification --test order_scan_bootstrap -- --nocapture
```

Coverage includes exact legacy-reader payload parity, cold/warm canonical
controls, request and response-body counts, distinct reader capabilities,
admin/non-admin scope, no writes, deterministic parallel-reader gates, section
errors, ignored nonempty caller Qolip codes, real control changes during a read,
session/assignment/capability revocation, bounded revision retries and timeout.
These fixture measurements are not production latency measurements.

Generic authentication, method, query and no-store cases are also in
`tools/mini_erp_verifier/contracts.json`. The repository-wide verifier remains:

```sh
make verify
```
