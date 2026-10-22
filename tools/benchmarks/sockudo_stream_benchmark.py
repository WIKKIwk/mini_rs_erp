#!/usr/bin/env python3
"""Compare real loopback WS frames on identical, synthetic queue workloads.

The companion Rust fixture exposes baseline Axum/tungstenite 0.29 and the
production Sockudo adapter in the same release binary. No database, credentials,
business rows, TLS connection, or remote address is used. Production sequence
diff code is included by the fixture instead of reimplemented for the server.
"""
from __future__ import annotations

import argparse
import base64
from concurrent.futures import ThreadPoolExecutor
import copy
import ctypes
import ctypes.util
import hashlib
import http.client
import json
import math
import os
from pathlib import Path
import platform
import socket
import statistics
import struct
import subprocess
import tempfile
import threading
import time
from urllib.parse import urlencode
import zlib

APPARATUS = "synthetic-apparatus"
BASELINE_REVISION = "95d1863597b73fe4183cab33b2060362db0da6c4"
MOBILE_BASELINE_REVISION = "703cfd8d5ff81e9c3b5263e834fb80eb5b985736"
NATIVE_FNV = None


def stats(values):
    values = sorted(values)
    if not values:
        return {}
    return {"n": len(values), "min": values[0], "p50": statistics.median(values),
            "p95": values[math.ceil(len(values) * .95) - 1], "max": values[-1],
            "mean": statistics.mean(values)}


def identifier(value):
    return "00000000-0000-4000-8000-%012x" % value


def fingerprint(values):
    if NATIVE_FNV is not None:
        data = b"".join(item.encode("utf8") + b"\xff" for item in values)
        return "%016x" % NATIVE_FNV(data, len(data))
    value = 0xcbf29ce484222325
    for item in values:
        for octet in item.encode("utf8") + b"\xff":
            value = ((value ^ octet) * 0x100000001b3) & 0xffffffffffffffff
    return "%016x" % value


def native_validator():
    """Keep Python validation from dominating the concurrent-client scheduler.

    The C implementation is independent of the production Rust helper and is
    checked against the Python Unicode/boundary fixtures before measurements.
    The validator runs outside the timed receive/decode path.
    """
    global NATIVE_FNV
    directory = tempfile.TemporaryDirectory(prefix="accord-stream-validator-")
    source = Path(__file__).with_name("stream_fnv1a64.c")
    library = Path(directory.name) / "validator.so"
    try:
        subprocess.run(["cc", "-O3", "-shared", "-fPIC", "-o", str(library), str(source)],
                       check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=30)
        loaded = ctypes.CDLL(str(library))
        function = loaded.stream_benchmark_fnv1a64
        function.argtypes = (ctypes.c_char_p, ctypes.c_size_t)
        function.restype = ctypes.c_uint64
        NATIVE_FNV = function
        self_test()
        return directory, {"implementation": "independent-c-fnv1a64",
            "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest()}
    except (OSError, subprocess.SubprocessError):
        NATIVE_FNV = None
        directory.cleanup()
        return None, {"implementation": "reference-python-fnv1a64"}


def sequence_edit(before, edit):
    assert edit["base_len"] == len(before), "Sequence base length mismatch"
    assert edit["base_hash"] == fingerprint(before), "Sequence base fingerprint mismatch"
    after = []
    for operation in edit["ops"]:
        assert isinstance(operation, list) and operation
        if operation[0] == 0:
            assert len(operation) == 3
            start, count = operation[1:]
            assert type(start) is int and type(count) is int
            assert start >= 0 and count > 0 and start + count <= len(before)
            after.extend(before[start:start + count])
        elif operation[0] == 1:
            assert len(operation) == 2 and isinstance(operation[1], list)
            assert all(isinstance(item, str) for item in operation[1])
            after.extend(operation[1])
        else:
            raise AssertionError("Unknown sequence edit operation")
        assert len(after) <= edit["len"], "Sequence edit output exceeds declared length"
    assert edit["len"] == len(after), "Sequence result length mismatch"
    assert edit["hash"] == fingerprint(after), "Sequence result fingerprint mismatch"
    return after


def apply_event(state, event):
    if event["type"] == "snapshot":
        return event
    assert event["type"] == "state_delta"
    assert event["epoch"] == state["epoch"] and event["scope"] == state["scope"]
    assert event["base_rev"] == state["rev"], "Non-contiguous revision"
    assert event["rev"] > event["base_rev"]
    after = copy.deepcopy(state)
    for field, changes in event["patch"].items():
        values = after.setdefault(field, {})
        for key in changes["remove"]:
            values.pop(key, None)
        values.update(changes["upsert"])
        for key, edit in changes.get("edit", {}).items():
            assert key in state[field], "Missing sequence baseline"
            values[key] = sequence_edit(state[field][key], edit)
    after["rev"] = event["rev"]
    return after


def expected_next(sequence, statuses, step, scenario, count):
    sequence = list(sequence)
    if scenario in ("reorder", "snapshot") and len(sequence) > 1:
        source, target = (step * 7919) % len(sequence), (step * 104729 + 1) % len(sequence)
        item = sequence.pop(source)
        sequence.insert(target, item)
    elif scenario == "replace":
        sequence[(step * 7919) % len(sequence)] = identifier(count + step)
    elif scenario == "status":
        item = sequence[(step * 7919) % len(sequence)]
        statuses[item] = "in_progress" if step % 2 == 0 else "queued"
    return sequence


class WebSocket:
    """Minimal RFC6455/7692 client, exposing real frame and handshake bytes."""
    def __init__(self, port, route, parameters):
        self.socket = socket.create_connection(("127.0.0.1", port), timeout=30)
        self.socket.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        self.buffer = b""
        self.received = self.sent = 0
        self.received_control = self.sent_control = 0
        self.received_payload = self.decoded = 0
        self.inflater = None
        self.server_no_context = False
        self.window_bits = 15
        self.extensions = ""
        key = base64.b64encode(os.urandom(16)).decode("ascii")
        request = (
            "GET %s?%s HTTP/1.1\r\nHost: 127.0.0.1:%s\r\n"
            "Upgrade: websocket\r\nConnection: Upgrade\r\n"
            "Sec-WebSocket-Key: %s\r\nSec-WebSocket-Version: 13\r\n"
            "Sec-WebSocket-Extensions: permessage-deflate; client_max_window_bits\r\n\r\n"
        ) % (route, urlencode(parameters), port, key)
        self.handshake_sent = len(request.encode("ascii"))
        self.socket.sendall(request.encode("ascii"))
        while b"\r\n\r\n" not in self.buffer:
            data = self.socket.recv(65536)
            if not data:
                raise OSError("EOF during WebSocket handshake")
            self.buffer += data
        head, self.buffer = self.buffer.split(b"\r\n\r\n", 1)
        self.handshake_received = len(head) + 4
        lines = head.decode("ascii").split("\r\n")
        assert " 101 " in lines[0], "WebSocket upgrade rejected: " + lines[0]
        headers = {key.lower().strip(): value.strip()
                   for key, value in (line.split(":", 1) for line in lines[1:])}
        expected_accept = base64.b64encode(hashlib.sha1(
            (key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode("ascii")
        ).digest()).decode("ascii")
        assert headers["sec-websocket-accept"] == expected_accept
        self.extensions = headers.get("sec-websocket-extensions", "")
        if self.extensions:
            options = [part.strip() for part in self.extensions.split(";")]
            assert options[0] == "permessage-deflate"
            self.server_no_context = "server_no_context_takeover" in options
            for option in options:
                if option.startswith("server_max_window_bits="):
                    self.window_bits = int(option.split("=", 1)[1])
        expected_compression = route == "/sockudo" and parameters.get("profile") != "disabled"
        assert bool(self.extensions) == expected_compression, "Compression negotiation mismatch"

    def exact(self, length):
        while len(self.buffer) < length:
            data = self.socket.recv(max(65536, length - len(self.buffer)))
            if not data:
                raise OSError("Unexpected WebSocket EOF")
            self.buffer += data
        result, self.buffer = self.buffer[:length], self.buffer[length:]
        return result

    def send(self, opcode, data=b""):
        size = len(data)
        head = bytes((0x80 | opcode, 0x80 | (size if size < 126 else 126 if size < 65536 else 127)))
        if size >= 65536:
            head += struct.pack("!Q", size)
        elif size >= 126:
            head += struct.pack("!H", size)
        mask = os.urandom(4)
        encoded = head + mask + bytes(octet ^ mask[index % 4] for index, octet in enumerate(data))
        self.socket.sendall(encoded)
        self.sent += len(encoded)
        if opcode >= 8:
            self.sent_control += len(encoded)

    def event(self):
        fragments, compressed, opcode = [], False, None
        wire_bytes = payload_bytes = 0
        while True:
            head = self.exact(2)
            frame_opcode, size, overhead = head[0] & 15, head[1] & 127, 2
            assert not head[1] & 0x80, "Server frame must not be masked"
            assert not head[0] & 0x30, "Unexpected reserved bits"
            if size == 126:
                size, overhead = struct.unpack("!H", self.exact(2))[0], 4
            elif size == 127:
                size, overhead = struct.unpack("!Q", self.exact(8))[0], 10
            assert size <= 64 * 1024 * 1024
            payload = self.exact(size)
            self.received += size + overhead
            self.received_payload += size
            if frame_opcode >= 8:
                self.received_control += size + overhead
            if frame_opcode == 9:
                self.send(10, payload)
                continue
            if frame_opcode == 10:
                continue
            if frame_opcode == 8:
                raise OSError("Server closed WebSocket")
            if frame_opcode in (1, 2):
                assert opcode is None
                opcode, compressed = frame_opcode, bool(head[0] & 0x40)
            else:
                assert frame_opcode == 0 and opcode is not None
                assert not head[0] & 0x40
            fragments.append(payload)
            wire_bytes += size + overhead
            payload_bytes += size
            if not head[0] & 0x80:
                continue
            payload = b"".join(fragments)
            started = time.perf_counter_ns()
            if compressed:
                assert self.extensions, "Compressed frame without negotiation"
                if self.inflater is None or self.server_no_context:
                    self.inflater = zlib.decompressobj(-self.window_bits)
                payload = self.inflater.decompress(payload + b"\x00\x00\xff\xff")
                assert not self.inflater.unconsumed_tail
            inflated = time.perf_counter_ns()
            result = json.loads(payload)
            decoded = time.perf_counter_ns()
            self.decoded += len(payload)
            return result, {"frame_bytes": wire_bytes, "payload_bytes": payload_bytes,
                "decoded_bytes": len(payload), "compressed": compressed,
                "inflate_ms": (inflated - started) / 1e6,
                "json_decode_ms": (decoded - inflated) / 1e6}

    def close(self):
        try:
            self.send(8, struct.pack("!H", 1000))
        except OSError:
            pass
        self.socket.close()


def process_usage(pid):
    if platform.system() == "Darwin":
        class RUsage(ctypes.Structure):
            _fields_ = [("uuid", ctypes.c_ubyte * 16)] + [
                (name, ctypes.c_uint64) for name in (
                    "user", "system", "wakeups", "interrupts", "pageins", "wired",
                    "resident", "footprint", "start", "exit", "child_user", "child_system",
                    "child_wakeups", "child_interrupts", "child_pageins", "child_elapsed",
                    "disk_read", "disk_write")]
        class Timebase(ctypes.Structure):
            _fields_ = [("numer", ctypes.c_uint32), ("denom", ctypes.c_uint32)]
        proc = ctypes.CDLL(ctypes.util.find_library("proc"))
        timebase = Timebase()
        ctypes.CDLL(ctypes.util.find_library("System")).mach_timebase_info(ctypes.byref(timebase))
        value = RUsage()
        if proc.proc_pid_rusage(pid, 2, ctypes.byref(value)):
            raise RuntimeError("Cannot read fixture process usage")
        return {"cpu_ms": (value.user + value.system) * timebase.numer / timebase.denom / 1e6,
                "rss_bytes": value.resident, "footprint_bytes": value.footprint}
    if platform.system() == "Linux":
        fields = Path("/proc/%s/stat" % pid).read_text().rsplit(")", 1)[1].split()
        ticks = os.sysconf("SC_CLK_TCK")
        return {"cpu_ms": (int(fields[11]) + int(fields[12])) * 1000 / ticks,
                "rss_bytes": int(fields[21]) * os.sysconf("SC_PAGE_SIZE")}
    return {}


def prepare(port, parameters):
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=120)
    try:
        connection.request("GET", "/prepare?" + urlencode(parameters))
        response = connection.getresponse()
        assert response.status == 200, "Fixture rejected dataset"
        return json.loads(response.read())
    finally:
        connection.close()


def one_client(port, route, parameters, ready, start):
    begun = time.perf_counter_ns()
    client = WebSocket(port, route, parameters)
    connected = time.perf_counter_ns()
    state, bootstrap = client.event()
    bootstrapped = time.perf_counter_ns()
    sequence = [identifier(i) for i in range(parameters["count"])]
    statuses = {item: "queued" for item in sequence}
    assert state["sequences"][APPARATUS] == sequence
    assert state["visible_order_ids"][APPARATUS] == sequence
    assert state["order_statuses"] == statuses
    ready.wait(timeout=120)
    assert start.wait(timeout=120)
    timings, frames, inflate, decoding = [], [], [], []
    sent_before = client.sent
    for step in range(parameters["updates"]):
        started = time.perf_counter_ns()
        client.send(1, b'{"type":"next"}')
        event, metric = client.event()
        received = time.perf_counter_ns()
        timings.append((received - started) / 1e6)
        frames.append(metric["frame_bytes"])
        inflate.append(metric["inflate_ms"])
        decoding.append(metric["json_decode_ms"])
        sequence = expected_next(sequence, statuses, step,
            parameters["scenario"], parameters["count"])
        state = apply_event(state, event)
        assert state["rev"] == step + 1
        assert state["sequences"][APPARATUS] == sequence
        assert state["visible_order_ids"][APPARATUS] == sequence
        assert state["order_statuses"] == statuses
    result = {"roundtrip_ms": timings, "update_frame_bytes": frames,
        "inflate_ms": inflate, "json_decode_ms": decoding,
        "bootstrap": dict(bootstrap, connect_ms=(connected - begun) / 1e6,
            receive_ms=(bootstrapped - connected) / 1e6),
        "handshake_sent_bytes": client.handshake_sent,
        "handshake_received_bytes": client.handshake_received,
        "update_sent_frame_bytes": client.sent - sent_before,
        "negotiated_extensions": client.extensions,
        "correctness_checks": parameters["updates"] + 1,
        "final_sequence_fingerprint": fingerprint(sequence)}
    client.close()
    result.update(total_received_ws_frame_bytes=client.received,
        total_sent_ws_frame_bytes=client.sent,
        received_control_frame_bytes=client.received_control,
        sent_control_frame_bytes=client.sent_control)
    return result


def case(port, pid, route, parameters, clients):
    dataset = prepare(port, parameters)
    ready, start = threading.Barrier(clients + 1), threading.Event()
    before_connect = process_usage(pid)
    def run_client():
        try:
            return one_client(port, route, parameters, ready, start)
        except BaseException:
            ready.abort()
            start.set()
            raise
    with ThreadPoolExecutor(max_workers=clients) as executor:
        futures = [executor.submit(run_client)
                   for _ in range(clients)]
        try:
            ready.wait(timeout=120)
        except threading.BrokenBarrierError:
            start.set()
            for future in futures:
                if future.done():
                    future.result()
            raise
        before_updates = process_usage(pid)
        started = time.perf_counter_ns()
        start.set()
        values = [future.result(timeout=180) for future in futures]
        elapsed_ms = (time.perf_counter_ns() - started) / 1e6
    after_updates = process_usage(pid)
    merged = lambda key: [value for result in values for value in result[key]]
    cpu = after_updates.get("cpu_ms", 0) - before_updates.get("cpu_ms", 0)
    return {"route": route, "parameters": parameters, "clients": clients,
        "dataset": dict(dataset, update_generation_ns=stats(dataset["update_generation_ns"]),
                        update_decoded_bytes=stats(dataset["update_decoded_bytes"])),
        "roundtrip_ms": stats(merged("roundtrip_ms")),
        "inflate_ms": stats(merged("inflate_ms")),
        "json_decode_ms": stats(merged("json_decode_ms")),
        "update_frame_bytes": stats(merged("update_frame_bytes")),
        "total_update_received_frame_bytes": sum(merged("update_frame_bytes")),
        "total_update_sent_frame_bytes": sum(value["update_sent_frame_bytes"] for value in values),
        "bootstrap_received_frame_bytes": sum(value["bootstrap"]["frame_bytes"] for value in values),
        "bootstrap_roundtrip_ms": stats([value["bootstrap"]["connect_ms"] +
            value["bootstrap"]["receive_ms"] for value in values]),
        "handshake_received_bytes": sum(value["handshake_received_bytes"] for value in values),
        "handshake_sent_bytes": sum(value["handshake_sent_bytes"] for value in values),
        "total_received_ws_frame_bytes": sum(value["total_received_ws_frame_bytes"] for value in values),
        "total_sent_ws_frame_bytes": sum(value["total_sent_ws_frame_bytes"] for value in values),
        "received_control_frame_bytes": sum(value["received_control_frame_bytes"] for value in values),
        "sent_control_frame_bytes": sum(value["sent_control_frame_bytes"] for value in values),
        "update_phase_wall_ms": elapsed_ms,
        "server_update_cpu_ms": cpu,
        "server_update_cpu_ms_per_message": cpu / (clients * parameters["updates"]),
        "server_bootstrap_cpu_ms": before_updates.get("cpu_ms", 0) - before_connect.get("cpu_ms", 0),
        "server_rss_before_updates_bytes": before_updates.get("rss_bytes"),
        "server_rss_after_updates_bytes": after_updates.get("rss_bytes"),
        "negotiated_extensions": sorted(set(value["negotiated_extensions"] for value in values)),
        "correctness_checks": sum(value["correctness_checks"] for value in values),
        "final_sequence_fingerprints": sorted(set(value["final_sequence_fingerprint"] for value in values))}


def self_test():
    assert fingerprint(["order-000000", "ўзбек-😀"]) == "054688fb8e618984"
    before, after = ["a", "b", "c"], ["c", "a", "b"]
    edit = {"base_len": 3, "base_hash": fingerprint(before), "len": 3,
            "hash": fingerprint(after), "ops": [[0, 2, 1], [0, 0, 2]]}
    assert sequence_edit(before, edit) == after
    for change in ({"base_len": 4}, {"base_hash": "0" * 16}, {"len": 2},
                   {"hash": "0" * 16}, {"ops": [[0, 1, 4]]}, {"ops": [[2, 1, 2]]}):
        try:
            sequence_edit(before, dict(edit, **change))
        except AssertionError:
            continue
        raise AssertionError("Corrupt patch was accepted")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--counts", default="1,10,1000,10000")
    parser.add_argument("--clients", default="1,8")
    parser.add_argument("--updates", type=int, default=30)
    parser.add_argument("--scenarios", default="reorder,replace,status,snapshot")
    parser.add_argument("--profiles", default="production,disabled,dedicated:1,dedicated:3,dedicated:6,shared:3")
    parser.add_argument("--repeats", type=int, default=2)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    self_test()
    if args.self_test and not args.binary:
        print("benchmark reducer self-tests passed")
        return
    if not args.binary or not args.output:
        parser.error("--binary and --output are required for measured runs")
    counts = [int(value) for value in args.counts.split(",")]
    clients = [int(value) for value in args.clients.split(",")]
    assert all(1 <= value <= 10000 for value in counts)
    assert all(1 <= value <= 128 for value in clients)
    assert 1 <= args.updates <= 512 and 1 <= args.repeats <= 10
    binary = args.binary.resolve()
    validator_directory, validator_metadata = native_validator()
    report = {"dataset": "synthetic", "backend_baseline_revision": BASELINE_REVISION,
        "mobile_baseline_revision": MOBILE_BASELINE_REVISION,
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "harness_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "host": {"platform": platform.platform(), "logical_cpus": os.cpu_count()},
        "client_correctness_validator": validator_metadata,
        "configuration": {"counts": counts, "clients": clients, "updates": args.updates,
            "scenarios": args.scenarios.split(","), "profiles": args.profiles.split(","),
            "repeats": args.repeats},
        "limitations": [
            "Synthetic UUID queues and local TCP loopback; no database or production data.",
            "Baseline/current transport share one release executable and identical pre-serialized messages.",
            "Application diff/serialization timings are reported separately from timed WS transport.",
            "Traffic counts RFC6455 frame bytes, plus separately reported HTTP handshakes; excludes TCP/IP/TLS overhead.",
            "Latency includes loopback request/response, Python inflate/JSON decode and local scheduling.",
            "Concurrent clients issue separate next commands; this is not a single business-event broadcast test.",
            "RSS includes shared cached fixture messages and allocator retention, not isolated per-connection RAM.",
            "Fixture bootstrap is a WS snapshot; production state streams bootstrap via HTTP, so bootstrap results are transport-only.",
            "Fresh connections are measured; production revision resume is covered by application tests, not this fixture.",
            "Client next commands are tiny and uncompressed; uplink savings for large client payloads are not measured.",
            "Outgoing close frames are counted; server close replies after the measured stream are not drained.",
        ], "cases": []}
    process = subprocess.Popen([str(binary)], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    try:
        ready = json.loads(process.stdout.readline())
        assert ready["dataset"] == "synthetic"
        report["fixture_build"] = ready
        source_root = Path(__file__).resolve().parents[2]
        actual_inputs = {path: hashlib.sha256((source_root / path).read_bytes()).hexdigest()
                         for path in ready["build_inputs_sha256"]}
        assert actual_inputs == ready["build_inputs_sha256"], "Rebuild fixture: source inputs changed"
        port = ready["port"]
        profiles = [("/baseline", "disabled", 3)]
        for profile in args.profiles.split(","):
            name, _, level = profile.partition(":")
            profiles.append(("/sockudo", name, int(level) if level else 3))
        for repeat in range(args.repeats):
            for count in counts:
                for scenario in args.scenarios.split(","):
                    for protocol in ("v1", "v2"):
                        if protocol == "v2" and scenario in ("status", "snapshot"):
                            continue
                        # Alternate profile order to avoid always favoring a warm
                        # transport; /prepare excludes first-dataset compilation.
                        ordered_profiles = profiles if repeat % 2 == 0 else list(reversed(profiles))
                        for client_count in clients:
                            for route, profile, level in ordered_profiles:
                                parameters = {"count": count, "updates": args.updates,
                                    "protocol": protocol, "scenario": scenario,
                                    "profile": profile, "level": level, "window": 15, "threshold": 128}
                                result = case(port, process.pid, route, parameters, client_count)
                                result["repeat"] = repeat
                                report["cases"].append(result)
                                args.output.parent.mkdir(parents=True, exist_ok=True)
                                args.output.write_text(json.dumps(report, indent=2) + "\n")
                                print(json.dumps({"repeat": repeat, "count": count,
                                    "scenario": scenario, "protocol": protocol, "clients": client_count,
                                    "transport": profile if route == "/sockudo" else "tungstenite",
                                    "level": level, "bytes_per_update": result["update_frame_bytes"]["mean"],
                                    "p95_ms": result["roundtrip_ms"]["p95"],
                                    "server_cpu_ms": result["server_update_cpu_ms"]}), flush=True)
        report["build_inputs_unchanged_during_run"] = all(
            hashlib.sha256((source_root / path).read_bytes()).hexdigest() == digest
            for path, digest in ready["build_inputs_sha256"].items())
        args.output.write_text(json.dumps(report, indent=2) + "\n")
    finally:
        process.terminate()
        try:
            process.communicate(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.communicate()
        if validator_directory is not None:
            validator_directory.cleanup()


if __name__ == "__main__":
    main()
