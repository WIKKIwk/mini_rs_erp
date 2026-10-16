# Return an unused Resume to pause

Worker queue controls can contain an optional `re_pause` object with `resume_id`,
`worker_role`, and `worker_ref`. The owning worker can long press the order and
select **Qayta pauza qilish**. Existing `allowed_actions` values are unchanged so
older mobile clients continue accepting the queue contract.

`POST /v1/mobile/admin/production-maps/re-pause` accepts only:

```json
{"apparatus":"apparatus:default:bosma_7","order_id":"zakaz-example","resume_id":"queue-event-id"}
```

This is available only for a new, ordinary Resume by the existing session owner.
Resumes involving WIP claims, resource reacquisition, shift handoff, unfreezing,
requeueing, early closure, or training retain their existing stop workflows.
Already running sessions from before this feature have no checkpoint.

The checkpoint records the previous pause status and payload plus an exact
fingerprint of the resumed session. Migration `0146` removes checkpoints on
subsequent session, queue, material, report, route, or order-control mutations.
Checks use durable state rather than elapsed time or device-local flags.

The command locks the order, apparatus and session, verifies the owner and exact
Resume, restores `paused` or `roll_detached`, and pauses scheduling in one
transaction. Cutting output revisions remain monotonic. No roll, QR, stock
quantity, tooling checkout, or previous output is created or rewound. A zero
quantity `pause` audit event carries `event: "re_pause"` and its `resume_id`.

The same token can be retried safely: its durable receipt is acknowledged without
overwriting subsequent work. Mobile recovery refreshes state without replaying
an uncertain write. The client ignores completions from an old account or server
scope. After deployment, another ordinary Resume can be paused again.

Focused PostgreSQL regression checks require an explicit disposable database:

```sh
RE_PAUSE_TEST_DATABASE_URL=postgres://postgres@127.0.0.1:5432/postgres \
  cargo test --locked --features verification --test resume_re_pause -- --ignored --test-threads=1
```

The checks create a separate random database on that server, migrate it and
exercise real queue writes and HTTP authorization, including the runtime role.
