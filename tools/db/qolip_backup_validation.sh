#!/usr/bin/env bash

# Qolip blocks live in the shared warehouse hierarchy, so the backup contract
# must cover both the warehouse tables and the Qolip-specific tables.
QOLIP_REQUIRED_TABLES=(
	mini_items
	mini_warehouses
	mini_warehouse_assignments
	mini_qolip_product_specs
	mini_qolip_locations
	mini_qolip_cell_qrs
	mini_qolip_checkouts
	mini_qolip_order_notes
)

validate_qolip_database_tables() {
	local psql_bin="$1"
	local database_url="$2"
	local missing

	missing="$(
		"$psql_bin" -X -v ON_ERROR_STOP=1 -At "$database_url" -c "
SELECT COALESCE(string_agg(table_name, ',' ORDER BY table_name), '')
FROM (
    VALUES
        ('mini_items'),
        ('mini_warehouses'),
        ('mini_warehouse_assignments'),
        ('mini_qolip_product_specs'),
        ('mini_qolip_locations'),
        ('mini_qolip_cell_qrs'),
        ('mini_qolip_checkouts'),
        ('mini_qolip_order_notes')
) AS expected(table_name)
WHERE to_regclass('public.' || table_name) IS NULL;
"
	)"

	if [ -n "$missing" ]; then
		echo "required Qolip table(s) missing from source database: $missing" >&2
		return 1
	fi
}

validate_qolip_dump() {
	local pg_restore_bin="$1"
	local dump_path="$2"
	local listing
	local table
	local missing_tables=()
	local missing_data=()

	if ! listing="$("$pg_restore_bin" --list "$dump_path")"; then
		echo "Qolip backup archive cannot be inspected: $dump_path" >&2
		return 1
	fi

	for table in "${QOLIP_REQUIRED_TABLES[@]}"; do
		if ! grep -Eq "TABLE public ${table} " <<<"$listing"; then
			missing_tables+=("$table")
		fi
		if ! grep -Eq "TABLE DATA public ${table} " <<<"$listing"; then
			missing_data+=("$table")
		fi
	done

	if [ "${#missing_tables[@]}" -gt 0 ] || [ "${#missing_data[@]}" -gt 0 ]; then
		echo "Qolip backup coverage validation failed" >&2
		if [ "${#missing_tables[@]}" -gt 0 ]; then
			echo "missing table entries: ${missing_tables[*]}" >&2
		fi
		if [ "${#missing_data[@]}" -gt 0 ]; then
			echo "missing table-data entries: ${missing_data[*]}" >&2
		fi
		return 1
	fi
}
