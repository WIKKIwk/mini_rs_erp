# Cancel an accidental WIP Start

Workers on cutting, adhesive/extrusion lamination and cold-glue machines can
undo an untouched Start. Printing and flexographic printing are excluded.
The mobile long-press menu calls this **Boshlashni bekor qilish** and asks for
confirmation before sending the request.

The additive `cancel_start` queue control contains `session_id`, `token`,
`worker_ref` and `previous_state`. It does not add a new `allowed_actions`
value, so existing clients keep their normal strict action decoder.

`POST /v1/mobile/admin/production-maps/cancel-start` accepts only:

```json
{"apparatus":"apparatus:default:asset-010","order_id":"order-id","session_id":"session-id","token":"64-hex-checkpoint-token"}
```

The authenticated worker must own the Start and still have queue management
access to that machine. The transaction locks the order, apparatus, session,
input WIP, affected stock and schedule rows. It compares the complete stored
Start checkpoint and rejects later work, output, merges, stock corrections,
changed material assignments, route edits and frozen orders. A checkpoint is
not permission to override any changed state.

On success, the original WIP identity, QR, quantity, recipe/lineage and location
are retained. Source usage fields and fresh raw-material claims return to their
previous state; the queue and schedule return to their previous state. Only the
unused execution session is removed. Start and cancellation events remain in
the audit log, together with zero-quantity material return events. WIP revisions
remain monotonic. A retry acknowledges the original receipt without changing
any later Start.

Apply migration `0179_cancel_start_material_usage` before deploying the backend;
it adds the constrained, zero-quantity `usage_cancelled` audit event. Checkpoints
are saved atomically in session JSONB for new eligible Starts. Existing sessions without a trustworthy checkpoint
do not expose this action. Test-mode/in-memory stores do not advertise it.

Verification uses an explicitly supplied disposable PostgreSQL server:

```sh
CANCEL_START_TEST_DATABASE_URL=postgres://.../postgres \
  cargo test --locked --features verification --test cancel_start -- --ignored --test-threads=1
```
