# Persistent production lists — 2026-10-23

## Checkpoint

The existing Sockudo integration was committed before this task:

- Backend: `32822e2f6483eddc73115baa294ea2a34c7c8179`.
- Mobile: `60943560da3b218b1a528b1e7e5f24db305fdd0e`.

## Implemented behavior

Production lists survive an application restart in a bounded on-device cache.
Workers and nonpaged supply/read-only views restore their canonical snapshot;
the paged administrator screen restores its previously loaded canonical page
prefixes and apparatus catalog. Cached rows are visible before the network
responds. Cached data does not authorize production actions: a matching server
ready, valid replay followed by ready, or canonical HTTP confirmation must first
release the existing authority barrier.

| Reopen scenario | List transfer |
| --- | --- |
| Matching cached cursor, unchanged server state | Small WebSocket ready; no list HTTP request |
| Matching baseline with retained changes | Existing compact state/page deltas |
| HTTP fallback, worker cursor unchanged | `304`, zero response body |
| Expired/lost replay, changed epoch or invalid baseline | Canonical HTTP replacement |
| Offline | Last cached rows remain visible; production actions remain unavailable |
| No valid cache | Existing initial HTTP loading |

Authentication, WebSocket control messages and background catalog refreshes still
use traffic. No fixed percentage reduction in total application traffic is claimed.
Backend replay remains bounded; this change does not introduce a durable server
event journal. A long-offline changed list can require a full replacement.

## Storage and lifecycle

- Native storage uses atomic files in the private application support directory;
  web selects the existing IndexedDB-backed SQLite dependency.
- Stable identities include server, account, role, capabilities and assignments.
  Worker window/search and administrator window identity remain separate.
  Raw authentication tokens are not serialized.
- Envelopes validate schema, identity, age and SHA-256 integrity. Unknown,
  truncated, corrupt and expired entries safely become cache misses.
- Defaults: 7 days, 24 entries, 16 MiB per entry, 64 MiB total. Administrator
  windows also retain their existing 12-window/20-page/5-page-per-window bounds.
- Logout invalidates old screen owners and queued/in-flight writes before the
  next session. Failed deletion does not make old records readable again.
- Canonical snapshots retain programs, geometry, queue policy, complete raw
  server action contracts, statuses, histories and notification state. Pending
  optimistic mutations are not saved as canonical baselines.
- Saves are coalesced (400 ms canonical, 250 ms administrator), skip unchanged
  identities and flush on background/navigation. Large native codecs and integrity
  checks execute in background isolates.
- Optional disk recovery has a 500 ms deadline. A late disk read
  cannot overwrite the subsequent fresh HTTP result.
- Demo mode does not populate the production canonical cache.

## Verification

The backend already supported the required cursor protocol. Eight new actual
HTTP/WebSocket route tests cover cold unchanged and empty lists, offline replay,
lost baselines, authorization/assignment/window changes, and administrator page
continuity. Together with the existing focused helper/page/conditional suites,
80 Rust tests passed.

The final mobile runs passed **185 focused tests**. Two existing cold-load text
assertions failed and were reproduced on the original checkpoint, as detailed
below. An additional existing scanner source-contract failure was also verified
against that checkpoint. The focused analyzer reported no issues across 11 items.

Mobile coverage includes real native file recreation, interrupted writes,
permission failures, corruption and limits, background codecs, account/logout
races, writable action barriers, delayed catalog responses, slow-cache fallback,
contiguous replay, missing history, old-server conditional reads and exact model
roundtrips. The final focused test run and analyzer results are recorded under
`/Users/wikki/Desktop/Accord_project/outputs/persistent_list_cache_20261023/`.

### Existing unrelated test failures

`production_map_worker_contract_architecture_test.dart` —
`worker start scanner validates WIP against server order and apparatus` fails at
line 121 on both the checkpoint and the current tree. It searches a scanner
substring for a direct `adminProgressQrLookup(` call; the code delegates through
`_checkWorkerWipScan(...)`. Both the test and its inspected source file are
byte-for-byte unchanged from `60943560`:

- Test blob: `2e311399462df1744f84e2f38db0f42577533cd5`.
- Scanner source blob: `975a9a39557c1d85f3c6cd2e73643484487da6c9`.

The unrelated scanner test was not rewritten. No full-repository green test claim
is made. No production deployment or real-device battery/WAN measurement was
performed in this task.

The two existing `worker_network_latency_test.dart` cold-load `503`/timeout
widget cases also fail their reconnection-text assertion on both current code
and the complete isolated `60943560` checkout. The actual HTTP-start and timeout
assertions pass; the UI renders connection progress instead of that text. All
1,521 archived tracked source blobs were checked against the checkpoint. Those
original assertions were retained, and their baseline/current evidence is saved
with the final test artifacts.
