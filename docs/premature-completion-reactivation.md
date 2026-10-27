# Premature completion reactivation

Workers can correct an accidental Complete through a dedicated API. Ordinary
`allowed_actions` are unchanged so older mobile clients retain their existing
strict action parser.

- `POST /v1/mobile/admin/production-maps/reactivation/preview` accepts
  `{apparatus, order_id, batch_id?}`. Bosma uses the order; lamination/cutting
  require an exact produced batch from the final completion boundary.
- The response is `{available, denial_reason?, target?}`. An available target
  contains `{apparatus, order_id, batch_id, session_id, stage_node_id, token}`.
- `POST /v1/mobile/admin/production-maps/reactivation` accepts the exact target.
  The token is also the idempotency key; an authenticated matching receipt
  acknowledges a retry without modifying any subsequent work.

Both endpoints require apparatus queue capability, worker role and apparatus
assignment. Another assigned worker may reopen the completed work; the original
worker's identity and completion report remain immutable. Worker completion
history is scoped to assigned apparatuses, including work from previous shifts.
Preview is advisory:
execution revalidates the whole proposal under order/apparatus, package, batch,
material, catalog and tooling locks. The token includes the exact graph,
configuration, source session, affected batches, resources and queue state.

A new linked continuation session is created **paused**, never running. Resume
is a separate action. Before Resume, the reopening worker can use Close again
without recording new production. The old completed session,
its report and actual output measurements remain immutable. Existing quantities
are never produced or counted again by reactivation; subsequent reports contain
**additional production only**. Current source input is restored only when its
processed owner is the exact completed source session. Lamination's previous
outputs remain with their downstream consumers and do not block reopening.
The selected output identifies its completed execution; it is never reclaimed.
If the input was physically returned, reported exhausted, or lacks positive
remainder evidence, the continuation
requires a valid remainder or new input QR through normal Resume intake. A
pre-completion snapshot must not restore a returned parent roll. Released or
exhausted lamination materials require normal material intake as well.
Cutting's selected completion outputs and affected siblings are still reserved
to the continuation and unavailable downstream. Every session writer releases
these cutting reservations when
work stops, its source changes, a shift closes, or a new output cycle starts.

Raw material consumption is compensated with the existing immutable
`consumption_reversed` ledger event, matching stock identity, quantity,
assignment, order, apparatus and original session time. Unused released
materials are not reclaimed. Physical Bosma tooling uses the standard Start
preparation and checkout transaction; availability and product composition are
checked again before commit.

Worker reactivation is denied after the actual final physical operation is
closed, regardless of the target apparatus. A completed individual final-stage
WIP remains eligible while the operation has outstanding work. Closed,
cancelled, early-closed or frozen orders remain blocked. An incorrectly derived
`production_completed` projection may be explicitly corrected only while the
actual final operation is still open; its lifecycle correction has its own
ledger entry.

Stable denial codes: `apparatus_busy`, `source_not_free`, `final_stage_closed`,
`stale_target`, `resource_unavailable`, `order_closed`, `unsupported_history`.
Execution returns HTTP409 `reactivation_<code>` if a condition changes after
preview. Unsafe legacy history (ambiguous repeated print occurrence, completion
without an exact event witness, incomplete merged-cycle provenance) is rejected
rather than guessed. Newly completed sessions retain the pre-completion
lineage snapshot for safe continuation of merged cutting runs.

No schema change is required; existing queue/progress receipts and material
reversal constraints are used. No production database is accessed by development
verification.

Opening WIP inputs also require a confirmed intake whose current route resolves
to the exact producing occurrence. The locked intake snapshot participates in
the preview token; cancelled or remapped intakes cannot be restored.

## Focused verification

Use an explicitly disposable PostgreSQL server with the `mini_rs_erp` runtime
login role already provisioned. The suite creates random isolated databases;
it never falls back to an application database or reads `.env`.

```sh
REACTIVATION_TEST_DATABASE_URL=postgres://postgres@127.0.0.1:56319/postgres \
RE_PAUSE_TEST_DATABASE_URL=postgres://postgres@127.0.0.1:56319/postgres \
cargo test --locked --features verification \
  --test work_reactivation --test resume_re_pause -- --ignored --test-threads=1
```

Coverage includes print/lamination/cutting continuation, independent runtime
services racing the same receipt, immutable output quantities, downstream use,
stale revisions, final closure, worker/assignment ownership, reservation release
on freeze and cutting boundaries, incremental completion and another reopening,
opening intake validity, and compensating material consumption accounting.
Lamination regressions also cover already-used downstream outputs, shared
apparatus history, another assigned worker's continuation, returned or exhausted
inputs, missing remainder evidence, and immutable residual material accounting.
