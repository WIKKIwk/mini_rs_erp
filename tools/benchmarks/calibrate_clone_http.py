#!/usr/bin/env python3
"""Calibrate gzip on exact private-clone HTTP bodies, never persisting bodies.

Run through tools/db/macos_keychain.py --admin db and the isolated fixture
Python. Each calibrator accepts body bytes on stdin and emits metrics only.
"""
import argparse
import gzip
import http.client
import json
from pathlib import Path
import subprocess
from urllib.parse import urlencode

from order_stream_fixture import Fixture
from order_stream_benchmark import HTTP, PAGE, SNAPSHOT, digest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-dir", type=Path, required=True)
    parser.add_argument("--new-calibrator", type=Path, required=True)
    parser.add_argument("--baseline-calibrator", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    fixture = Fixture(args.run_dir)
    server = fixture.metadata["servers"]["after"]
    assert server["database"].startswith("bench_order_stream_after_")
    port = server["port"]
    login = HTTP(port)
    try:
        _, account = login.request("/v1/mobile/auth/login", "POST",
                                   fixture.admin_credentials("after"))
    finally:
        login.close()
    token = account["token"]
    admin = HTTP(port, token)
    report = {"input": "exact clone HTTP JSON bodies in memory only", "rounds": 21, "cases": []}
    try:
        _, snapshot = admin.request(SNAPSHOT)
        measured = json.loads((args.run_dir / "after-sockudo-comparison.json").read_text())
        expected = measured["selection_fingerprints"]["apparatus"]
        apparatus = next(key for key in snapshot["sequences"] if digest(key) == expected)
        del snapshot
        for view, length, scope in (("sequence", 40, apparatus), ("orders", 60, "")):
            cursor, pages = "", []
            for page_number in range(length // 20):
                connection = http.client.HTTPConnection("127.0.0.1", port, timeout=30)
                try:
                    connection.request("GET", PAGE + "?" + urlencode({
                        "view": view, "apparatus": scope, "target": "", "search": "",
                        "limit": 20, "cursor": cursor,
                    }), headers={"Accept-Encoding": "gzip", "Authorization": "Bearer " + token})
                    response = connection.getresponse()
                    raw = response.read()
                    assert response.status == 200
                    body = gzip.decompress(raw) if response.getheader("Content-Encoding") == "gzip" else raw
                finally:
                    connection.close()
                cursor = json.loads(body).get("next_cursor") or ""
                page = {"page": page_number, "actual_current_gzip_bytes": len(raw)}
                for name, binary in (("baseline_miniz", args.baseline_calibrator),
                                     ("new_zlib_rs", args.new_calibrator)):
                    run = subprocess.run([str(binary.resolve()), "21"], input=body,
                                         capture_output=True, check=True)
                    page[name] = json.loads(run.stdout)
                pages.append(page)
                del raw, body
            for backend in ("baseline_miniz", "new_zlib_rs"):
                records = []
                for level in range(3, 10):
                    entries = [next(value for value in page[backend]["results"] if value["level"] == level)
                               for page in pages]
                    records.append({"level": level, "gzip_bytes_total": sum(v["gzip_bytes"] for v in entries),
                                    "sum_page_compression_p50_ms": sum(v["compression_p50_ms"] for v in entries),
                                    "sum_page_compression_p95_ms": sum(v["compression_p95_ms"] for v in entries)})
                report["cases"].append({"view": view, "backend": backend,
                    "loaded_pages": len(pages),
                    "input_bytes": sum(page[backend]["input_bytes"] for page in pages),
                    "actual_current_gzip_bytes": sum(page["actual_current_gzip_bytes"] for page in pages),
                    "levels": records})
    finally:
        try:
            admin.request("/v1/mobile/auth/logout", "POST", {})
        finally:
            admin.close()
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report))


if __name__ == "__main__":
    main()
