# Full backend recovery

The scheduled/manual backup and `make db-backup` now produce one
`*.accord-backup.tar.gz` recovery archive. Download this archive from the server
monitor. A `.dump` alone cannot reconstruct the backend.

## Coverage

The archive contains:

- A full PostgreSQL custom dump: tables, rows, sequences, constraints, triggers,
  functions, owners, grants, and migration history. The accompanying SQL export
  comes from that same dump, not a second database snapshot.
- Runtime `data/`, including access-code/Telegram keys, roles, Telegram
  configuration/drafts, FCM configuration, sessions, preferences, local media,
  and published Android releases.
- Configured state/credential paths outside `data/`, with relocation bindings.
- Iroh endpoint identity and configured gateway TLS files, so their keys are
  retained when separately restarting the transport/gateway services.
- Transactional LMDB copies and SQLite online backups, including committed WAL
  data. Live LMDB locks and SQLite sidecars are not treated as backups.
- Effective configuration and the original dotenv file. On the configured Mac,
  the application's database password and access-code/FCM key are recovered
  from its Keychain entries. Recovery does not require the old Keychain or
  generate replacement encryption keys.
- Backend source and Cargo.lock, plus the running executable when invoked by
  BackupDoctor. Available migration, LMDB and media helper executables are included.
- Configured R2 `chat_media/` and `profile_avatars/` objects, not just URLs.
  R2 credentials need list/read permissions. An unavailable configured object
  store fails the backup instead of silently omitting files.

Media references from the actual PostgreSQL snapshot are checked against the
copied local files/R2 inventory. If concurrent cleanup removed a required file,
the backup fails and must be retried instead of publishing incomplete media.

The versioned manifest includes SHA-256 and size for every file. Archives are
validated before publication; downloads recheck the outer checksum.
Old database-only backups remain downloadable but do not satisfy the
full-recovery health indicator.

## Build and back up

Build the helper with the backend:

```sh
cargo build --release --locked \
  --bin mini_rs_erp --bin mini_rs_migrate --bin mini_rs_backup_state
```

The runtime launcher also builds the helper when building a release. Deploy
the updated backend and tools to activate the new scheduled/manual format.
Direct execution needs the backend's effective environment and working directory:

```sh
make db-backup
# Configured Mac, including optional cluster role definitions:
python3 tools/db/macos_keychain.py --admin db -- bash tools/db/backup_postgres.sh
```

Directories/files are private (0700/0600). Archives contain credentials and
decrypted runtime secrets; protect them as confidential recovery material.
Copy completed archives and checksums to independently protected storage.
The default local backup directory does not protect against loss of that disk.

## Verify and recover

Use PostgreSQL tools compatible with the source server version, Python 3.9+,
and the runtime dependencies for the backup's OS/architecture. Different
platforms require rebuilding the included source with its Cargo.lock; native
executables and dynamically linked media tools are platform specific.

```sh
python3 tools/db/recovery_bundle.py verify /private/backup.accord-backup.tar.gz
```

Provision a **new, empty PostgreSQL database** with encoding/collation matching
`database/database-info.json` in the archive. Set `MINI_ERP_RESTORE_DATABASE_URL`
to it using an operator credential with permission to create missing application
roles and restore ownership. Supply passwords privately through libpq/your
secret manager. The tool does not replay `globals.sql`, elevate recreated
source roles, or reset existing cluster login passwords.

```sh
python3 tools/db/recovery_bundle.py restore \
  /private/backup.accord-backup.tar.gz \
  --target /srv/accord-recovered
```

The directory must not already exist. Every archive member is verified before
extraction or database mutation. Recovery prepares source/state, relocates
configured paths, restores PostgreSQL in one transaction, and installs the
prepared directory. Missing roles have no administrative privileges; only the
application login receives its recovered password.

The runtime connection retains the original application user/password and uses
the target database's host, port and name. Set
`MINI_ERP_RECOVERY_RUNTIME_DATABASE_URL` to use a separately provisioned login.
The generated `recovery-environment.json` supplies configuration without the
old dotenv/Keychain. The original admin database URL is removed so future
backups cannot accidentally contact the old database.

Stop the original service before starting the recovered one:

```sh
cd /srv/accord-recovered
./run-restored.sh
```

If the executable was unavailable or belongs to another platform:

```sh
cargo build --release --locked --bin mini_rs_erp --bin mini_rs_backup_state
./run-restored.sh
```

Full recovery is offline: a running process owns LMDB handles, cached
configuration and network sessions. Existing in-app `.dump` import remains
database-only. Replacing a nonempty database with the recovery tool requires
explicit `--replace-database --service-stopped` and an independent pre-restore
backup. Prefer a new database and switch services after checking it.

R2 recovery reuses identical objects and uploads missing ones; it refuses
conflicting overwrites. Remote configuration must still identify the intended
buckets, with upload permission when objects need recovery.

## Consistency and verification limits

Online backups capture a PostgreSQL snapshot and transactional snapshots of
each local database. Independent stores/files are copied during the backup
window, not in one cross-system transaction. Stop the backend and integration
workers before backup when a single quiescent checkpoint is required.
In-flight connections and remote multipart uploads may need reconnection/retry.
External Telegram/Google services, LAN equipment and the host OS are not cloned.

`verified` checks integrity and coverage, not whether every individual snapshot
has been booted. The manifest records `restore_tested: false`. Regression tests
restore an isolated PostgreSQL cluster, preserve active-work fixture data and
sequence continuation, compare the current schema's later security rules and
function bodies, and check SQLite WAL, LMDB, R2 and damaged archive handling.

```sh
cargo build --locked --bin mini_rs_backup_state --bin mini_rs_migrate
python3 -m unittest discover -s tools/db -p 'test_recovery*.py' -v
cargo test --locked --lib core::backup_doctor
cargo test --locked --bin mini_rs_backup_state
```
