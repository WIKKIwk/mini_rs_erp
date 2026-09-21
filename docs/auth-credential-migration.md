# PostgreSQL credential cutover

The server stores every account's access code in `mini_auth_accounts.credential_hash`
as an Argon2id PHC string (unique random salt, 19 MiB memory, two passes, one lane).
Builtin admin/warehouse/material identities are in `mini_auth_builtin_identities`.
Access flags move with the credentials. Migration 0129 also preserves an encrypted
copy for admin viewing and copying, with no regeneration cooldown. Other catalog
and role storage is unchanged. See [admin access codes](admin-access-codes.md).

## Existing installation

1. Stop every old ERP server instance so no process can update legacy codes
   during cutover. Keep the current database backup protected.
2. Build the new server and `mini_rs_auth_migrate`. Configure the existing database
   runtime/migration URLs and, if customized, `MOBILE_API_ADMIN_STORE_PATH`.
3. Run the tool in the same working directory as the server, where `.env` resides.
   Existing `ADMINKA_PHONE`/`ADMINKA_CODE`, warehouse/material environment codes and
   all JSON access states are imported. Supplier codes that were previously
   derived from supplier data are converted into hashes as well.

   ```sh
   cargo run --bin mini_rs_auth_migrate
   ```

   If this installation still used the old hard-coded administrator, supply the
   operator-chosen phone and code explicitly. Do not put a password in command-line
   arguments or shell history. The following reads it from a protected file:

   ```sh
   cargo run --bin mini_rs_auth_migrate -- \
     --admin-phone +998901234567 --admin-code-stdin < /secure/admin-code
   ```

   An existing 8-character code can be retained during migration. Initial bootstrap
   uses this same command with an empty legacy store.
4. The tool hashes/imports all credentials in one PostgreSQL transaction and
   records a cutover marker. Only after commit does it clear local JSON code fields
   and remove retired password keys from `.env`. Local replacements use mode 0600,
   atomic rename and fsync. If cleanup fails, rerun the tool: the committed marker
   prevents stale JSON/environment passwords from being imported again.
5. Remove obsolete password values from the service manager's environment/secret
   injection and dispose of the bootstrap input file according to your secret
   handling policy. Start the new server. Verify administrator login and one
   account from each deployed role. The same phone/code remains valid.

The tool never prints passwords or hashes. It does not rewrite Git history or
delete historical backups; any previous copy containing plaintext credentials
still needs restricted access. Do not roll back to an old server binary after
cutover: it cannot authenticate against the new hash store. The database dump now
includes authentication credentials, but existing non-credential catalog/media
backup requirements remain.

## Code issuance and recovery

Admin code-generation endpoints return the new code, and later authorized detail
and warehouse settings GETs return the same code for viewing and copying. Codes
remain available after restarting the server. The hash is never returned. Normal
settings updates ignore echoed `werka_code`; regeneration explicitly changes it.
Hash-only accounts from the earlier cutover retain their valid login codes; see
[recovery without resets](admin-access-codes.md) for restoring their readable copy.

An operator with database maintenance access can reset a lost admin code:

```sh
cargo run --bin mini_rs_auth_migrate -- \
  --reset-admin --admin-code-stdin < /secure/new-admin-code
```

Previously issued sessions are not automatically revoked by this command.
PostgreSQL must be available to validate logins; there is no environment/JSON
credential fallback. Successful logins are not counted by the failed-login limit.

## Original cutover verification (2026-09-20, before visibility was restored)

- 77 focused Rust tests passed: authentication, throttling, admin routes, local
  state, environment cleanup, PostgreSQL import/rotation and the migration CLI.
  PostgreSQL tests used disposable databases and the restricted `mini_rs_erp`
  runtime role. The CLI test applied all 126 migrations with a factory catalog.
- 13 focused Flutter tests passed, including code issuance during user creation
  and hiding the code when a profile is reopened. Analysis of the affected
  libraries had no errors or warnings; one existing generated-constant naming
  informational diagnostic remains.
- The real HTTP server passed ten consecutive successful admin logins, hidden
  credential GET responses, warehouse code issuance, login after restarting the
  server, and rejection of the old administrator code after reset.
- A broader Flutter widget run was also attempted and was not green: older
  customer/supplier/warehouse harnesses lack localization delegates, and worker
  settings tests leave notice timers pending. Those unrelated harnesses were not
  changed. The entire application test suite is not claimed to pass.

These checks used isolated local data. No running installation or production
database was migrated or deployed as part of this implementation.
