# Worker low bandwidth optimization

## Goal and correctness boundary

Make colour trial buttons and worker state exchange fast on weak ERP servers,
tablets and unreliable networks. PostgreSQL commits and existing server business
checks remain authoritative. No optimistic completion, automatic mutation replay,
production deployment, or production business writes are part of this work.

## Baseline

Read-only worker measurement before changes: the first WebSocket snapshot was
2,646,728 uncompressed bytes, including 285 maps and 14 apparatus control scopes.
The existing HTTP queue response compressed to about 83 KB. These are baseline
measurements, not measured post-change performance.

## Completed stages

1. Checkpoint commits: ERP `02c0ee9`, mobile `e5befea`.
2. Committed colour response carries optional authoritative current controls.
   Mobile releases the action immediately after its response and does not await
   an extra control GET. Missing/invalid controls disable actions and trigger a
   read-only refresh. ERP `f1f0ed4`, mobile `8c9e539e`.
3. Opt-in `state_delta_v1`: `state_ready`, revision-bound `state_delta` and
   `state_resync`. It covers maps, queue order, barrier fingerprints, states,
   action permissions, order controls/statuses and worker notifications together.
   Removed entries are explicit. Unchanged maps are neither serialized nor parsed.
   Workers bootstrap over compressed HTTP; WS never duplicates that full frame.
   A gap, broadcast lag or patch over 64 KiB triggers HTTP reconciliation.
   Legacy clients retain their previous protocol. Client retries use jitter and a
   30 second cap; socket outages do not start a second full-read retry loop.
   ERP `face566`, mobile `637e695f`.
4. Worker bootstrap and live snapshots contain assigned apparatus orders plus
   the worker's own completion history/decisions. Cross-machine states and
   permissions for those orders remain present. An opaque assignment scope
   accompanies the epoch/revision; a changed assignment requires a fresh view
   even at the same revision. Scope is checked again on live heartbeats.
   Large HTTP JSON decoding and typed model construction run in one isolate;
   large native WS frames are decoded in order with source backpressure.
   ERP `bf9baab`, mobile `85014235`.
5. Colour validation evaluates the assigned apparatus's controls with the same
   predicates and full cross-stage state. Session/progress queries are restricted
   to that queue's orders. Compiled programs are reused only when the complete
   current map definition is identical, including legacy code normalization.
   Changed formulas are recompiled. Exact consecutive colour-only notifications
   reuse completion history lists; coalesced or delayed events reread them.
   ERP `3861c8f`.

This stage deliberately uses the existing epoch/global snapshot revision. It does
not claim durable colour-event replay: a disconnected client or restarted server
reconciles with authoritative HTTP state. Existing durable sequence replay remains
available to the legacy sequence protocol.

6. Duplicate colour transitions return the original committed hold without
   rewriting actor/time or emitting a new invalidation. Wrong order/apparatus
   retries are rejected. On a lost colour response, mobile clears old controls
   and reconciles by GET; it never automatically replays the POST.
   ERP `8aa1c7f`, mobile `edd99395`.

7. WS outages start one conditional HTTP fallback. Authorization and the current
   assignment scope are checked before accepting an epoch/revision cursor. An
   unchanged view returns empty `304` with `no-store`, before snapshot compilation
   or history queries. The optional long poll is capped at 6 seconds, within the
   mobile 10 second read timeout. Reads remain single-flight, with a minimum 2
   second interval after success and jitter/backoff after failure. Healthy WS,
   backgrounding, disposal or fatal authorization/contract errors stop fallback.
   Worker HTTP views carry history and decisions atomically, avoiding two extra
   bootstrap requests and allowing notification recovery without WS.
   ERP `c650ed7`, mobile `3b29f020`.
8. State deltas are serialized once for both the 64 KiB limit and socket send.
   Colour widget checks cover both buttons, duplicate same-frame clicks, a valid
   control ACK with zero additional GETs, legacy controls and lost responses.

## Verification

Stage 1: `cargo check --locked` and focused Dart analysis passed.
Stage 3: four Rust patch tests and fifteen Flutter delta/regression tests passed;
focused Dart analysis passed. Stage 4: worker route/history projection and 23
Flutter parser/delta/API regression tests passed. Two core colour workflow tests
passed. A broader print-preflight test filter also selected
a PostgreSQL integration test; it failed at local database authentication before
its database work. No production database was used or deployed.
Stage 5: compiler reuse and exact colour-history revision guards passed. Worker
widget checks passed (34 cases); two admin order re-entry tests failed identically
on the pre-change mobile checkpoint `e5befea`, so they are existing failures.
Stage 6: three core colour tests and ten Flutter state/colour parser tests passed.
Stage 7: conditional route test covers empty 304, commit wakeup, assignment
changes at the same revision and invalid authentication. All 36 production-map
handler unit tests passed. Flutter API/delta checks passed (27 cases), worker
action/activity/recovery checks passed (31 cases). Focused analysis passed.
Stage 8: six colour button widget cases passed. A synthetic 285-map test confirms
unchanged maps/programs are absent from a queue/stage state patch: its full
fixture snapshot is 217,835 bytes and that two-field patch is 207 bytes. This
does not represent the complete real colour message or measured device latency.
Final worker/colour/sequence/recovery widget run passed 41 cases; together with
the 27 API/delta checks, 68 focused Flutter cases passed. `cargo check --locked`,
all six Rust state-patch tests and focused Dart analysis passed.

Bandwidth targets and real weak-network/tablet latency still require runtime
measurement after these changes are deployed to an authorized test environment.
