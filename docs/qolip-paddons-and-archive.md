# Qolip paddons and reversible archive

Qolip paddons package existing physical molds. They are warehouse-scoped and
independent of production/WIP pallets. A mold shared by several products has
one physical membership, identified by its canonical mold code. Standalone
sets remain eligible without inventing a product association.

## Operator workflow

Create a paddon, select or scan existing molds, review the selection, then add
them. Locking is an explicit operation and does not depend on printing. A
locked paddon can be inspected or archived; composition changes require an
explicit unlock. Archived paddons are read-only until restored. Restoring keeps
the previous lock state, identity, QR code and members.

Molds outside paddons can be archived independently, including a batch of
selected molds. To retrieve one mold from an archived paddon, restore and
unlock its paddon first. Remove the mold, then optionally lock and archive the
remaining contents. Each archive visit retains its own snapshot: an earlier
eight-mold visit still shows eight molds after a later six-mold visit.

Archive is not peredelka. It never starts a mileage cycle, erases manufacturing
history, changes product composition, or silently releases production locks.
Inspection records expected, seen, missing and unexpected mold codes without
editing membership or opening the lock.

## HTTP surface

All endpoints use existing `qolip.manage`/administrator authorization and check
warehouse ownership on the server, including history and replayed requests.

| Method | Path under `/v1/mobile/qolip` | Purpose |
| --- | --- | --- |
| GET | `/paddons` | Active or archived paddons |
| GET | `/paddons/detail?code=QP-00001` | Current members and event history |
| GET | `/paddons/molds` | Canonical molds and availability |
| POST | `/paddons/qr/print` | Print a label without changing lifecycle state |
| POST | `/paddons/action` | create, add, remove, lock, unlock, archive, restore, inspect |
| GET | `/archive` | Archive visits, including historical returned visits |
| POST | `/archive/action` | Archive individual molds or restore selected visits |

Writes require a `request_id`. Retrying the same intent retains that ID;
changing its payload requires a new ID. Paddon mutations other than creation
also require `expected_revision` from the displayed detail. A stale revision
requires refreshing and reviewing current contents, not a silent retry against
the new revision. Individual restore targets archive entry IDs rather than mold
codes so an old request cannot restore a later archive visit accidentally.

The print endpoint uses the existing Qolip printer input (`qolip_code`,
`driver_url`, `printer`, `print_mode`, `print_count`, `print_transport`) rather
than a lifecycle request. Printed/displayed pallet QR payloads are typed:
`qolip-paddon:QP-00001`. This keeps arbitrary mold codes, including existing
`QP-*` mold codes, distinct from pallet identifiers. Printing is not retried as
a lifecycle mutation and does not require or change a package revision.

## Integrity and compatibility

The database serializes storage operations with physical mold resource locks.
Packing/archive must reject open checkouts, active production ownership,
peredelka and inventory transfers. Existing checkout, movement, editing and
production entry points must also reject packed/archived molds, including
catalog-only molds which have no home-stock row.

Operational stock excludes held molds while home identity remains reserved.
Code quantities are preserved; packaging does not create another stock receipt.
Archive filters on user-facing catalogs must not alter
`mini_qolip_effective_product_molds`: production still needs the complete
selected set and its extensions. An archived alternative does not invalidate
another available, complete alternative.

The runtime role has no deletion permission on storage history. Historical
memberships retain references to physical codes. Renaming/deleting a referenced
mold must not detach its history. Stored archive snapshots remain readable
after the current paddon changes.

## Deployment and validation

Apply the registered `0173_qolip_paddons` and `0174_qolip_storage_guards`
migrations together through the normal maintenance migration process before
using the new mobile routes. Do not apply them manually to a live database as
part of a local test run. Existing WIP pallet tables and APIs retain their own
semantics.

Use a disposable PostgreSQL cluster for integration tests, with
`MINI_ERP_TEST_ADMIN_DATABASE_URL` explicitly set to that cluster. Relevant
checks include unauthorized reads/writes, warehouse isolation, concurrent
membership and checkout attempts, all-or-nothing batch failures, retry replay,
stale revisions, repeated archive visits, snapshot preservation, locked restore,
inspection, and unchanged mileage. Mobile checks cover stale responses,
account changes, canceled confirmations, conflict recovery and archived detail
navigation as well as successful flows.
