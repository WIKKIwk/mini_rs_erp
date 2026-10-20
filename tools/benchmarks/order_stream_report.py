#!/usr/bin/env python3
"""Render sanitized measured order-stream results; does not read private logs."""
import argparse
import json
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-dir", required=True)
    parser.add_argument("--output-dir", required=True)
    parser.add_argument("--date", required=True)
    args = parser.parse_args()
    source, output = Path(args.run_dir), Path(args.output_dir)
    output.mkdir(parents=True, exist_ok=True)
    results = {
        mode: json.loads((source / (mode + "-results.json")).read_text())
        for mode in ("before", "after")
    }
    diagnostics = {
        mode: json.loads((source / (mode + "-sql-counts.json")).read_text())
        for mode in ("before", "after")
    }
    crossprocess = json.loads((source / "crossprocess-results.json").read_text())
    assert crossprocess["independent_os_processes"] == 2
    assert crossprocess["restored_original_order"]
    for mode, result in results.items():
        assert result["fanout"] and result["reorder"]["restored_original_order"]
        assert result["notifications"]
        assert all(
            row.get("exact_rest_view_match", row.get("exact_rest_page_match"))
            for row in result["correctness"]
        ), "Failed correctness result"
        assert result["server"]["query_log"] is False
        assert diagnostics[mode]["server"]["query_log"] is True
    before, after = results["before"], results["after"]
    assert before["dataset_dump_sha256"] == after["dataset_dump_sha256"]
    assert before["fixture_roles"] == after["fixture_roles"]
    assert before["host"] == after["host"]
    phases = {
        mode: {phase["name"]: phase for phase in result["phases"]}
        for mode, result in results.items()
    }
    lines = [
        "# Order stream: measured local before/after",
        "",
        "Date: " + args.date + ". These are real release-server, HTTP/WebSocket and "
        "PostgreSQL measurements. They are not a production capacity or phone "
        "battery claim.",
        "",
        "## Revisions and dataset",
        "",
        "| Item | Value |",
        "|---|---|",
        "| Backend before | `" + before["server"]["source_revision"] + "` |",
        "| Backend after | `" + after["server"]["source_revision"] + "` |",
        "| Before binary SHA-256 | `" + before["server"]["binary_sha256"] + "` |",
        "| After binary SHA-256 | `" + after["server"]["binary_sha256"] + "` |",
        "| Input dump SHA-256 | `" + before["dataset_dump_sha256"] + "` |",
        "| Source maps / workers | %s / %s |" % (
            before["source_counts"]["production_maps"], before["source_counts"]["workers"]
        ),
        "| Host | %s; %s logical CPUs; %.0f GiB RAM |" % (
            before["host"]["platform"], before["host"]["logical_cpus"],
            before["host"]["ram_bytes"] / 2**30,
        ),
        "| Database / connection pool | Local PostgreSQL 18.4 / 16 per server |",
        "",
        "Both versions use fresh clones of the same dump. Original business rows "
        "and original credentials were not changed. Temporary clone credentials "
        "and two clone-only worker apparatus assignments allow three real worker "
        "principals to share one list; eight other apparatus scopes remain separate.",
        "",
        "## Status/control update fanout",
        "",
        "Five hold/release cycles = ten actual mutation events per scenario. "
        "Latency is the time until all clients apply a substantive event, starting "
        "before the mutation HTTP request. Bootstrap and HTTP correctness oracles "
        "are outside these phases.",
        "",
        "| Clients / principals | Before CPU ms | After CPU ms | CPU change | "
        "Before p95 ms | After p95 ms | Before WS bytes | After WS bytes |",
        "|---|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for b, a in zip(before["fanout"], after["fanout"]):
        assert b["name"] == a["name"] and b["cycles"] == a["cycles"]
        bp, ap = phases["before"][b["name"]], phases["after"][a["name"]]
        cpu = (ap["server_cpu_ms"] / bp["server_cpu_ms"] - 1) * 100
        lines.append("| %s / %s | %.1f | %.1f | %+.1f%% | %.1f | %.1f | %s | %s |" % (
            b["clients"], b["distinct_principals"], bp["server_cpu_ms"],
            ap["server_cpu_ms"], cpu, b["all_clients_latency_ms"]["p95"],
            a["all_clients_latency_ms"]["p95"], b["total_frame_bytes"],
            a["total_frame_bytes"],
        ))
    lines += [
        "", "## Administrator loaded windows", "",
        "Ten status changes per window. Bytes combine WebSocket frames and "
        "required gzip HTTP reload response bodies; HTTP headers are excluded.",
        "",
        "| Window | Before reload GETs | After reload GETs | Before bytes | "
        "After bytes | Before p95 ms | After p95 ms |",
        "|---|---:|---:|---:|---:|---:|---:|",
    ]
    for b, a in zip(before["admin_pages"], after["admin_pages"]):
        if b["name"].endswith("_exact_each_event"):
            continue
        assert b["name"] == a["name"]
        lines.append("| %s | %s | %s | %s | %s | %.1f | %.1f |" % (
            b["name"], b["reload_gets"], a["reload_gets"],
            b["total_body_and_frame_bytes"], a["total_body_and_frame_bytes"],
            b["latency_ms"]["p95"], a["latency_ms"]["p95"],
        ))
    lines += ["", "## Other measured behavior", "", "| Scenario | Before | After |",
              "|---|---:|---:|"]
    metrics = [
        ("Notification bootstrap JSON bytes", lambda r: r["notifications"]["payload_bytes"]),
        ("Notification bootstrap full order maps", lambda r: r["notifications"]["map_count_sent"]),
        ("Reconnect snapshot GETs", lambda r: r["recovery"][0]["snapshot_gets"]),
        ("Reconnect replayed events", lambda r: r["recovery"][0]["replayed_events"]),
        ("Other-apparatus data events (two changes)", lambda r: sum(
            e["type"] != "no_event" for e in r["disjoint"]["events"])),
        ("Other-apparatus WS frame bytes (two changes)", lambda r: sum(
            e["frame_bytes"] for e in r["disjoint"]["events"])),
        ("Idle HTTP GETs / data events (8 s)", lambda r:
            "%s / %s" % (r["idle"]["http_gets"], r["idle"]["data_events"])),
    ]
    lines += ["| %s | %s | %s |" % (name, getter(before), getter(after)) for name, getter in metrics]
    for name, phase in (("Idle server CPU, ms", "idle_worker_streams"),
                        ("Other-apparatus server CPU, ms", "other_apparatus_change")):
        lines.append("| %s | %.1f | %.1f |" % (
            name, phases["before"][phase]["server_cpu_ms"], phases["after"][phase]["server_cpu_ms"],
        ))
    lines += [
        "| Peak observed server RSS across timed phases, MiB | %.1f | %.1f |" % tuple(
            max(p["server_rss_peak_bytes"] for p in results[mode]["phases"]) / 2**20
            for mode in ("before", "after")
        ),
        "", "## Separate SQL diagnostic", "",
        "One hold/release pair per scenario with SQLx query logging enabled. "
        "Counts are observed SQLx statement events, including incidental server "
        "maintenance in that interval. Diagnostic timings are intentionally not "
        "used above. PostgreSQL transaction/tuple/buffer counters remain available "
        "in the JSON and are not labeled as SQL counts.",
        "",
        "| Scenario | Before SQLx events | After SQLx events |",
        "|---|---:|---:|",
    ]
    ad = {p["name"]: p for p in diagnostics["after"]["phases"]}
    for bp in diagnostics["before"]["phases"]:
        ap = ad[bp["name"]]
        assert bp["sql_logging_observed"] and ap["sql_logging_observed"]
        lines.append("| %s | %s | %s |" % (
            bp["name"], bp["sqlx_statement_events"], ap["sqlx_statement_events"],
        ))
    lines += [
        "", "## Correctness", "",
        "Both runs passed exact authorized HTTP comparisons for all recorded "
        "scenarios, including held and released actor-specific controls, the "
        "other-apparatus views while the hold is active, current ordering, "
        "order statuses, stage states, maps, pagination metadata and cursors. "
        "A real version-checked reorder and its inverse restored the original "
        "sequence with no lost rows. Reconnect state matched fresh HTTP.",
        "",
        "- Before: %s recorded exact checks." % len(before["correctness"]),
        "- After: %s recorded exact checks." % len(after["correctness"]),
        "",
        "## Two independent server processes", "",
        "A separate correctness run starts two OS processes against one disposable "
        "PostgreSQL clone. Process B logs in independently as the acting worker, "
        "another worker and an administrator, then subscribes to worker state and "
        "a 40-row administrator window. Real hold, release, reorder and inverse "
        "reorder requests go through process A. All four actions converge to the "
        "exact authorized HTTP content on both processes; original ordering is "
        "restored. Static role/config files are copied explicitly for this test.",
        "",
        "Two-process tested binary SHA-256: `" + crossprocess["candidate_binary_sha256"] + "`.",
        "",
        "## Limits and reproduction", "",
        "See [the harness methodology](order_stream_README.md). This is one local "
        "paired run with 32 sockets maximum, not a capacity forecast. p95 uses ten "
        "samples per mutation scenario. Other role/capability combinations and broader "
        "deployment stress need their own tests beyond the admin/aparatchi scopes here. "
        "No WAN, TLS, battery, physical mobile rendering or reconnect storm is measured.",
        "",
        "Replay is bounded: state history retains at most 64 entries / 256 KiB "
        "for 90 seconds; page history retains at most 8 windows / 512 KiB for "
        "90 seconds. Missing or expired history falls back to authoritative HTTP. "
        "Role/config files are not automatically shared between deployment nodes.",
        "",
        "## Reference methods", "",
        "- [Centrifuge prepared fanout payloads](https://github.com/centrifugal/centrifuge/"
        "blob/7ac037196c955bf7140f2a48f8d35fd4de4b77d9/hub.go#L1168): prepare reusable "
        "wire data once for compatible subscribers.",
        "- [Centrifugo bounded stream recovery](https://centrifugal.dev/docs/server/"
        "history_and_recovery) and [Dart subscriber recovery](https://github.com/"
        "centrifugal/centrifuge-dart/blob/380cb2e4cb4fc43960daa8ffda6fea621d0efa8b/"
        "lib/src/subscription.dart): prove epoch/cursor continuity or reload.",
        "- [PowerSync watched queries](https://github.com/powersync-ja/powersync.dart/"
        "blob/1961d8ac7652fd3789d50a4c28a0e35034383ba2/packages/powersync/lib/src/"
        "database/powersync_database.dart#L611): react to relevant changes and coalesce work.",
        "- [Flutter build cost guidance](https://docs.flutter.dev/perf/"
        "best-practices#control-build-cost): bound rebuild work to changing content.",
        "",
        "These are reference methods adapted to the existing system. No external "
        "realtime platform or replacement project was introduced.",
        "",
    ]
    stem = "order_stream_" + args.date
    (output / (stem + ".md")).write_text("\n".join(lines))
    (output / (stem + ".json")).write_text(json.dumps({
        "primary": results, "sql_diagnostics": diagnostics, "crossprocess": crossprocess,
    }, indent=2) + "\n")
    print("Generated " + stem + ".md and sanitized metrics JSON")


if __name__ == "__main__":
    main()
