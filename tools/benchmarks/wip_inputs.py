#!/usr/bin/env python3
"""Benchmark the stage-input SQL bottleneck on connection-local fixtures.

Run through tools/db/macos_keychain.py db; no credentials are printed and no
existing application rows are read or written. psycopg 3 is a tool-only optional
dependency. This is a SQL component benchmark, not printer or HTTP latency.
"""

import argparse
import hashlib
import json
import math
import platform
import re
from pathlib import Path
import statistics
import time
import tracemalloc

import psycopg
from psycopg.types.json import Jsonb


# Snapshot of the previous load_progress_batch SQL, deliberately retained so
# both implementations can be rerun against identical fixtures after edits.
FULL_BATCH_SQL = """
SELECT batch.batch_id, batch.revision, batch.session_id,
       COALESCE(EXTRACT(EPOCH FROM session.started_at)::bigint,
                EXTRACT(EPOCH FROM batch.created_at)::bigint) AS started_at_unix,
       COALESCE(EXTRACT(EPOCH FROM session.session_updated_at)::bigint,
                EXTRACT(EPOCH FROM batch.updated_at)::bigint) AS completed_at_unix,
       batch.canonical_apparatus_id AS apparatus, batch.order_id, batch.action, batch.status,
       produced_qty::float8 AS produced_qty, uom, qr_payload,
       label_item_code, label_item_name, executor_name,
       worker_role, worker_ref, worker_display_name,
       wip_status, COALESCE(batch.canonical_current_apparatus_id, '') AS current_apparatus,
       current_location, COALESCE(batch.canonical_next_apparatus_id, '') AS next_apparatus,
       parent_batch_id, used_by_session_id,
       COALESCE(batch.canonical_used_by_apparatus_id, '') AS used_by_apparatus,
       processed_by_session_id,
       COALESCE(batch.canonical_processed_by_apparatus_id, '') AS processed_by_apparatus,
       return_ink_kg::float8 AS return_ink_kg,
       lamination_print_leftover_rolls::float8 AS lamination_print_leftover_rolls,
       lamination_film_leftover_rolls::float8 AS lamination_film_leftover_rolls,
       rezka_bosma_waste::float8 AS rezka_bosma_waste,
       rezka_lamination_waste::float8 AS rezka_lamination_waste,
       rezka_edge_waste::float8 AS rezka_edge_waste,
       total_waste::float8 AS total_waste, finished_goods_kg::float8 AS finished_goods_kg,
       bobina_kg::float8 AS bobina_kg, finished_goods_meter::float8 AS finished_goods_meter,
       diameter::float8 AS diameter, description, payload_json
FROM mini_progress_batches AS batch
LEFT JOIN (
    SELECT session_id, started_at, updated_at AS session_updated_at
    FROM mini_order_run_sessions
) AS session ON session.session_id = batch.session_id
WHERE batch.batch_id = %s
"""

OPENING_SQL = """
SELECT CASE WHEN i.source_apparatus <> '' THEN i.resume_stage_node_id ELSE '' END,
       CASE WHEN i.source_apparatus = '' THEN i.resume_stage_node_id ELSE '' END,
       i.source_apparatus, COALESCE(i.resume_apparatus, ''), b.wip_status = 'waiting'
FROM mini_opening_wip_intakes i JOIN mini_opening_wip_batches b ON b.intake_id = i.intake_id
WHERE i.order_id = %s AND i.status = 'confirmed' AND b.wip_status IN ('waiting', 'in_use')
"""

ORDER = "benchmark-only-order"
APPARATUS = "apparatus:benchmark:rezka"

OLD_TOTALS_SQL = """
SELECT count(*) FILTER (WHERE wip_status='waiting' AND COALESCE(canonical_next_apparatus_id,'')='')::BIGINT,
       count(*) FILTER (WHERE wip_status='waiting' AND COALESCE(canonical_next_apparatus_id,'')<>'')::BIGINT,
       count(*) FILTER (WHERE wip_status='in_use')::BIGINT,
       count(*) FILTER (WHERE wip_status='processed' AND lower(COALESCE(processed_by_apparatus,'')) LIKE 'warehouse:%%')::BIGINT
FROM mini_progress_batches WHERE order_id=%s
"""

NEW_TOTALS_SQL = """
SELECT free_wip_count,waiting_next_stage_count,in_use_wip_count,accepted_wip_count
FROM mini_progress_batch_lifecycle_totals WHERE order_id=%s
"""


def fixtures(connection, count, payload_bytes):
    with connection.transaction():
        connection.execute("""
            CREATE TEMP TABLE mini_progress_batches
                (LIKE public.mini_progress_batches INCLUDING DEFAULTS INCLUDING CONSTRAINTS INCLUDING INDEXES);
            CREATE TEMP TABLE mini_order_run_sessions (session_id TEXT PRIMARY KEY,
                started_at TIMESTAMPTZ, updated_at TIMESTAMPTZ);
            CREATE TEMP TABLE mini_production_maps (id TEXT PRIMARY KEY, map_json JSONB NOT NULL);
            CREATE TEMP TABLE mini_opening_wip_intakes (intake_id TEXT, order_id TEXT, status TEXT,
                source_apparatus TEXT, resume_apparatus TEXT, resume_stage_node_id TEXT);
            CREATE TEMP TABLE mini_opening_wip_batches (intake_id TEXT, wip_status TEXT);
        """)
        # Explicit pg_temp search path guarantees writes cannot reach public
        # even if a future query introduces an uncreated fixture table.
        connection.execute("SET search_path = pg_temp")
        connection.execute("SET statement_timeout = '120s'")
        connection.execute("INSERT INTO mini_order_run_sessions VALUES ('benchmark-session',now(),now())")
        connection.execute("INSERT INTO mini_production_maps VALUES (%s,%s)",
                           (ORDER, Jsonb({"id": ORDER, "product_code": "BENCH", "title": "Benchmark",
                            "nodes": [{"id": "start", "kind": "start", "title": "Start"},
                                      {"id": "cut", "kind": "apparatus", "title": "Cut", "apparatus_id": APPARATUS},
                                      {"id": "lam", "kind": "apparatus", "title": "Lam", "apparatus_id": "apparatus:benchmark:lam"},
                                      {"id": "end", "kind": "end", "title": "End"}],
                            "edges": [{"from": "start", "to": "cut"}, {"from": "cut", "to": "lam"},
                                      {"from": "lam", "to": "end"}]})))
        payload = {"stage_node_id": "cut", "next_stage_node_id": "lam", "benchmark_padding": "x" * payload_bytes}
        connection.execute("""
            INSERT INTO mini_progress_batches
                (batch_id,session_id,apparatus,canonical_apparatus_id,order_id,action,status,
                 produced_qty,uom,qr_payload,label_item_code,label_item_name,wip_status,
                 canonical_next_apparatus_id,next_apparatus,payload_json)
            SELECT 'bench-' || lpad(i::text,10,'0'), 'benchmark-session', %s, %s, %s,
                   'roll_complete','completed',1,'kg','benchmark-qr-' || i,'BENCH','Benchmark',
                   CASE WHEN i %% 5 = 0 THEN 'in_use' ELSE 'waiting' END,
                   'apparatus:benchmark:lam','apparatus:benchmark:lam',%s
            FROM generate_series(1,%s) i
        """, (APPARATUS, APPARATUS, ORDER, Jsonb(payload), count))
        connection.execute("ANALYZE mini_progress_batches")
        connection.execute("ANALYZE mini_order_run_sessions")


def old_inputs(connection):
    with connection.transaction():
        cursor = connection.cursor()
        cursor.execute("SELECT map_json FROM mini_production_maps WHERE id=%s", (ORDER,))
        cursor.fetchone()
        cursor.execute("SELECT batch_id FROM mini_progress_batches WHERE order_id=%s "
                       "AND wip_status IN ('waiting','in_use') ORDER BY batch_id", (ORDER,))
        ids = [row[0] for row in cursor.fetchall()]
        batches = []
        for batch_id in ids:
            cursor.execute(FULL_BATCH_SQL, (batch_id,))
            batches.append(cursor.fetchone())
        cursor.execute(OPENING_SQL, (ORDER,))
        opening = cursor.fetchall()
        return batches, opening, len(ids) + 3


def new_inputs(connection, statements):
    with connection.transaction():
        batches, opening = [], []
        for sql in statements:
            cursor = connection.execute(sql, (ORDER,))
            rows = cursor.fetchall()
            if "mini_progress_batches" in sql or "mini_progress_batch_work_inputs" in sql:
                batches = rows
            elif "mini_opening_wip" in sql:
                opening = rows
        return batches, opening, len(statements)


def totals(connection, projected):
    with connection.transaction():
        rows = connection.execute(NEW_TOTALS_SQL if projected else OLD_TOTALS_SQL, (ORDER,)).fetchall()
        return rows, [], 1


def write_rollback(connection, action, payload_bytes, verify=False):
    with connection.transaction():
        if action == "insert":
            connection.execute("""
                INSERT INTO mini_progress_batches
                    (batch_id,session_id,apparatus,canonical_apparatus_id,order_id,action,status,
                     produced_qty,uom,qr_payload,label_item_code,label_item_name,wip_status,
                     canonical_next_apparatus_id,next_apparatus,payload_json)
                VALUES ('benchmark-extra','benchmark-session',%s,%s,%s,'roll_complete','completed',
                    1,'kg','benchmark-extra-qr','BENCH','Benchmark','waiting',
                    'apparatus:benchmark:lam','apparatus:benchmark:lam',%s)
            """, (APPARATUS, APPARATUS, ORDER, Jsonb({"stage_node_id": "cut", "next_stage_node_id": "lam",
                                                     "benchmark_padding": "x" * payload_bytes})))
        elif action == "update":
            connection.execute("UPDATE mini_progress_batches SET wip_status='in_use' WHERE batch_id='bench-0000000001'")
        else:
            connection.execute("DELETE FROM mini_progress_batches WHERE batch_id='bench-0000000001'")
        if verify:
            expected = connection.execute(OLD_TOTALS_SQL, (ORDER,)).fetchone()
            actual = connection.execute(NEW_TOTALS_SQL, (ORDER,)).fetchone()
            assert expected == actual, (action, expected, actual)
            expected_routes = connection.execute("""
                SELECT pg_temp.mini_progress_batch_work_route(b) AS route_json,count(*)
                FROM mini_progress_batches b WHERE order_id=%s AND wip_status IN ('waiting','in_use')
                GROUP BY route_json ORDER BY route_json
            """, (ORDER,)).fetchall()
            actual_routes = connection.execute("""
                SELECT route_json,batch_count FROM mini_progress_batch_work_inputs
                WHERE order_id=%s ORDER BY route_json
            """, (ORDER,)).fetchall()
            assert expected_routes == actual_routes, (action, "route class counts differ")
        # Measure the write and its lock/trigger cost without accumulating data.
        raise psycopg.Rollback()
    return [], [], 1


def apply_projection(connection, migration):
    # The fixture is populated before backfill to exercise its true migration
    # path. Functions/triggers and tables exist only in this connection's
    # pg_temp namespace; no public objects are changed.
    sql = migration.read_text()
    if "public." in sql.lower() or "search_path" in sql.lower():
        raise ValueError("This harness requires unqualified projection objects inheriting pg_temp search_path; "
                         "use an isolated disposable database for schema-qualified migrations")
    sql = sql.replace("CREATE TABLE IF NOT EXISTS", "CREATE TEMP TABLE IF NOT EXISTS")
    sql = sql.replace("CREATE TABLE ", "CREATE TEMP TABLE ")
    # PostgreSQL resolves temporary relations/types through search_path, but
    # never temporary functions. Qualify fixture function definitions and calls.
    function_names = re.findall(r"CREATE FUNCTION\s+([a-zA-Z_][a-zA-Z_0-9]*)\s*\(", sql)
    for name in function_names:
        sql = re.sub(r"\b" + re.escape(name) + r"\b", "pg_temp." + name, sql)
    started = time.perf_counter_ns()
    with connection.transaction():
        connection.execute(sql)
    elapsed = (time.perf_counter_ns() - started) / 1e6
    expected = connection.execute(OLD_TOTALS_SQL, (ORDER,)).fetchone()
    actual = connection.execute(NEW_TOTALS_SQL, (ORDER,)).fetchone()
    if expected != actual:
        raise AssertionError(("projection backfill counters differ", expected, actual))
    before = connection.execute("SELECT route_key,route_json,batch_count FROM mini_progress_batch_work_inputs "
                                "WHERE order_id=%s ORDER BY route_key", (ORDER,)).fetchall()
    for action in ("insert", "update", "delete"):
        write_rollback(connection, action, 0, verify=True)
        assert connection.execute(NEW_TOTALS_SQL, (ORDER,)).fetchone() == expected, action
        after = connection.execute("SELECT route_key,route_json,batch_count FROM mini_progress_batch_work_inputs "
                                   "WHERE order_id=%s ORDER BY route_key", (ORDER,)).fetchall()
        assert before == after, action
    return round(elapsed, 3)


def summary(name, operation, repetitions):
    times = []
    first = None
    for _ in range(repetitions + 1):
        start = time.perf_counter_ns()
        batches, opening, queries = operation()
        duration = (time.perf_counter_ns() - start) / 1e6
        if first is None:
            first = duration
        else:
            times.append(duration)
    # Trace memory in a separate untimed repetition to avoid distorting latency.
    del batches, opening
    tracemalloc.start()
    batches, opening, queries = operation()
    _, peak = tracemalloc.get_traced_memory()
    tracemalloc.stop()
    return {"name": name, "first_run_ms": round(first, 3), "warm_samples_ms": [round(t, 3) for t in times],
            "median_ms": round(statistics.median(times), 3),
            "p95_ms": round(sorted(times)[math.ceil(.95 * len(times)) - 1], 3),
            "sql_client_statements": queries, "rows_materialized": len(batches) + len(opening),
            "python_peak_allocated_bytes": peak}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sizes", nargs="+", type=int, default=[10,100,1000,10000])
    parser.add_argument("--repetitions", type=int, default=7)
    parser.add_argument("--payload-bytes", type=int, default=1024)
    parser.add_argument("--mode", choices=["before", "after", "both"], default="before")
    parser.add_argument("--new-sql-json", type=Path,
                        help="JSON array containing the exact new inputs_tx SELECT statements; $1 is accepted")
    parser.add_argument("--new-source", type=Path,
                        help="Extract actual inputs_tx SELECT strings from stage_execution.rs")
    parser.add_argument("--migration", type=Path, help="Transactional work-input/counter projection migration")
    parser.add_argument("--write-repetitions", type=int, default=21)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    max_count = 1000000 if args.mode == "after" and args.payload_bytes == 0 else 100000
    if args.repetitions < 1 or not all(0 < n <= max_count for n in args.sizes):
        parser.error("use bounded positive fixture sizes <=100000; after-only with zero padding permits <=1000000")
    if args.mode != "before" and not (args.new_sql_json or args.new_source):
        parser.error("after/both requires --new-sql-json or --new-source")
    if args.new_sql_json and args.new_source:
        parser.error("provide either --new-sql-json or --new-source")
    if args.mode != "before" and not args.migration:
        parser.error("after/both requires --migration")
    if args.new_sql_json:
        statements = json.loads(args.new_sql_json.read_text())
    elif args.new_source:
        source = args.new_source.read_text()
        start = source.index("pub(super) async fn inputs_tx(")
        end = source.index("fn route_projection_batch(", start)
        statements = re.findall(r'"(SELECT[\s\S]*?)"(?=\s*,?\s*\))', source[start:end])
        if len(statements) != 3:
            parser.error("expected three literal inputs_tx SELECT statements; use explicit --new-sql-json for other layouts")
    else:
        statements = []
    statement_hash = hashlib.sha256(json.dumps(statements, sort_keys=True).encode()).hexdigest()
    statements = [sql.replace("$1", "%s") for sql in statements]
    result = {"scope": "PostgreSQL stage-input SQL component; no HTTP, Rust projection, printer or disk cache flush",
              "fixture": "connection-local temporary tables; same local ERP PostgreSQL; 80% waiting/20% in_use; one route",
              "platform": platform.platform(), "payload_padding_bytes": args.payload_bytes,
              "psycopg_version": psycopg.__version__,
              "new_sql_sha256": statement_hash if statements else None,
              "migration_sha256": hashlib.sha256(args.migration.read_bytes()).hexdigest() if args.migration else None,
              "repetitions": args.repetitions, "measurements": []}
    for count in args.sizes:
        with psycopg.connect(autocommit=True) as connection:
            result["postgres_version"] = connection.execute("SHOW server_version").fetchone()[0]
            fixtures(connection, count, args.payload_bytes)
            measurements = {"wip_rows": count}
            if args.mode in ("before", "both"):
                measurements["before"] = summary("previous N+1 full batch loading", lambda: old_inputs(connection), args.repetitions)
                measurements["before_counters"] = summary("previous full-table counters", lambda: totals(connection, False), args.repetitions)
                measurements["before_writes"] = {action: summary(action + " without projection, rolled back",
                    lambda action=action: write_rollback(connection, action, args.payload_bytes), args.write_repetitions)
                    for action in ("insert", "update", "delete")}
            if args.mode in ("after", "both"):
                measurements["projection_backfill_ms"] = apply_projection(connection, args.migration)
                measurements["after"] = summary("new source SQL", lambda: new_inputs(connection, statements), args.repetitions)
                measurements["after_counters"] = summary("projected counters", lambda: totals(connection, True), args.repetitions)
                measurements["after_writes"] = {action: summary(action + " with projection, rolled back",
                    lambda action=action: write_rollback(connection, action, args.payload_bytes), args.write_repetitions)
                    for action in ("insert", "update", "delete")}
                measurements["projection_backfill_and_rollback_checks"] = "passed"
            result["measurements"].append(measurements)
            compact = {"wip_rows": count}
            for name, measurement in measurements.items():
                if isinstance(measurement, dict) and "median_ms" in measurement:
                    compact[name] = {key: measurement[key] for key in
                                     ("median_ms", "p95_ms", "sql_client_statements", "rows_materialized")}
                elif name.endswith("_writes"):
                    compact[name + "_median_ms"] = {action: item["median_ms"] for action, item in measurement.items()}
                else:
                    compact[name] = measurement
            print(json.dumps(compact), flush=True)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n")


if __name__ == "__main__":
    main()
