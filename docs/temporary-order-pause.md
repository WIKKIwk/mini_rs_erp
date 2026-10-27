# Temporarily remove an unfinished order

An assigned production worker can long press running work and select
**Buyurtmani yechib turish**. This frees the apparatus while preserving the
unfinished physical roll. It requires no measured kilograms, meters, or printer.
It creates no output batch, QR, stock movement, or completed production quantity.
The existing measured roll-removal operation keeps its output-recording behavior.

The original work session, input WIP ownership, material/tooling reservations,
physical output start, and open cutting-frame records remain attached to the
order. Already recorded outputs remain unchanged. **Davom ettirish** resumes this
same unfinished output cycle once the apparatus and its queue priority allow it.
The eventual roll is measured and recorded once. The physical start/end span
includes the interruption; pause/resume times are separately recorded in audit events.

## Queue discipline

The apparatus's existing canonical queue discipline controls the next selection:

- `strict_sequence`: both Start and Resume follow the current saved sequence.
  The first unfinished order has priority, including paused work. Moving a paused
  order below pending work enables the new first order and prevents that paused
  order from resuming early. Completed and frozen rows do not take priority;
  running work and colour trials retain their apparatus occupancy protections.
- `free_pick`: any otherwise eligible order on the apparatus may start. This
  applies to configured printing apparatus as well as other operations.

Neither choice needs an urgent-order flag. Assignment, material, WIP, tooling,
order control, and apparatus occupancy checks still apply. A search result or
page boundary never defines queue priority. An idle apparatus resumes paused
work when its current sequence position is eligible, or when its policy is
`free_pick`.

Paused orders can be rearranged in the saved sequence without changing their
sessions or reserved resources. Running work, colour trials and frozen orders
retain their existing reorder protections. Reordering a paused order changes
only its priority: moving it to the end enables earlier unfinished work while
preserving that order's physical roll for its later turn.

## Additive mobile contract

Eligible queue controls expose `temporary_pause: {session_id, token}`. Paused
controls expose `temporarily_paused: true`. The existing `allowed_actions` enum
does not gain an unknown value, preserving compatibility with older clients.
Compact worker rows optionally expose `temporarily_paused_apparatus`, a list of
canonical apparatus IDs, so the temporary-pause label survives cache restores
and list refreshes. Empty lists are omitted. This is display metadata only;
fresh full controls remain the authority for Start, Resume and other actions.

`POST /v1/mobile/admin/production-maps/temporary-pause` accepts only:

```json
{
  "apparatus": "apparatus:default:bosma_7",
  "order_id": "zakaz-example",
  "session_id": "run-session-id",
  "token": "64-character-token-from-the-current-control"
}
```

The endpoint requires the worker role, queue-management capability and apparatus
assignment. The current session and order state are checked under the store's
transaction boundary. A durable receipt acknowledges an exact retry without
rewinding later work. A later suspension uses a new token even if both actions
occur in the same second. Resume also compares the expected held session and
input ownership at commit, protecting changes made after preparation.

The mobile client refreshes authoritative state after a failed or uncertain
response. It never falls back to measured roll removal or automatically replays
an uncertain suspension. Responses from an old account/server scope are ignored.

## Response latency

The successful response acknowledges the committed pause without building a
presentation snapshot. The mobile card shows the acknowledged paused state
immediately; a newer live snapshot always wins. Fresh Resume/Start permissions
still come from the canonical background refresh.

Pausing holds the apparatus mutation guard, allowing independent machines to
continue concurrently. Broad map/freeze mutations and the store's order/input
locks still exclude conflicting work. Successful pauses log resolution, guard
wait, store and total durations without logging the session token.

Snapshot reads retain only the latest state and latest freeze for each stage;
the audit history remains complete. Rezka frame projections reuse the existing
immutable map topology cache instead of traversing the route for every control.
The same cache is shared across visibility, sequence, control and presentation
phases. A worker detail request builds the requested order while retaining its
queue/WIP dependencies, rather than first building the worker's entire list.
Fresh scoped detail can enable the next order while the list refresh is pending.

Measure operator latency with an optimized backend build (`cargo build --release
--bin mini_rs_erp`); the Mac Keychain launcher's default executable uses that
build. The local Flutter preview routes its configured HTTPS test account to
the same core on `127.0.0.1:8081` through development HTTP/WebSocket proxies.
This avoids sending local requests through the public tunnel. The canonical
account URL and production transport checks stay unchanged. If starting a core
with another port, align these development proxy targets with that listener.

## Verification

Memory-store and disposable-PostgreSQL regressions are in
`tests/temporary_pause.rs` and `tests/temporary_pause_postgres.rs`. The PostgreSQL
suite requires an explicit `TEMPORARY_PAUSE_TEST_DATABASE_URL`, creates random
test databases, and never reads `.env` or falls back to an application database.
Its HTTP case changes the test process's working directory temporarily, so run
the PostgreSQL test binary with `--test-threads=1`.
