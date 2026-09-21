# Local Mac PostgreSQL access

The Mac database uses SCRAM-SHA-256 authentication. Both TCP loopback addresses
and Unix sockets require authentication. PostgreSQL remains bound to localhost.
ERP user roles and login codes are independent of these database credentials.

Two generated passwords live in the login Keychain under the service named by
`MINI_ERP_DATABASE_KEYCHAIN_SERVICE`. Account names match the users in the two
password-free database URLs in `.env`. Do not commit `.env`, passwords, dumps,
or Keychain exports. The PostgreSQL data directory contains SCRAM verifiers.

## Starting the backend

Run these commands from the repository root:

```sh
python3 tools/db/macos_keychain.py run
# Development:
python3 tools/db/macos_keychain.py run -- cargo run --bin mini_rs_erp
```

`tools/runtime/up_domain.sh` uses this helper automatically on a configured Mac.
Launching the binary directly bypasses Keychain loading and cannot authenticate.
The helper passes only the restricted runtime credential to the HTTP process.
The already-applied migration registry is checked with that account. New schema
migrations require a separate maintenance command before restarting the service:

```sh
python3 tools/db/macos_keychain.py migrate
```

Build the migration binary first when source migrations have changed. Do not use
`migrate` mode to launch the HTTP server. Other platforms retain their existing
startup path. Unlock the login Keychain before startup after a logout/reboot.
The helper fails closed if Keychain access is unavailable.

## Database console and backup

```sh
# Restricted application account:
python3 tools/db/macos_keychain.py db
# Explicit maintenance console:
python3 tools/db/macos_keychain.py --admin db
# Existing backup procedure, including role definitions:
python3 tools/db/macos_keychain.py --admin db -- bash tools/db/backup_postgres.sh
```

PostgreSQL tools receive `PGPASSWORD` in their process environment; their command
arguments and the URLs passed by the backup script contain no password. Do not
print environments or enable shell tracing around credential-bearing commands.
Preserve backup directory permissions (0700) and file permissions (0600).

The backend also uses `PGPASSWORD`, which SQLx supports, so its backup subprocesses
keep working without embedding a password in command arguments. An authorized
in-app restore obtains the maintenance credential in a separate restore process,
then runs the migration binary before the backend's final migration check.
`up_domain.sh` builds both binaries on Mac. Keep them on the same source revision.

## Forgotten password or unavailable Keychain

The generated database password does not need to be memorized. With an unlocked
login Keychain, the maintenance console above continues to work. Password loss
does not destroy or encrypt the PostgreSQL data.

If Keychain access is lost, the authorized OS owner of the PostgreSQL data
directory can recover access locally. Stop the ERP, save the current HBA file,
and temporarily put `local all wikki peer` before the socket SCRAM rule, replacing
`wikki` with the actual database/OS administrator name if different. Reload HBA
using the bundled `pg_ctl`, then connect through the configured Unix socket as
that OS user. TCP rules must remain SCRAM throughout. Set new generated passwords
and store matching values in Keychain through an operator-controlled process;
verify fresh SCRAM connections, remove the temporary peer rule and reload again.
Do not restore the old `trust` rules as a permanent recovery mechanism.

Recovery requires access as the PostgreSQL OS owner (or an authorized system
administrator). If both that access and Keychain are lost, recovery requires a
separately protected backup. A tested recovery procedure and a readable dump
should be kept outside Git; restoring a dump is not needed for password reset.

## Security boundary

These rules close unrestricted database login. The runtime account retains its
existing application grants and cannot become the database superuser. They do
not isolate PostgreSQL from malicious software already running as the same Mac
user who owns its files and Keychain. Separate OS service accounts are a later,
distinct hardening step.

Reference: [PostgreSQL password authentication](https://www.postgresql.org/docs/18/auth-password.html)
and [local peer authentication](https://www.postgresql.org/docs/18/auth-peer.html).
