# Canonical Apparatus Clean-Cutover Runbook

Status: operator procedure for migrations 0069 and 0070.

This procedure performs a clean cutover. It has no dual-read, dual-write, or
legacy fallback interval. Never run it against a live database without the
normal production change approval, maintenance window, and verified backup.

## Preconditions

- The old binary is stopped before the cutover transaction starts.
- Database migration history is valid and ends at 0068.
- A restorable pre-cutover backup and its checksum have been verified in an
  isolated restore rehearsal.
- The new binary and `mini_rs_apparatus_cutover` binary come from the same
  reviewed release commit.
- `MINI_ERP_MIGRATION_DATABASE_URL` identifies the intended maintenance
  database. The command does not run migrations automatically.

Record the current row counts, migration history, backup path, backup SHA-256,
old binary identifier, and new binary identifier in the change ticket.

## 1. Install canonical authority schema

Run the migration binary with the explicit operator gate; do not start the
application binary yet:

```sh
mini_rs_migrate --through 0069
```

The command uses the normal advisory lock, checksum validation and one
transaction, but cannot advance into the clean-cutover migration. Confirm that
`mini_schema_migrations` ends at
`0069_canonical_apparatus_revision_authority` and that migrations 0001-0068
retain their recorded checksums and `applied_at` values.

Migration 0069 adds append-only revisions, current heads, exact AASX bytes,
runtime projection provenance, writer guards, outbox, and diagnostics. Its
nullable legacy projection provenance exists only as migration input before
the manifest transaction.

## 2. Produce and review preflight

```sh
mini_rs_apparatus_cutover preflight apparatus-preflight.json
```

The report fingerprint binds the complete observed migration source. Review
every legacy apparatus, observed identity, configuration source and hash,
dependent foreign-key count, apparatus-named text reference, and diagnostic.
Do not continue when `blocking_issues` is non-empty.

Create `apparatus-cutover-drafts.json` with exactly one complete canonical
draft for every legacy apparatus. The stable target `ApparatusId` must equal
the already canonical legacy `mini_apparatus.id`; the tool never derives or
replaces identity from names or aliases. Resolve every conflict explicitly.

## 3. Build and preview the exact manifest

```sh
mini_rs_apparatus_cutover build \
  apparatus-cutover-drafts.json apparatus-cutover-manifest.json
mini_rs_apparatus_cutover preview \
  apparatus-cutover-manifest.json apparatus-cutover-preview.json
```

`build` reruns preflight and refuses a stale fingerprint. The resulting
manifest contains the complete revision, deterministic AASX SHA-256, and exact
acknowledgement of every observed identity and source hash. Review the preview
and reconcile source identities/counts before applying it.

## 4. Apply the atomic cutover

```sh
mini_rs_apparatus_cutover apply \
  apparatus-cutover-manifest.json apparatus-cutover-result.json
```

`apply` uses a serializable transaction and a global advisory lock. It reruns
preflight under the transaction snapshot and aborts if the source fingerprint
changed. It inserts identity, revision 1, exact AASX, head, mandatory runtime
projection, queue/material/capacity projections, and exactly one outbox event
per apparatus. Any error rolls back the complete transaction.

Verify that source and target apparatus counts match, every source identity is
present exactly once, all heads/revisions/projections/outbox counts reconcile,
and `mini_canonical_apparatus_projection_drift` is empty.

## 5. Remove legacy authority

Apply migration `0070_canonical_apparatus_clean_cutover`. It fails closed on
an upgraded database unless the exact P11 manifest transaction has populated
all canonical provenance. It then makes provenance mandatory, makes every
retained projection fully read-only outside the canonical writer, and removes
legacy groups and configuration columns.

Apply migration `0071_qolip_lock_ownership`. It marks only canonical
Qolip-tooling apparatus sessions as physical lock owners; downstream progress
sessions retain Qolip lineage without becoming a second lock authority.

Restart migration once and prove `(version, checksum, applied_at)` history is
unchanged. Start the new binary only after its startup canonical repository
check succeeds.

## Post-cutover checks

- Current canonical revision count, head count, runtime projection count, and
  outbox count agree.
- Every retained queue/material/capacity row has current revision/hash
  provenance and `mini_canonical_apparatus_projection_drift` returns zero rows.
- No unresolved/orphan/duplicate diagnostic returns a row.
- Admin list/detail reads projection data; authenticated AASX GET returns the
  exact stored bytes; ordinary runtime requests do not read the AASX BYTEA.
- A rename preserves `ApparatusId` and historical queue/WIP references.
- Direct writes to canonical or projection tables fail outside
  `CanonicalApparatusService`.

## Rollback

There is no application-level fallback after migration 0070. If acceptance
fails, stop the new binary, restore the verified pre-cutover backup into the
approved database target, verify the restored migration history and counts,
and redeploy the recorded old binary. Do not reverse migrations in place and
do not re-enable legacy readers or writers in the new binary.
