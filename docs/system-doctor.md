# Algorithmic System Doctor

System Doctor uses repeated checks, separate recovery thresholds and bounded
automatic backup attempts. These are independently implemented policies inspired
by [Monit's monitoring model](https://mmonit.com/monit/documentation/monit.html),
without an AI service or a Monit dependency.

## Continuous checks

The backend starts the sampler during startup, independently of the mobile
monitor screen. The nominal interval is two seconds; slow checks skip missed
ticks instead of accumulating work. Database probes have a two-second timeout.
Backup catalogue scans run in a separate worker, with at most one scan in
progress, so slow backup storage does not suspend resource observations.
They acquire connections through the existing SQLx pool, so later probes can
observe recovery without replaying production or warehouse commands.

The Doctor confirms a fault after three consecutive failing observations and
requires two recovery observations to resolve a confirmed incident. A transient
failure is a warning. A recovering incident remains active until recovery is
confirmed. Reading the REST endpoint or opening extra WebSockets does not
advance these counters.

| Check | Fault threshold | Recovery threshold |
| --- | --- | --- |
| Database | Configured database is unreachable | Configured database is reachable |
| Host memory | At least 90% used, excluding estimated reclaimable memory | At most 80% used |
| Selected filesystem | At most 1024 MiB available | At least 1536 MiB available |

Thresholds and confirmation counts are configurable with the
`MINI_ERP_DOCTOR_*` settings in `.env.example`. Memory is measured on Linux and
macOS; disk checks use `MINI_ERP_DISK_MONITOR_PATH` or the backend working
directory. External metric commands have a two-second timeout and are killed
on cancellation. Missing or malformed memory/disk observations are `unknown`,
and cannot resolve an existing incident. `cpu_percent` retains the existing
normalized load-average meaning; CPU pressure is informational in this version.

## Automatic resource protection

A confirmed memory or disk incident pauses admission of new manual backups,
scheduled backups and database imports. Manual requests return HTTP 503 with
`backup_resource_pressure`. Scheduled attempts defer without creating a failed
backup or consuming their retry budget. Previously admitted jobs retain their
reservation. Admission resumes after confirmed resource recovery.

This version controls backup/import admission and can restart a managed
Cloudflare Tunnel after confirmed public-domain failures. It does not kill
unrelated processes, automatically restore a database, delete production files,
or restart the ERP process. ERP process crash recovery requires an external
supervisor. Host-wide memory is observed; per-process RSS and container-specific
limits are not collected.

Automatic scheduled backups have at most three attempts per configured local
calendar day. The second attempt waits at least one hour after the preceding
attempt finishes; the third waits at least two hours. A new day resets the daily
budget but retains a global one-hour cooldown. Attempts are counted from saved
manifests, preserving the budget after restart. A successful recovery-ready
backup on the same local day satisfies the schedule. Manual attempts do not
consume the automatic retry budget.

## Health and diagnostics

- `GET /healthz` remains the lightweight process liveness response.
- `GET /readyz` returns 200 only when a cached sample no older than 15 seconds
  reports a reachable configured database, no resource pressure and no
  confirmed public-domain outage.
  A confirmed database outage must also complete its two recovery observations.
  Startup, stale samples and unavailable database return 503. It exposes only
  `{"ok": true}` or `{"ok": false}` and uses `Cache-Control: no-store`.
- Authenticated admin REST/live monitoring responses include `doctor.status`,
  `doctor.protected`, `doctor.checked_at_unix`, per-check observations and the
  last 32 state transitions. Transitions are also recorded through tracing;
  the in-memory journal resets after restart. Raw database error details are
  not copied into the Doctor journal.

Use the backend's `/readyz` for application readiness: the gateway's own
`/healthz` indicates gateway liveness. Existing mobile clients can continue to
read the original monitoring fields and ignore the additive `doctor` object.

## Public domain and tunnel recovery

When `MINI_ERP_DOCTOR_DOMAIN_URL` or `PUBLIC_HOSTNAME` is configured, Doctor
checks `https://<host>/healthz` every 30 seconds. Three consecutive failed
checks mark the domain critical and appear in `doctor.checks` and
`doctor.domain`. It confirms the local ERP endpoint is healthy before attempting
recovery. If it is, Doctor restarts the `cloudflared` process using the existing
`cloudflared.yml` and reconnects the named tunnel. It does not change DNS routes
or create a new tunnel. Recovery attempts are at least one minute apart; two
successful public checks clear the incident.

`tools/runtime/up_domain.sh` passes its selected host, local endpoint, tunnel
state directory and tunnel name to the ERP automatically. For other launch
methods, configure the corresponding `MINI_ERP_DOCTOR_DOMAIN_*` values in
`.env.example`. Restart the ERP after changing them. A stopped ERP process
cannot run its own Doctor; configure the operating system's service manager to
restart the ERP process.

## Open Wi-Fi Internet recovery

An independent Doctor worker checks Internet access every 15 seconds. After
three consecutive failed checks it scans visible, explicitly open Wi-Fi access
points. Stronger signals are tried first. Association alone is insufficient:
after a five-second settling period the worker verifies Internet access using
HTTPS probes. Captive-portal redirects, login pages, TLS failures and non-204
responses do not count as Internet access. Either of the two independent
default probe providers succeeding is sufficient to keep the current route.

If an AP has no Internet access, Doctor disconnects its own attempted connection,
waits five seconds and tries the next candidate. A round tries at most five
different APs. Failed APs have a five-minute cooldown. If a successful scan no
longer sees a failed AP and a later successful scan sees the same BSSID again,
its remaining cooldown is cleared and it can be tried in that scheduled round.
Each later disappearance/reappearance can trigger another retry; a continuously
visible AP keeps its cooldown, and scan errors do not count as disappearance.
Internet verification and round retry limits still apply. Exhausted rounds restore
the original saved connection where possible and wait at least one minute before
rescanning; repeated exhausted rounds back off up to ten minutes. Connectivity
is rechecked before each new association so a recovered existing route stops
further switching. Cleanup failures prevent further switching until cleanup
succeeds. If the original AP is unavailable, the restoration failure is recorded
and later rounds can still discover other working open APs after cooldown.
Permission/configuration failures suspend recovery until configuration or OS
permissions are corrected and the service is restarted.

`MINI_ERP_DOCTOR_WIFI_ENABLED` defaults to `true`; set it to `false` to disable
the worker. All timing/confirmation limits, interface and HTTPS probe URLs are
configured using the `MINI_ERP_DOCTOR_WIFI_*` variables in `.env.example`.
Configured probes must use HTTPS, have no embedded credentials and return 204;
the default requests contain no MES credentials or application data. Disabled
configuration performs no probes, scans or association attempts. Invalid
configuration and missing OS permissions are visible in `doctor.wifi`.

The network worker does not block database/RAM/disk sampling and does not alter
application readiness semantics. Admin monitoring includes `doctor.wifi.status`,
attempt/failure counts, selected SSID, next attempt time, sanitized error codes
and the last 16 network transitions. Native networking is disabled in unit tests
and the isolated contract-verification harness; tests use fake adapters and
local HTTP fixtures.

### Linux

The host must use NetworkManager with `nmcli` available to the backend service
account. That account needs permission to scan and activate Wi-Fi connections.
Only open infrastructure APs with no WPA/RSN security flags are eligible.
Activation uses a new in-memory profile with autoconnect disabled and pins the
exact BSSID. Cleanup deletes only UUIDs created by Doctor. Existing Ethernet
connections and other saved profiles are preserved. The previous Wi-Fi profile
UUID is retained for restoration.

### macOS

Build the CoreWLAN helper on macOS before deploying the feature:

```sh
make doctor-wifi-helper
```

Deploy the generated `tools/runtime/doctor_wifi` alongside the backend, or set
`MINI_ERP_DOCTOR_WIFI_MAC_HELPER` to its absolute path. The running helper/host
must already have Location Services authorization and permission to associate
with Wi-Fi. A headless service may require an authorized/signed host identity.
Missing authorization produces `permission_denied`; Doctor does not request
interactive authorization or extract passwords from Keychain.

The helper re-scans the exact SSID/BSSID and verifies `.none` security before
association without a password. Restoring a secured original connection uses
its existing macOS saved profile and service-account access; failure is reported.
CoreWLAN has no per-association no-remember flag. The helper does not commit
network configuration, but macOS may remember joined networks according to the
host's existing settings.
