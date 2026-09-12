# Runtime database privileges

`audit_runtime_privileges.sql` is a read-only check of application table and
sequence permissions, append-only guards, and the owner privileges required by
foreign-key row locks in both directions. On hardened databases it also checks
runtime/owner separation, read-only migration history and reset definer ACLs.
It reports invalid indexes, unvalidated constraints,
and canonical-apparatus projection drift.

Run it with an authorized database connection (the role defaults to
`mini_rs_erp`; override with `-v runtime_role=...`):

```sh
psql -X -v ON_ERROR_STOP=1 --dbname "$MINI_ERP_MIGRATION_DATABASE_URL" \
  -f tools/db/audit_runtime_privileges.sql
```

## Repair for migration 0102

0102 revokes UPDATE from append-only tables. When the application role also
owns a referenced table, PostgreSQL's internal `SELECT ... FOR KEY SHARE` then
fails during otherwise authorized child INSERTs. It requires UPDATE on at
least one column, even though it does not modify the referenced row.

0103 grants UPDATE on one key column of each of the six affected parent tables,
after verifying its enabled immutable trigger. Broad UPDATE and DELETE remain
revoked; the triggers still reject actual row changes. The original 0102 SQL
and checksum must not be modified.

Normal upgrades apply 0103 through the migration runner. For an authorized
repair while an older binary is still running, first take a database backup,
then apply the exact 0103 SQL in a transaction:

```sh
psql -X -v ON_ERROR_STOP=1 --single-transaction \
  --dbname "$MINI_ERP_MIGRATION_DATABASE_URL" \
  -f migrations/postgres/0103_runtime_foreign_key_locks.sql
```

Do **not** insert 0103 into migration history while running a binary whose
registry ends at 0102: that binary rejects unknown migrations on restart.
The privilege repair persists independently of history. A later binary with
0103 registered can safely reapply and record the idempotent migration. Do not
grant unrestricted UPDATE/DELETE or disable immutable triggers as a workaround.

## Regression test

The psql regression uses real PostgreSQL FK checks, reproduces all six failures,
checks the repaired inserts and row locks, verifies UPDATE/DELETE protection,
tests separate/runtime table owners, and verifies refusal when a guard is
disabled. It rolls back every fixture and requires an empty disposable database
named `mini_rs_erp_test_runtime_fk_*` plus an existing `mini_rs_erp` role.

```sh
psql -X -v ON_ERROR_STOP=1 --dbname mini_rs_erp_test_runtime_fk_local \
  -f tools/db/tests/runtime_foreign_key_locks.sql
```

## 0104: runtime security and guarded order reset

0104 is an operator migration: the first run requires a role able to create the
non-login `mini_rs_erp_owner` role and transfer this database's ownership. It
never changes the runtime password or reassigns objects in another database.
The runtime must not be a member of the owner role. Application tables,
sequences, views, functions and the public schema belong to the owner; HTTP
gets the required CRUD/EXECUTE privileges, read-only migration history, and
only one UPDATE column on each of nine append-only tables for row locking.

The reset store keeps its admin authorization, verified backup, advisory lock,
transaction and final verification. Only its privileged history deletion and
transactional sequence restart use narrowly scoped SECURITY DEFINER functions.
The functions use qualified objects, a fixed safe search path, no PUBLIC
EXECUTE, and restore function-local settings automatically. Merely setting
`mini_rs_erp.order_reset=on` cannot bypass an append-only guard. No UPDATE reset
exception exists; DELETE is allowed only on raw-material order history under
the definer's actual owner identity. Other append-only histories remain blocked.

Deployment order:

1. Build the matching ERP and `mini_rs_migrate` binaries; take a verified backup.
2. Stop the old ERP for the coordinated upgrade. Run the new migration binary
   with an operator connection (or as the database owner once provisioned).
3. Install/start the matching ERP with `MINI_ERP_DATABASE_URL` set to the ordinary
   runtime login. Startup validates existing migration checksums without DDL.
   A configured migration connection is not returned to HTTP stores.
4. Run the read-only audit and real API/health checks. Never call reset on live
   orders to verify deployment. Keep the old binary and database backup;
   old binaries cannot start against newer migration history.

Future migrations should run as the owner, via a controlled operator session.
New append-only tables must explicitly narrow their default CRUD grant and
install immutable guards. Do not edit already-applied migration checksums.

Restore is a maintenance operation: provide `MINI_ERP_RESTORE_DATABASE_URL` or
`MINI_ERP_MIGRATION_DATABASE_URL` with database-owner authority; the HTTP login
is intentionally insufficient. The restore script checks this before dropping
anything and reapplies 0104 ownership/ACLs in the restore transaction, because
`pg_restore --no-owner --no-privileges` otherwise discards the boundary while
retaining migration history. Package the 0104 SQL alongside the restore script.

The existing Rust-native DB/rollback integration test now connects as the real
runtime role, checks forged flags, forbidden owner switching/trigger disabling,
child FK row locks, temporary-table shadowing, and late-failure rollback before
executing the backed-up HTTP reset against its own disposable database:

```sh
MINI_ERP_TEST_ADMIN_DATABASE_URL=postgres://superuser@127.0.0.1:5432/postgres \
  cargo test --locked --test order_reset_e2e -- --nocapture
```

Use a local PostgreSQL instance permitting the existing `mini_rs_erp` test
login. The test creates only its PID-scoped database, never uses production
data, and preserves the non-login owner role for subsequent test migrations.
