# Mobile transport: authenticated Iroh + LAN hints + HTTPS fallback

## Contract

- HTTPS remains the cold-start/default path. Discovery and health probes run
  in the background. A cached endpoint gets a bounded 250 ms recovery window.
- Native iOS/Android use one reusable encrypted Iroh/QUIC connection after a
  successful `/healthz` probe. Iroh can use direct IP or relay paths. LAN
  discovery adds local address hints; same SSID/public IP is **not** proof of
  server identity. Native bridges are linked on iOS using IrohLib 1.0.0.
- The initial identity comes only from the same HTTPS origin's
  `/v1/mobile/iroh-ticket`, without redirects, with `auto_connect: true` and
  `supports_connection_reuse: true`. Cache is scoped to the exact HTTPS origin;
  legacy unscoped tickets are not used for automatic routing.
- Bonjour/Android NSD advertises `_accord-erp._udp` with `server_ref` equal to
  the public endpoint ID. Discovery uses a separate native channel from the
  printer, so starting ERP discovery cannot cancel printer discovery.
  Private IPv4 hints must match the pinned ID; QUIC authenticates that ID.
- ERP HTTP stays on loopback. The sidecar only forwards `/healthz` and
  `/v1/mobile/*`. Bearer tokens, server-side authorization and PostgreSQL
  transactions remain unchanged. Tickets are addresses, **not** credentials.
- GET/HEAD can switch paths on transport failure. A mutation switches only
  for native `iroh_not_sent`, emitted strictly before the first write attempt.
  A lost acknowledgement, timeout or partial write never causes automatic
  POST/PUT/PATCH/DELETE replay. Its outcome must be reconciled with ERP; no
  fictitious success is shown. Server 401/403/409/500 are not transport retries.
- Failures suspend the native route for 15 seconds; subsequent operations
  use HTTPS. New probes can restore it. Uploads above 1 MiB and Flutter Web
  retain HTTP. Saved/manual HTTPS endpoints are eligible, but only using
  their own origin's authenticated discovery. Plain HTTP overrides retain HTTP.
- Background app health checks use the same native route and HTTPS fallback.
  Live subscriptions give discovery up to 3 seconds before choosing WSS;
  worker mutations do not wait for this cold-start discovery.
- The sidecar keeps a stable 32-byte private key (0600 on Unix), publishes
  updated tickets atomically and starts without waiting for a relay. A
  previously paired phone can therefore reconnect locally without WAN.
  A first-ever pairing still needs trusted HTTPS. Guest Wi-Fi/client
  isolation, denied local-network permission or blocked UDP can prevent LAN;
  HTTPS remains available when reachable. No transport guarantees service
  during total network loss or ERP shutdown.

## Safe activation (operator-controlled)

Build/test source first. No running ERP, tunnel or installed phone is changed
merely by this code being committed.

The existing domain launcher supports opt-in:

```sh
IROH_ENABLED=1 bash tools/runtime/up_domain.sh <existing-hostname>
```

Successful explicit activation creates `iroh.enabled` in the domain state
directory. Normal subsequent launcher runs retain that opt-in. An explicit
`IROH_ENABLED=0` skips sidecar activation for that invocation; it does not stop
an already-running sidecar or erase the persisted opt-in.

It starts the separate sidecar and supplies the following settings to a newly
started ERP process:

```text
IROH_TICKET_FILE=<domain-state-directory>/iroh.ticket
IROH_SECRET_KEY_FILE=<domain-state-directory>/iroh.key
IROH_SUPPORTS_CONNECTION_REUSE=1
IROH_AUTO_CONNECT=1
```

The launcher **does not restart a healthy ERP or tunnel** to activate discovery.
An already-running ERP must inherit these variables at its next approved
normal restart. On other deployments, run the sidecar under the existing
service supervisor with a loopback ERP target and persistent private key file.
Never copy a test key into production or silently replace a damaged key.
On Windows, restrict the key directory ACL to the runtime service account.

The updated Mobile build defaults to `IROH_AUTO_CONNECT=true`, but remains
HTTPS until the server opts in and the native route passes health checks.
Build with `--dart-define=IROH_AUTO_CONNECT=false` to disable automatic routing.
Retired unscoped/manual ticket preferences do not opt a server into this flow.

For immediate server-side rollback, stop only the verified sidecar process
using its service supervisor. Keep ERP and HTTPS running. Native operations
already sent must not be manually retried until their outcome is checked.
Setting server `IROH_AUTO_CONNECT=false` blocks new bootstrap; already paired
offline-capable clients retain their identity cache, so stopping the sidecar
is the definitive rollback for those clients.

## Verification and release gate (2026-09-11)

- Focused Flutter transport tests cover pre-send vs unknown-outcome failures,
  timeouts, concurrent warm-up, stale probes, origin isolation, false LAN
  adverts, cached offline LAN recovery, HTTP framing and server capability gates.
  The activation follow-up passed 73 transport/auth/endpoint tests and 8 worker
  start/resume tests; focused Dart analysis reported no issues. Saved HTTPS
  overrides are explicitly covered by a regression test.
- Rust tests exercise a real relay-disabled QUIC connection across multiple
  requests to an isolated mock HTTP server, preserving Authorization and 401.
  Key persistence, corrupt-key refusal and request-smuggling rejection are covered.
- A temporary relay-disabled sidecar reached the running ERP `/healthz` ten
  times with HTTP 200; Bonjour advertised the expected service. This is
  same-machine smoke evidence, not a physical-phone/Wi-Fi latency benchmark.
- Swift 5 type-check of the Iroh bridge passed against resolved IrohLib 1.0.0.
- A fresh signed iOS release (1.0.17, build 47) built successfully using the
  supported install script and was installed on the paired iPhone 11 Pro.
  Device inventory confirmed build 47 for
  `com.example.accordMobileV2.mirsaid.uzkingshark`; the other installed Accord
  bundles were preserved. The earlier generated `audio_session` package issue
  no longer blocked this build. Physical-phone direct-path/latency evidence
  is still pending; installation alone does not prove LAN use.
- Android's unrelated unresolved `packQrCellWidth` in
  `BluetoothPrinterChannel.kt` was not changed or rebuilt in this activation.
- Before broader rollout, require full native builds and physical iOS
  and Android checks: LAN, WAN-only, LAN with WAN disconnected, permission
  denial, guest network, ERP restart and reply loss during a worker action.
  Do not report an action as completed until the ERP confirms it.
- Activated the sidecar for `mini-rs-erp-test.wspace.sbs`, then restarted only
  the existing ERP release process with its discovery environment. The HTTPS
  tunnel process was preserved. Local and public `/healthz` returned 200;
  both ticket endpoints returned `auto_connect: true` and
  `supports_connection_reuse: true`. No database migrations were added/run.
