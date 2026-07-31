#!/usr/bin/env bash
set -euo pipefail
umask 077

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

if [ -z "${MINI_ERP_DATABASE_URL:-}" ]; then
	echo "MINI_ERP_DATABASE_URL is required" >&2
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

# Pick the restore mode from the archive rather than blindly creating database
# sessions. Small dumps finish faster and more safely in one transaction;
# larger custom archives can use pg_restore's parallel workers.
ARCHIVE_SIZE_BYTES="$(wc -c < "$MINI_ERP_RESTORE_DUMP")"
ARCHIVE_SIZE_BYTES="${ARCHIVE_SIZE_BYTES//[[:space:]]/}"
TOC_ENTRIES="$("$PG_RESTORE" --list "$MINI_ERP_RESTORE_DUMP" | awk -F: '/^;[[:space:]]+TOC Entries:/ {gsub(/[[:space:]]/, "", $2); print $2; exit}')"
TOC_ENTRIES="${TOC_ENTRIES:-0}"

if ! [[ "$ARCHIVE_SIZE_BYTES" =~ ^[0-9]+$ && "$TOC_ENTRIES" =~ ^[0-9]+$ ]]; then
	echo "could not determine restore archive size or table of contents" >&2
	exit 1
fi

RESTORE_JOBS="${MINI_ERP_RESTORE_JOBS:-}"
if [ -z "$RESTORE_JOBS" ]; then
	if [ "$ARCHIVE_SIZE_BYTES" -lt $((256 * 1024 * 1024)) ] || [ "$TOC_ENTRIES" -lt 128 ]; then
		RESTORE_JOBS=1
	else
		CPU_COUNT="$(getconf _NPROCESSORS_ONLN 2>/dev/null || printf '1')"
		if ! [[ "$CPU_COUNT" =~ ^[0-9]+$ ]] || [ "$CPU_COUNT" -lt 1 ]; then
			CPU_COUNT=1
		fi
		# Eight workers is a conservative automatic ceiling. Operators can
		# override it with MINI_ERP_RESTORE_JOBS after measuring their server.
		RESTORE_JOBS="$CPU_COUNT"
		[ "$RESTORE_JOBS" -gt 8 ] && RESTORE_JOBS=8
	fi
fi

if ! [[ "$RESTORE_JOBS" =~ ^[1-9][0-9]*$ ]]; then
	echo "MINI_ERP_RESTORE_JOBS must be a positive integer" >&2
	exit 2
fi

# Restore only database objects; ownership and cluster-wide globals are not
# taken from an uploaded file.
RESTORE_ARGS=(
	--clean
	--if-exists
	--no-owner
	--no-privileges
	--exit-on-error
	--verbose
	--no-password
	--dbname="$MINI_ERP_DATABASE_URL"
)
if [ "$RESTORE_JOBS" -gt 1 ]; then
	RESTORE_ARGS+=(--jobs="$RESTORE_JOBS")
else
	# pg_restore cannot combine multiple jobs with --single-transaction.
	# The serial path is the default for this app's small dumps and gives
	# all-or-nothing rollback when an object fails.
	RESTORE_ARGS+=(--single-transaction)
fi

"$PG_RESTORE" \
	"${RESTORE_ARGS[@]}" \
	"$MINI_ERP_RESTORE_DUMP"
