# Chat and live-stream recovery — backend deployment

## Scope and production boundary

Implemented and cross-built on macOS using Wikki's command:
`RUSTC_WRAPPER=sccache cargo zigbuild --release --target x86_64-unknown-linux-gnu`.
The backend was deployed to `debian-wyse-server` on 2026-09-11 UTC. The service
`mini-rs-erp.service` uses `/home/user/mini_rs_erp_deploy/src/mini_rs_erp` and the
database `mini_rs_erp_dev`. No compilation ran on Debian. Mobile build/deployment
and firewall/Iroh tuning remain deferred.

## Confirmed causes

- The inspected Debian database had zero rows in `mini_chat_event_clock`.
  Chat writers required an existing singleton; message transactions rolled back
  with `StoreFailed`. The missing-row failure was reproduced on an isolated
  local PostgreSQL database before applying the code fix.
- 43 undelivered freeze events addressed their requester as their recipient.
  Direct messages correctly reject self-DMs; retrying could never resolve this.
  Another 20 freeze events failed during persistence. Chat delivery was already
  enabled; adding an activation flag would not have repaired either condition.
- Failed freeze deliveries were retried every two seconds, including events
  with hundreds of thousands of attempts.
- Production-map live sockets sent snapshots and Ping frames but never read
  incoming Pong, Ping, or Close frames. Warehouse live sockets had the same
  reader omission and no heartbeat.
- Debian's firewall blocked observed LAN packets to the Iroh agent's UDP port.
  Iroh also logged relay timeouts and lost streams. These server settings remain
  unchanged; the local client now defaults to the existing HTTPS/WSS route.

## Changes

- Migration `0105_chat_delivery_recovery` restores the chat clock from durable
  event cursors without rewinding a surviving clock. Its legacy-writer trigger
  can recover a missing singleton too.
- All current chat-card/message writers allocate cursors using an atomic,
  transactional upsert. The singleton lock preserves commit ordering; recovery
  rolls back when its message transaction rolls back.
- Freeze self-notifications are retained with `skipped_at` and the explicit
  reason `self_notification_not_required`; they are not falsely marked delivered
  and do not create invented recipients. Order state and audit rows are retained.
- Retry delays grow from 2 seconds to a 900-second cap. Valid deliveries are
  not abandoned because of their historical attempt counts. Retry-marker
  failures are logged rather than silently ignored.
- Freeze/inventory cards lock their conversation before checking their existing
  message. Concurrent creation is idempotent and late events cannot overwrite a
  newer card status.
- Iroh is opt-in using `--dart-define=IROH_AUTO_CONNECT=true`. The default false
  gate covers HTTP requests, warm-up, and live subscriptions, including clients
  with cached tickets. Mutation requests are not replayed across transports.
- Native Flutter live clients use a bounded 10-second upgrade, 25-second
  transport Ping/Pong, prompt cancellation, and late-handshake cleanup. Quiet
  queues are not treated as disconnected just because no JSON snapshot changes.
  Monitor application-level ping/pong is retained.
- Backend production-map and warehouse sockets continuously read control
  frames. Warehouse writes now have bounded send time and a heartbeat too.
  Existing queue authority, permissions, and reconnect/fail-closed UI rules are
  unchanged.

## Local verification

All PostgreSQL checks used throwaway test databases on the Mac, with the
`mini_rs_erp` runtime login for the new persistence/claim checks.

```sh
MINI_ERP_TEST_ADMIN_DATABASE_URL=postgres://superuser@127.0.0.1:5432/postgres cargo test --locked --test chat_reliability -- --nocapture
MINI_ERP_TEST_ADMIN_DATABASE_URL=postgres://superuser@127.0.0.1:5432/postgres cargo test --locked --lib postgres_production_map_store_persists_maps_sequences_and_queue_states -- --nocapture
cargo test --locked --lib http::admin_route_tests::production_map_canonical_snapshot -- --nocapture
cargo test --locked --lib core::chat:: -- --nocapture
```

Passed: missing-clock recovery, concurrent card creation/status ordering,
normal-message and legacy-trigger recovery, transaction rollback, concurrent
outbox claims, retained self-notification audit, bounded retry including a
570,000-attempt fixture, and real loopback WebSocket Ping/Pong and Close.

In Accord Mobile V2:

```sh
flutter test --no-pub test/native_iroh_transport_test.dart test/resilient_iroh_route_test.dart test/warehouse_live_client_io_test.dart test/admin_production_map_canonical_snapshot_test.dart test/admin_server_monitor_api_test.dart
flutter test --no-pub --dart-define=IROH_AUTO_CONNECT=true test/native_iroh_transport_test.dart
flutter test --no-pub test/admin_production_map_test_screen_test.dart --name 'worker recovery:'
flutter analyze --no-pub lib/src/core/native_iroh_transport.dart lib/src/core/realtime/warehouse_live_client_io.dart test/native_iroh_transport_test.dart test/warehouse_live_client_io_test.dart
```

The broad `admin_production_map_test_screen_test.dart` suite is not green:
32 failures were observed in the changed checkout. Every one also reproduced
in an isolated copy using the original HEAD versions of both changed transport
files. This comparison does not certify those existing UI flows or fix their
tests. Keep these failures separate from the focused transport acceptance.

## Completed backend deployment

- Verified the x86-64 ELF artifact and matching local/server SHA-256:
  `dae78a06d102158db2515bc92849dca00e8bb29447d14176696bdf7fe49c6a34`.
- The Linux migration helper first validated the existing 102 migrations. During
  maintenance, it applied `0103` through `0105` in the runner's single transaction,
  using the local PostgreSQL operator connection without changing service secrets.
- Maintenance ran from `2026-09-11T19:51:47Z` to `2026-09-11T19:51:58Z`.
  A quiesced custom dump, SQL dump, old binary, service definition, and environment
  backup are retained in
  `/home/user/mini_rs_erp_deploy/backups/chat-stream-cutover.8uqk6mNI`.
  The custom archive was fully decoded without restoring it, and dump/binary
  checksums were verified. A full restore rehearsal was not performed.
- Installed the matching migration helper and backup/restore support scripts,
  including the security SQL required by the restore script. Actual restore is
  an operator operation; the HTTP runtime was not granted owner credentials.
- Runtime-role privilege audit passed: 113 public tables, 357 indexes, zero
  invalid indexes, zero unvalidated constraints, and zero apparatus projection
  drift. Migration history is at `0105`, and the chat clock has one row.
- Existing freeze backlog recovered automatically: 20 events delivered, 43
  self-notifications retained as skipped, and zero pending events.
- Local health passed. The existing Cloudflare tunnel was also stopped during
  maintenance and was explicitly started again; public health and mobile server
  handshake then passed at `https://mini-rs-erp-dev.wspace.sbs`.
- `mini-rs-erp-erp2.service` remained active and was not deployed.

## Post-deployment reconnect incident

Both `cloudflared-mini-rs-erp-dev.service` and `mini-rs-iroh-agent.service`
declare `Requires=mini-rs-erp.service`. Explicitly stopping ERP at
`2026-09-11T19:51:47Z` also stopped both dependent services. Starting ERP alone
did not start them again. Cloudflare was restored during deployment checks,
but the Iroh agent was initially missed.

The installed mobile build still used Iroh automatically, and discovery still
advertised `auto_connect=true`. Consequently, healthy HTTPS and an on-screen
cached order list did not establish a healthy live connection. The worker
continued showing the reconnect banner. The agent was started at
`2026-09-11T19:56:02Z`; Wikki then confirmed that pressing Retry cleared the
banner. All three services were active with zero automatic restarts afterward.

For subsequent maintenance, record which dependent transport services are
active before stopping ERP. After ERP starts and passes local health, explicitly
start those previously active services on both the success and recovery paths.
Check their state and the public health/handshake before ending maintenance.
Do not unconditionally enable Iroh after a future intentional rollout shutdown.

## Remaining device acceptance

Mobile build/deployment requires Wikki's separate instruction. The local mobile
changes default Iroh opt-in to false; old installed clients have not received
that change. Changing only server discovery does not revoke cached tickets.
Authenticated quiet/live queue streams and real worker reconnects on factory
Wi-Fi and an external network still require device acceptance. Resume/freeze
actions on live orders were not invoked as deployment tests.

The reconnect-banner recovery above has user confirmation on one phone; it is
not a sustained-stream or cross-network acceptance test.

Iroh/firewall/relay tuning and physical-device acceptance remain deferred.
