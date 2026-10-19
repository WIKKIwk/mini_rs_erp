#!/usr/bin/env bash
set -euo pipefail
umask 077

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
source "$SCRIPT_DIR/qolip_backup_validation.sh"

# A configured Mac obtains maintenance credentials only inside the restore job.
# Ordinary HTTP startup continues to receive just the runtime password.
KEYCHAIN_MIGRATE_AFTER_RESTORE=0
if [ "$(uname -s)" = "Darwin" ] && [ -n "${MINI_ERP_DATABASE_KEYCHAIN_SERVICE:-}" ]; then
	if [ "${MINI_ERP_KEYCHAIN_MAINTENANCE_ACTIVE:-}" != "1" ]; then
		exec python3 "$SCRIPT_DIR/macos_keychain.py" --admin db -- bash "$0" "$@"
	fi
	case "${MINI_ERP_AUTO_MIGRATE_AFTER_RESTORE:-1}" in
		0|false|FALSE|off|OFF|no|NO) ;;
		*)
			if [ ! -x "$REPO_ROOT/target/release/mini_rs_migrate" ]; then
				echo "build mini_rs_migrate before a Keychain-managed restore" >&2
				exit 2
			fi
			KEYCHAIN_MIGRATE_AFTER_RESTORE=1
			;;
	esac
fi

RESTORE_DATABASE_URL="${MINI_ERP_RESTORE_DATABASE_URL:-${MINI_ERP_MIGRATION_DATABASE_URL:-${MINI_ERP_DATABASE_URL:-}}}"
if [ -z "$RESTORE_DATABASE_URL" ]; then
	echo "MINI_ERP_DATABASE_URL or MINI_ERP_RESTORE_DATABASE_URL is required" >&2
	exit 2
fi

if [ -z "${MINI_ERP_RESTORE_DUMP:-}" ] || [ ! -f "$MINI_ERP_RESTORE_DUMP" ]; then
	echo "MINI_ERP_RESTORE_DUMP must point to an existing dump" >&2
	exit 2
fi

find_pg_tool() {
	local name="$1"
	if command -v "$name" >/dev/null 2>&1; then
		command -v "$name"
		return
	fi
	local candidate
	for candidate in "$REPO_ROOT"/../.tools/postgres/*/bin/"$name"; do
		if [ -x "$candidate" ]; then
			printf '%s\n' "$candidate"
			return
		fi
	done
	echo "required PostgreSQL tool not found: $name" >&2
	exit 1
}

PG_RESTORE="$(find_pg_tool pg_restore)"
PSQL="$(find_pg_tool psql)"

# Fail before DROP SCHEMA if the caller supplied the ordinary HTTP credential.
# A maintenance connection must own the target database (or be superuser).
"$PSQL" --no-password --set=ON_ERROR_STOP=1 --dbname="$RESTORE_DATABASE_URL" \
	--command="DO \$\$ BEGIN
	IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname=current_user AND rolsuper)
	AND NOT pg_has_role(current_user,
	    (SELECT datdba FROM pg_database WHERE datname=current_database()), 'USAGE') THEN
	    RAISE EXCEPTION 'restore requires the maintenance/database-owner credential';
	END IF; END \$\$;" >/dev/null

LOCK_TIMEOUT_MS="${MINI_ERP_RESTORE_LOCK_TIMEOUT_MS:-30000}"
if ! [[ "$LOCK_TIMEOUT_MS" =~ ^[1-9][0-9]*$ ]]; then
	echo "MINI_ERP_RESTORE_LOCK_TIMEOUT_MS must be a positive integer" >&2
	exit 2
fi
# Fail with a job error instead of waiting indefinitely behind an active
# application transaction. Preserve any operator-provided libpq options.
export PGOPTIONS="${PGOPTIONS:-} -c lock_timeout=${LOCK_TIMEOUT_MS}"

# Validate the custom-format dump before touching the target database.
validate_qolip_dump "$PG_RESTORE" "$MINI_ERP_RESTORE_DUMP"

# Decode the ENTIRE archive before DROP SCHEMA. A failing pg_restore on the
# producer side of a pipe could otherwise let psql commit a truncated stream.
RESTORE_WORK="$(mktemp -d "${TMPDIR:-/tmp}/mini-erp-restore.XXXXXXXX")"
trap 'rm -rf "$RESTORE_WORK"' EXIT
RESTORE_ARGS=(
	--clean
	--if-exists
	--exit-on-error
	"--file=$RESTORE_WORK/archive.sql"
)
"$PG_RESTORE" "${RESTORE_ARGS[@]}" "$MINI_ERP_RESTORE_DUMP"

{
	printf '%s\n' \
		'DROP SCHEMA IF EXISTS public CASCADE;' \
		'CREATE SCHEMA public;' \
		'GRANT USAGE ON SCHEMA public TO PUBLIC;'
	cat "$RESTORE_WORK/archive.sql"
	# Preserve the archive's owners, column grants and SECURITY DEFINER bodies.
	# Replaying 0104 here would undo later migrations (including auth ACLs and
	# the template-safe order reset function). Database ownership is not part
	# of an ordinary pg_dump, so reestablish only that database-level boundary.
	printf '%s\n' \
		"SELECT EXISTS (SELECT 1 FROM public.mini_schema_migrations WHERE version='0104_runtime_security_boundaries') AS restore_hardened \\gset" \
		'\if :restore_hardened'
	cat <<'SQL'
DO $$
BEGIN
    EXECUTE format('ALTER DATABASE %I OWNER TO mini_rs_erp_owner', current_database());
    EXECUTE format('REVOKE CREATE ON DATABASE %I FROM mini_rs_erp, PUBLIC', current_database());
    EXECUTE format('GRANT CONNECT, TEMPORARY ON DATABASE %I TO mini_rs_erp', current_database());
END;
$$;
SQL
	printf '\n%s\n' '\endif'
} > "$RESTORE_WORK/restore.sql"
"$PSQL" \
	--no-password \
	--set=ON_ERROR_STOP=1 \
	--single-transaction \
	--dbname="$RESTORE_DATABASE_URL" \
	--file="$RESTORE_WORK/restore.sql"

if [ "$KEYCHAIN_MIGRATE_AFTER_RESTORE" = "1" ]; then
	python3 "$SCRIPT_DIR/macos_keychain.py" migrate
fi
