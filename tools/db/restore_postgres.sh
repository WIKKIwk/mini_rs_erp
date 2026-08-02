#!/usr/bin/env bash
set -euo pipefail
umask 077

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

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

LOCK_TIMEOUT_MS="${MINI_ERP_RESTORE_LOCK_TIMEOUT_MS:-30000}"
if ! [[ "$LOCK_TIMEOUT_MS" =~ ^[1-9][0-9]*$ ]]; then
	echo "MINI_ERP_RESTORE_LOCK_TIMEOUT_MS must be a positive integer" >&2
	exit 2
fi
# Fail with a job error instead of waiting indefinitely behind an active
# application transaction. Preserve any operator-provided libpq options.
export PGOPTIONS="${PGOPTIONS:-} -c lock_timeout=${LOCK_TIMEOUT_MS}"

# Validate the custom-format dump before touching the target database.
"$PG_RESTORE" --list "$MINI_ERP_RESTORE_DUMP" >/dev/null

# pg_restore has no CASCADE option for its generated DROP statements. Generate
# the SQL stream and execute it through psql so the public schema can be
# replaced with CASCADE inside one transaction. If any object or migration
# step fails, PostgreSQL rolls the complete restore back.
RESTORE_ARGS=(
	--clean
	--if-exists
	--no-owner
	--no-privileges
	--exit-on-error
	--file=-
)

{
	printf '%s\n' \
		'DROP SCHEMA IF EXISTS public CASCADE;' \
		'CREATE SCHEMA public;' \
		'GRANT USAGE ON SCHEMA public TO PUBLIC;'
	"$PG_RESTORE" \
		"${RESTORE_ARGS[@]}" \
		"$MINI_ERP_RESTORE_DUMP"
} | "$PSQL" \
	--no-password \
	--set=ON_ERROR_STOP=1 \
	--single-transaction \
	--dbname="$RESTORE_DATABASE_URL"
