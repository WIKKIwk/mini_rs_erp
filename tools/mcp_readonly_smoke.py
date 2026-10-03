#!/usr/bin/env python3
"""Exercise the compiled Rust synthetic fixture over stdio; never a replacement mock."""
import json
from pathlib import Path
import subprocess
import sys

binary = Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/examples/mcp_readonly_fixture").resolve()

def run(text, enabled=True):
    command = [str(binary)] + (["--synthetic-fixture"] if enabled else [])
    return subprocess.run(command, input=text, text=True, capture_output=True, check=True, timeout=20)

assert run("", enabled=False).stdout == "", "Fixture must remain disabled without explicit flag"
requests = [
    {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "local-smoke", "version": "1"}}},
    {"jsonrpc": "2.0", "method": "notifications/initialized"},
    {"jsonrpc": "2.0", "id": 2, "method": "tools/list"},
]
for number, (name, args) in enumerate([
    ("erp_summary", {}),
    ("erp_order_status", {"order_id": "DEMO-ORDER"}),
    ("erp_wip", {"order_id": "DEMO-ORDER"}),
    ("erp_warehouse", {"warehouse": "DEMO-WH"}),
], start=3):
    requests.append({"jsonrpc": "2.0", "id": number, "method": "tools/call", "params": {"name": name, "arguments": args}})
responses = [json.loads(line) for line in run("\n".join(map(json.dumps, requests)) + "\n").stdout.splitlines()]
assert len(responses) == 6, "Notifications must not produce a response"
assert responses[0]["result"]["protocolVersion"] == "2025-03-26"
assert len(responses[1]["result"]["tools"]) == 4
for response in responses[2:]:
    body = json.loads(response["result"]["content"][0]["text"])
    assert body["data"]["synthetic_fixture"] is True
    assert body["metadata"]["partial"] is True
    assert body["metadata"]["source_freshness"] == "unknown"
assert json.loads(run("malformed\n").stdout)["error"]["code"] == -32700
assert run("x" * 32769).stdout == "", "Oversized requests must close without processing"
print("PASS: compiled Rust stdio fixture: disabled gate, initialize, notification suppression, four-tool discovery/calls, synthetic metadata, malformed JSON, request-size cap")
