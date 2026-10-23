# Worker queue windows

Workers can opt into a bounded queue with `queue_limit=5|10|20|50`, or select
all eligible orders with `queue_limit=0`, on both
`GET /v1/mobile/admin/production-maps/sequence?worker_scope=true` and the
`state_delta_v1` WebSocket protocol at `/v1/mobile/admin/production-maps/live`.
An omitted limit preserves the legacy response. Other limits are rejected.
The explicit all option retains worker permissions, queue ordering, search and
scope checks. It also removes the completed-work count limit for that actor.

The bound applies independently to each visible apparatus. Strict queues retain
their authoritative order and reserve room for an already active job. Free-pick
queues show active work, then previously worked/paused jobs ordered by the
machine's persisted last-work time, then pending work in queue order. Frozen and
completed work does not consume queue slots. Completing a job fills its slot
with the next eligible job in the same revisioned delta.

`queue_search` searches the apparatus's complete eligible queue before applying
the limit, including order code/number, product, title, customer and node titles.
Clients opt into ranks with `queue_positions=true` on both HTTP and WebSocket
requests. Only opted-in snapshots and streams include
`sequence_positions_by_apparatus`, a map from apparatus ID to returned order ID and its positive one-based position in that full eligible
display queue. Search and active-job window pinning preserve these positions;
free-pick positions use the shared display order. Only window rows carry this
metadata. Both `state_delta_v1` and `state_delta_v2` send changes through the
existing nested `scopes`/`upsert`/`remove` format, including rank-only changes
when the matching order list is unchanged. Cursor scope/epoch and replay checks
also apply to position updates.

Search requires a limit, is case-insensitive and is capped at 120 characters. QR
bootstrap and individual order controls continue to use authoritative point
reads, so an order outside the list can still be opened.

The window participates in the assignment scope fingerprint. Changing limit or
search at the same production revision requires a new HTTP view; the old scope
cannot produce a 304 or authorize a delta on the new view. Snapshot reads and
live updates use the same selection. Completed-work rows use the chosen limit;
completion request/decision notifications retain their existing bounded reads.

The window is a read projection over the existing cached production authority.
It is applied before copying and serializing maps and controls for the worker.
Internal queue validation, busy-machine checks and cross-stage controls retain
the complete context, including hidden orders. It is deliberately not a SQL
`LIMIT` over raw maps: that would change queue priority and production safety.
Cold authority builds still inspect the relevant queue; this change bounds
worker payloads and reduces per-device copying, serialization and UI work. No
production CPU or latency improvement is claimed without measurement.

Focused backend verification uses the real Router and a real local WebSocket
over isolated memory stores:

```sh
cargo test --locked --features verification --test worker_queue_window
```
