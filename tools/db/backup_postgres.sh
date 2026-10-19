#!/usr/bin/env bash
set -euo pipefail
umask 077

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
source "$SCRIPT_DIR/qolip_backup_validation.sh"

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
case "$DATABASE_NAME" in
	*[!A-Za-z0-9._-]*|''|.|..)
		echo "database name cannot be used as a safe backup filename" >&2
		exit 1
		;;
esac

validate_qolip_database_tables "$PSQL" "$MINI_ERP_DATABASE_URL"

TIMESTAMP="${MINI_ERP_BACKUP_TIMESTAMP:-$(date +%Y%m%d_%H%M%S)}"
case "$TIMESTAMP" in
	*[!A-Za-z0-9._-]*|''|.|..)
		echo "MINI_ERP_BACKUP_TIMESTAMP contains unsupported characters" >&2
		exit 2
		;;
esac
BACKUP_ROOT="${MINI_ERP_BACKUP_DIR:-$REPO_ROOT/../backups/mini_rs_erp_db}"
BACKUP_DIR="$BACKUP_ROOT/$TIMESTAMP"
mkdir -p "$BACKUP_DIR"
chmod 700 "$BACKUP_DIR"
if ! mkdir "$BACKUP_DIR/.backup-lock" 2>/dev/null; then
	echo "another backup owns this snapshot directory" >&2
	exit 1
fi
trap 'rmdir "$BACKUP_DIR/.backup-lock" 2>/dev/null || true' EXIT

CUSTOM_DUMP="$BACKUP_DIR/$DATABASE_NAME.dump"
PLAIN_DUMP="$BACKUP_DIR/$DATABASE_NAME.sql"
RECOVERY_ARCHIVE="$BACKUP_DIR/$DATABASE_NAME.accord-backup.tar.gz"
if [ -e "$CUSTOM_DUMP" ] || [ -e "$PLAIN_DUMP" ] || [ -e "$RECOVERY_ARCHIVE" ]; then
	echo "refusing to overwrite an existing backup" >&2
	exit 1
fi

"$PG_DUMP" --no-password --format=custom --compress=0 --file="$CUSTOM_DUMP.part" "$MINI_ERP_DATABASE_URL"
validate_qolip_dump "$PG_RESTORE" "$CUSTOM_DUMP.part"
# Derive SQL from this exact archive: two independent pg_dump calls can capture
# different production states. This also reads/decompresses every archive entry.
"$PG_RESTORE" --exit-on-error --file="$PLAIN_DUMP.part" "$CUSTOM_DUMP.part"
mv "$CUSTOM_DUMP.part" "$CUSTOM_DUMP"
mv "$PLAIN_DUMP.part" "$PLAIN_DUMP"

if [ -n "${MINI_ERP_ADMIN_DATABASE_URL:-}" ]; then
	"$PG_DUMPALL" --globals-only --database="$MINI_ERP_ADMIN_DATABASE_URL" \
		--file="$BACKUP_DIR/globals.sql"
fi

# Role names/ownership/ACL dependencies are needed on a new PostgreSQL cluster.
# Do not export unrelated cluster accounts or grant them administrative powers.
"$PSQL" -X -w -v ON_ERROR_STOP=1 -At "$MINI_ERP_DATABASE_URL" > "$BACKUP_DIR/database-roles.json" <<'SQL'
WITH namespaces AS (
    SELECT oid FROM pg_namespace
    WHERE nspname NOT IN ('pg_catalog','information_schema') AND nspname NOT LIKE 'pg_toast%'
), role_oids AS (
    SELECT relowner AS oid FROM pg_class WHERE relnamespace IN (SELECT oid FROM namespaces)
    UNION SELECT proowner FROM pg_proc WHERE pronamespace IN (SELECT oid FROM namespaces)
    UNION SELECT nspowner FROM pg_namespace WHERE oid IN (SELECT oid FROM namespaces)
    UNION SELECT datdba FROM pg_database WHERE datname=current_database()
    UNION SELECT a.grantee FROM pg_class c CROSS JOIN LATERAL aclexplode(c.relacl) a
          WHERE c.relnamespace IN (SELECT oid FROM namespaces)
    UNION SELECT a.grantee FROM pg_proc p CROSS JOIN LATERAL aclexplode(p.proacl) a
          WHERE p.pronamespace IN (SELECT oid FROM namespaces)
    UNION SELECT a.grantee FROM pg_namespace n CROSS JOIN LATERAL aclexplode(n.nspacl) a
          WHERE n.oid IN (SELECT oid FROM namespaces)
    UNION SELECT defaclrole FROM pg_default_acl
    UNION SELECT a.grantee FROM pg_default_acl d CROSS JOIN LATERAL aclexplode(d.defaclacl) a
    UNION SELECT typowner FROM pg_type WHERE typnamespace IN (SELECT oid FROM namespaces)
    UNION SELECT lomowner FROM pg_largeobject_metadata
)
SELECT COALESCE(json_agg(rolname ORDER BY rolname), '[]'::json)
FROM pg_roles WHERE oid IN (SELECT oid FROM role_oids) AND rolname !~ '^pg_';
SQL
"$PSQL" -X -w -v ON_ERROR_STOP=1 -At "$MINI_ERP_DATABASE_URL" > "$BACKUP_DIR/database-info.json" <<'SQL'
SELECT json_build_object('encoding', pg_encoding_to_char(encoding),
    'collate', datcollate, 'ctype', datctype, 'server_version', current_setting('server_version'))
FROM pg_database WHERE datname=current_database();
SQL

cat > "$BACKUP_DIR/backup.meta" <<EOF
database=$DATABASE_NAME
created_at_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)
postgres_client_version=$($PG_DUMP --version)
custom_dump=$(basename "$CUSTOM_DUMP")
plain_dump=$(basename "$PLAIN_DUMP")
qolip_tables_validated=true
scope=full_recovery
EOF

python3 "$SCRIPT_DIR/recovery_bundle.py" create \
	--root "$REPO_ROOT" \
	--runtime "${MINI_ERP_BACKUP_RUNTIME_ROOT:-$PWD}" \
	--dump "$CUSTOM_DUMP" \
	--output "$RECOVERY_ARCHIVE"

(
	cd "$BACKUP_DIR"
	CHECKSUM_FILES=(
		"$(basename "$CUSTOM_DUMP")"
		"$(basename "$PLAIN_DUMP")"
		"backup.meta"
		"database-roles.json"
		"database-info.json"
		"$(basename "$RECOVERY_ARCHIVE")"
		"$(basename "$RECOVERY_ARCHIVE").json"
	)
	if [ -f "globals.sql" ]; then
		CHECKSUM_FILES+=("globals.sql")
	fi
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum "${CHECKSUM_FILES[@]}" > SHA256SUMS
	else
		shasum -a 256 "${CHECKSUM_FILES[@]}" > SHA256SUMS
	fi
)

echo "$BACKUP_DIR"
