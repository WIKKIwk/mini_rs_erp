#!/usr/bin/env bash
set -euo pipefail
umask 077

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

if [ -z "${MINI_ERP_DATABASE_URL:-}" ]; then
	echo "MINI_ERP_DATABASE_URL is required" >&2
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

PG_DUMP="$(find_pg_tool pg_dump)"
PG_RESTORE="$(find_pg_tool pg_restore)"
PSQL="$(find_pg_tool psql)"
PG_DUMPALL="$(find_pg_tool pg_dumpall)"

DATABASE_NAME="$($PSQL -X -At "$MINI_ERP_DATABASE_URL" -c 'SELECT current_database()')"
if [ -z "$DATABASE_NAME" ]; then
	echo "could not read database name" >&2
	exit 1
fi

TIMESTAMP="$(date +%Y%m%d_%H%M%S)"
BACKUP_ROOT="${MINI_ERP_BACKUP_DIR:-$REPO_ROOT/../backups/mini_rs_erp_db}"
BACKUP_DIR="$BACKUP_ROOT/$TIMESTAMP"
mkdir -p "$BACKUP_DIR"

CUSTOM_DUMP="$BACKUP_DIR/$DATABASE_NAME.dump"
PLAIN_DUMP="$BACKUP_DIR/$DATABASE_NAME.sql"

"$PG_DUMP" --format=custom --compress=0 --file="$CUSTOM_DUMP" "$MINI_ERP_DATABASE_URL"
"$PG_DUMP" --format=plain --file="$PLAIN_DUMP" "$MINI_ERP_DATABASE_URL"
"$PG_RESTORE" --list "$CUSTOM_DUMP" >/dev/null

if [ -n "${MINI_ERP_ADMIN_DATABASE_URL:-}" ]; then
	"$PG_DUMPALL" --globals-only --database="$MINI_ERP_ADMIN_DATABASE_URL" \
		--file="$BACKUP_DIR/globals.sql"
fi

cat > "$BACKUP_DIR/backup.meta" <<EOF
database=$DATABASE_NAME
created_at_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)
postgres_client_version=$($PG_DUMP --version)
custom_dump=$(basename "$CUSTOM_DUMP")
plain_dump=$(basename "$PLAIN_DUMP")
EOF

(
	cd "$BACKUP_DIR"
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum ./* > SHA256SUMS
	else
		shasum -a 256 ./* > SHA256SUMS
	fi
)

echo "$BACKUP_DIR"
