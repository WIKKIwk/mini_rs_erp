# Automatic mold confirmation for printing starts and released runs

The administrator may enable automatic mold confirmation in Additional Settings.
The default is off. Enabling it removes individual mold QR scans for a fresh
production Start, or replacement tooling on Resume after freeze, on equipment whose tooling policy requires
Qolip scans. Material/WIP scans, assignments, print preflight and other queue
gates continue to apply.

The server still requires a nonempty compatible physical set, all product-local
extensions, no active rework and valid resource ownership/stock. A catalog-only
mold without stock or checkout retains the existing accepted policy. Automatic
confirmation verifies recorded state; it does not establish physical presence
at the machine. Multiple alternative base sets require selecting one set.

An active rework on any effective mold of the order continues to block the
whole order, even if another alternative is intact. Readiness returns individual
mold statuses so healthy molds may be displayed beside an unavailable one.
Opening a sheet does not reserve inventory or create a checkout. Start claims
the selected molds using the ordinary checkout, lock and session/WIP lineage
transaction.

## API

- `GET/PUT /v1/mobile/admin/production-maps/qolip-start-settings` is restricted
  to the administrator principal with `admin.access`. PUT accepts
  `{"automatic_confirmation_enabled":true}`. The response includes
  `settings.automatic_confirmation_enabled` and a monotonic `settings.revision`.
- The queue interaction retains `qolip_mode:"scan_required"` and adds
  `qolip_automatic_confirmation` and `qolip_policy_revision`. Existing clients
  therefore retain manual scanning.
- New clients opt in with `automatic_confirmation:true` on the empty-code
  `qolip-validate` request, or `automatic_qolip_confirmation=true` on the order
  scan bootstrap. When enabled for the fresh start, `qolip` includes policy
  revision, an aggregate blocker, `sets` with physical-composition fingerprints
  and readiness, and individual `required_qolips` with readiness and blockers.
- An automatic Start sends `qolip_confirmation` containing
  `method:"automatic"`, `set_id`, `fingerprint`, and `policy_revision`.
  Resume after freeze accepts the same confirmation. Combining it with manual
  mold codes, other actions, training, or unsupported
  equipment is rejected. The server derives the selected codes.

Policy changes invalidate production views. An outdated automatic confirmation
is rejected at Start; the policy row is held through the commit so concurrent
disabling cannot admit a start after the policy change. The selected composition
fingerprint and every product's common-set binding are rechecked in that same
transaction. Start history records the automatic method and the actual selected
set/codes. Ordinary Resume retains its mounted tooling. A released printing run
advertises `qolip_mode:"scan_required"` on Resume. If its saved complete set still
matches the catalog, the manual requirements response includes
`reused_qolip_codes`; the mobile client restores those confirmations without
requiring another scan. Otherwise the operator scans the current complete set,
or confirms/selects it using the automatic policy. Resume revalidates catalog
composition, availability, checkouts and policy in the commit transaction.
The session and subsequent output use the new set. Earlier output remains
unchanged, and the queue event records `qolip_replaced_on_resume` with previous
and current tooling. Completed-work reactivation and training are unchanged.

## Rollout

Apply migration `0168_qolip_start_settings` with the maintenance migration
credentials before starting this backend revision. It creates the default-off
policy and its production invalidation trigger. Update mobile applications to
use the opt-in contract before expecting automatic confirmation; older clients
continue their existing manual workflow. The new setting is stored per backend
database and survives server restarts.
