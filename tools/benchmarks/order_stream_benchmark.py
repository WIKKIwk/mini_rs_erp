#!/usr/bin/env python3
"""Exercise real HTTP/WebSocket order streams on private PostgreSQL clones.

See order_stream_fixture.py. Outputs contain timings/counts/fingerprints only,
never account identities, tokens, codes, order identifiers or business payloads.
The client reducer is checked against fresh authorized REST snapshots.
"""
import argparse
import concurrent.futures
import contextlib
import copy
import ctypes
import ctypes.util
import gzip
import hashlib
import http.client
import json
import math
import os
from pathlib import Path
import platform
import re
import socket
import statistics
import struct
import subprocess
import threading
import time
from urllib.parse import urlencode, quote

from order_stream_fixture import Fixture, private_json

LIVE = "/v1/mobile/admin/production-maps/live"
SNAPSHOT = "/v1/mobile/admin/production-maps/sequence"
PAGE = "/v1/mobile/admin/production-maps/order-page"


def stats(values):
    values = sorted(values)
    if not values:
        return {}
    return {
        "n": len(values), "min": values[0], "p50": statistics.median(values),
        "p95": values[math.ceil(len(values) * .95) - 1], "max": values[-1],
        "mean": statistics.mean(values),
    }


def digest(value):
    return hashlib.sha256(
        json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()


class HTTP:
    def __init__(self, port, token=""):
        assert port in (18091, 18092)
        self.port, self.token = port, token
        self.connection = http.client.HTTPConnection("127.0.0.1", port, timeout=30)

    def request(self, path, method="GET", payload=None):
        headers = {"Accept-Encoding": "gzip", "User-Agent": "Accord-stream-benchmark/1"}
        if self.token:
            headers["Authorization"] = "Bearer " + self.token
        body = None
        if payload is not None:
            headers["Content-Type"] = "application/json"
            body = json.dumps(payload).encode()
        started = time.perf_counter()
        self.connection.request(method, path, body=body, headers=headers)
        response = self.connection.getresponse()
        raw = response.read()
        raw_complete = time.perf_counter()
        elapsed = (raw_complete - started) * 1000
        decoded = gzip.decompress(raw) if response.getheader("Content-Encoding") == "gzip" else raw
        value = json.loads(decoded) if decoded else None
        if response.status not in (200, 204, 304):
            code = value.get("error", "unknown") if isinstance(value, dict) else "unknown"
            # Avoid response bodies, which can contain business data.
            raise RuntimeError("Benchmark HTTP status %s, code %s" % (
                response.status, code if isinstance(code, str) and code.isidentifier() else "withheld"
            ))
        return {
            "status": response.status, "latency_ms": elapsed,
            "body_bytes": len(raw), "decoded_bytes": len(decoded),
            "response_decode_ms": (time.perf_counter() - raw_complete) * 1000,
        }, value

    def close(self):
        self.connection.close()


class WS:
    def __init__(self, port, token, query):
        assert port in (18091, 18092)
        self.socket = socket.create_connection(("127.0.0.1", port), timeout=30)
        self.buffer = b""
        self.received = self.sent = 0
        self.fragments = []
        query = dict(query, token=token)
        path = LIVE + "?" + urlencode(query)
        handshake = (
            "GET %s HTTP/1.1\r\nHost: 127.0.0.1:%s\r\n"
            "Upgrade: websocket\r\nConnection: Upgrade\r\n"
            "Sec-WebSocket-Key: YWNjb3JkLWJlbmNobWFyaw==\r\n"
            "Sec-WebSocket-Version: 13\r\n\r\n"
        ) % (path, port)
        self.socket.sendall(handshake.encode())
        while b"\r\n\r\n" not in self.buffer:
            self.buffer += self.socket.recv(65536)
        head, self.buffer = self.buffer.split(b"\r\n\r\n", 1)
        if b" 101 " not in head.split(b"\r\n")[0]:
            self.socket.close()
            raise RuntimeError("WebSocket handshake rejected")

    def exact(self, size):
        while len(self.buffer) < size:
            data = self.socket.recv(max(65536, size - len(self.buffer)))
            if not data:
                raise OSError("WebSocket closed")
            self.buffer += data
        data, self.buffer = self.buffer[:size], self.buffer[size:]
        return data

    def send(self, opcode, payload=b""):
        assert len(payload) < 126
        mask = os.urandom(4)
        encoded = bytes([0x80 | opcode, 0x80 | len(payload)]) + mask + bytes(
            value ^ mask[index % 4] for index, value in enumerate(payload)
        )
        self.socket.sendall(encoded)
        self.sent += len(encoded)

    def event(self, timeout=15):
        deadline = time.monotonic() + timeout
        while True:
            self.socket.settimeout(max(.001, deadline - time.monotonic()))
            head = self.exact(2)
            opcode, size, overhead = head[0] & 15, head[1] & 127, 2
            if size == 126:
                size, overhead = struct.unpack("!H", self.exact(2))[0], 4
            elif size == 127:
                size, overhead = struct.unpack("!Q", self.exact(8))[0], 10
            assert not head[1] & 128
            payload = self.exact(size)
            self.received += size + overhead
            if opcode == 9:
                self.send(10, payload)
                continue
            if opcode == 8:
                raise OSError("WebSocket closed by server")
            if opcode not in (0, 1):
                continue
            self.fragments.append(payload)
            if not head[0] & 0x80:
                continue
            payload = b"".join(self.fragments)
            self.fragments = []
            raw_complete = time.perf_counter()
            value = json.loads(payload)
            return {
                "payload_bytes": len(payload), "frame_bytes": size + overhead,
                "raw_complete_monotonic_s": raw_complete,
                "json_decode_ms": (time.perf_counter() - raw_complete) * 1000,
            }, value

    def close(self):
        try:
            self.send(8)
        except OSError:
            pass
        self.socket.close()


class RUsage(ctypes.Structure):
    _fields_ = [("uuid", ctypes.c_ubyte * 16)] + [
        (name, ctypes.c_uint64) for name in (
            "user", "system", "wakeups", "interrupts", "pageins", "wired",
            "resident", "footprint", "start", "exit", "child_user", "child_system",
            "child_wakeups", "child_interrupts", "child_pageins", "child_elapsed",
            "disk_read", "disk_write",
        )
    ]


class Timebase(ctypes.Structure):
    _fields_ = [("numer", ctypes.c_uint32), ("denom", ctypes.c_uint32)]


PROC = ctypes.CDLL(ctypes.util.find_library("proc"))
PROC.proc_pid_rusage.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_void_p]
TIMEBASE = Timebase()
ctypes.CDLL(ctypes.util.find_library("System")).mach_timebase_info(ctypes.byref(TIMEBASE))


def usage(pid):
    value = RUsage()
    if PROC.proc_pid_rusage(pid, 2, ctypes.byref(value)):
        return None
    return {
        "cpu_s": (value.user + value.system) * TIMEBASE.numer / TIMEBASE.denom / 1e9,
        "rss_bytes": value.resident, "footprint_bytes": value.footprint,
        "wakeups": value.wakeups,
    }


class Phase:
    def __init__(self, runner, name):
        self.runner, self.name = runner, name

    def __enter__(self):
        self.query_log = self.runner.fixture.run_dir / self.runner.mode / "server.log"
        self.query_log_start = self.query_log.stat().st_size
        self.before = usage(self.runner.server["pid"])
        self.db_before = self.runner.database_counters()
        self.started = time.perf_counter()
        self.samples = []
        self.stop = threading.Event()

        def monitor():
            while not self.stop.wait(.05):
                value = usage(self.runner.server["pid"])
                if value:
                    self.samples.append(value)

        self.monitor = threading.Thread(target=monitor, daemon=True)
        self.monitor.start()
        return self

    def __exit__(self, *args):
        after = usage(self.runner.server["pid"])
        elapsed = time.perf_counter() - self.started
        self.stop.set()
        self.monitor.join()
        # PostgreSQL cumulative statistics publish asynchronously; report them
        # explicitly as database counters, not an exact SQL query count.
        time.sleep(1.1)
        db_after = self.runner.database_counters()
        peak = [self.before, after, *self.samples]
        self.result = {
            "name": self.name, "wall_s": elapsed,
            "server_cpu_ms": (after["cpu_s"] - self.before["cpu_s"]) * 1000,
            "server_cpu_percent_one_core": (
                (after["cpu_s"] - self.before["cpu_s"]) / elapsed * 100
            ),
            "server_rss_before_bytes": self.before["rss_bytes"],
            "server_rss_after_bytes": after["rss_bytes"],
            "server_rss_peak_bytes": max(value["rss_bytes"] for value in peak),
            "server_footprint_peak_bytes": max(value["footprint_bytes"] for value in peak),
            "server_wakeups": after["wakeups"] - self.before["wakeups"],
            "database_counter_delta": {
                key: db_after[key] - self.db_before[key] for key in db_after
            },
        }
        if self.runner.server["query_log"]:
            with open(self.query_log, "rb") as source:
                source.seek(self.query_log_start)
                # Store counts only; raw SQL/log material remains private.
                lines = source.read().splitlines()
            count = sum(b"sqlx::query" in line for line in lines)
            self.result["sqlx_statement_events"] = count
            self.result["sql_logging_observed"] = count > 0
        self.runner.result["phases"].append(self.result)
        self.runner.save()
        print(json.dumps({
            "phase": self.name, "wall_s": round(elapsed, 3),
            "server_cpu_ms": round(self.result["server_cpu_ms"], 2),
            "rss_peak_mib": round(self.result["server_rss_peak_bytes"] / 1048576, 2),
        }), flush=True)


def field_patch(before, patch):
    result = dict(before)
    for key in patch["remove"]:
        result.pop(key, None)
    result.update(patch["upsert"])
    return result


def apply_state(before, event):
    assert event["epoch"] == before["epoch"]
    assert event.get("scope", "") == before.get("scope", "")
    assert event["base_rev"] == before["rev"], "Non-contiguous state delta"
    after = copy.deepcopy(before)
    for field, patch in event["patch"].items():
        if field == "maps":
            mapped = {item["map"]["id"]: item for item in before["maps"]}
            after["maps"] = list(field_patch(mapped, patch).values())
        elif field == "map_order":
            continue
        elif field in ("completed_orders", "completion_requests", "completion_request_decisions"):
            after[field] = patch
        elif field == "queue_policies":
            mapped = {item["apparatus_id"]: item for item in before[field]}
            after[field] = list(field_patch(mapped, patch).values())
        elif "scopes" in patch:
            nested = dict(before.get(field, {}))
            for removed in patch["remove"]:
                nested.pop(removed, None)
            for scope, changes in patch["scopes"].items():
                nested[scope] = field_patch(nested.get(scope, {}), changes)
            after[field] = nested
        else:
            after[field] = field_patch(before.get(field, {}), patch)
    if "map_order" in event["patch"]:
        mapped = {item["map"]["id"]: item for item in after["maps"]}
        after["maps"] = [mapped[identifier] for identifier in event["patch"]["map_order"]]
    after["rev"] = event["rev"]
    return after


STATE_FIELDS = (
    "maps", "sequences", "sequence_versions", "sequence_revisions",
    "visible_order_ids", "queue_states", "stage_states", "queue_policies",
    "queue_action_controls", "order_statuses", "order_controls",
    "frozen_orders_by_apparatus", "order_customers",
    "completed_orders", "completion_requests", "completion_request_decisions",
)


def comparable(snapshot):
    return {key: snapshot[key] for key in STATE_FIELDS if key in snapshot}


class Runner:
    def __init__(self, fixture, mode, clients, cycles, suffix="results",
                 protocols="auto", status_cycles=None):
        self.fixture, self.mode = fixture, mode
        self.server = fixture.metadata["servers"][mode]
        assert self.server["ready"]
        self.port, self.clients, self.cycles = self.server["port"], clients, cycles
        self.status_cycles = cycles if status_cycles is None else status_cycles
        self.sessions = []
        assert re.fullmatch(r"[a-z0-9-]+", suffix)
        self.suffix = suffix
        self.legacy_protocols = protocols == "legacy" or (
            protocols == "auto" and mode == "before"
        )
        self.result = {
            "mode": mode, "server": self.server,
            "protocols": "legacy" if self.legacy_protocols else "modern",
            "include_control": True,
            "harness_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
            "command_config": {
                "clients": clients, "cycles": cycles,
                "status_cycles": self.status_cycles, "output_suffix": suffix,
                "requested_protocols": protocols,
            },
            "dataset_dump_sha256": fixture.metadata["dump_sha256"],
            "source_counts": fixture.metadata["source_counts"],
            "host": {
                "platform": platform.platform(),
                "logical_cpus": os.cpu_count(),
                "ram_bytes": int(subprocess.check_output(["sysctl", "-n", "hw.memsize"])),
                "mach_timebase": [TIMEBASE.numer, TIMEBASE.denom],
            },
            "phases": [], "fanout": [], "recovery": [], "admin_pages": [],
            "stream_error_recovery": [],
            "correctness": [], "limitations": [
                "Local host loopback and local PostgreSQL; no WAN or phone battery measurement.",
                "Database transaction/block counters are not exact SQL query counts.",
                "Cloned test credentials; original account roles/assignments and business data retained.",
            ],
        }

    def save(self):
        private_json(self.fixture.run_dir / (self.mode + "-" + self.suffix + ".json"), self.result)

    def database_counters(self):
        database = self.server["database"]
        return json.loads(self.fixture.sql("""
            SELECT json_build_object(
              'xact_commit',xact_commit,'xact_rollback',xact_rollback,
              'blks_hit',blks_hit,'blks_read',blks_read,
              'tup_returned',tup_returned,'tup_fetched',tup_fetched)
            FROM pg_stat_database WHERE datname='%s';
        """ % database))

    def login(self, credentials):
        client = HTTP(self.port)
        try:
            _, value = client.request("/v1/mobile/auth/login", "POST", credentials)
        finally:
            client.close()
        self.sessions.append(value["token"])
        return value

    def get(self, token, path):
        client = HTTP(self.port, token)
        try:
            return client.request(path)
        finally:
            client.close()

    def snapshot(self, token):
        return self.get(token, SNAPSHOT + "?worker_scope=true&queue_limit=10")

    def profiles(self):
        self.admin = self.login(self.fixture.admin_credentials(self.mode))["token"]
        _, workers = self.get(self.admin, "/v1/mobile/admin/workers")
        profiles, unassigned = [], []
        for worker in sorted(workers, key=lambda item: item["id"]):
            _, value = self.get(
                self.admin, "/v1/mobile/admin/workers/profile-detail?id=" + quote(worker["id"])
            )
            if not value["worker"].get("code"):
                continue
            if not value.get("assigned_apparatus"):
                unassigned.append(value["worker"])
                continue
            try:
                login = self.login({
                    "phone": value["worker"]["phone"], "code": value["worker"]["code"]
                })
                _, snapshot = self.snapshot(login["token"])
            except RuntimeError:
                continue
            profiles.append({
                "token": login["token"], "assigned": login["assigned_apparatus"],
                "snapshot": snapshot,
                "credentials": {"phone": value["worker"]["phone"], "code": value["worker"]["code"]},
            })
        if not profiles:
            raise RuntimeError("Fixture has no accessible assigned workers")
        for profile in profiles:
            for apparatus in profile["assigned"]:
                controls = profile["snapshot"].get("queue_action_controls", {}).get(apparatus, {})
                visible = profile["snapshot"].get("sequences", {}).get(apparatus, [])
                for order in visible:
                    if controls.get(order, {}).get("print_preflight_allowed"):
                        self.actor, self.apparatus, self.order = profile, apparatus, order
                        self.profiles_all = profiles
                        self.shared_profiles = [
                            item for item in profiles if apparatus in item["assigned"]
                        ]
                        self.disjoint_profiles = [
                            item for item in profiles if apparatus not in item["assigned"]
                        ]
                        # The local dump has one active worker per apparatus.
                        # Exercise real distinct principals with two additional
                        # assignments in the disposable clone only.
                        seeded = 0
                        for worker in unassigned:
                            if seeded == 2:
                                break
                            if not worker["code"].startswith("40"):
                                continue
                            admin = HTTP(self.port, self.admin)
                            try:
                                admin.request(
                                    "/v1/mobile/admin/role-assignments", "PUT", {
                                        "principal_role": "aparatchi",
                                        "principal_ref": worker["id"],
                                        "role_id": "aparatchi",
                                        "assigned_apparatus": [apparatus],
                                    }
                                )
                            finally:
                                admin.close()
                            login = self.login({
                                "phone": worker["phone"], "code": worker["code"]
                            })
                            _, snapshot = self.snapshot(login["token"])
                            self.shared_profiles.append({
                                "token": login["token"],
                                "assigned": login["assigned_apparatus"],
                                "snapshot": snapshot,
                                "credentials": {"phone": worker["phone"], "code": worker["code"]},
                            })
                            seeded += 1
                        self.result["fixture_roles"] = {
                            "authenticated_workers": len(profiles) + seeded,
                            "workers_sharing_target_apparatus": len(self.shared_profiles),
                            "workers_on_other_apparatus": len(self.disjoint_profiles),
                            "target_queue_orders": len(visible),
                            "clone_only_added_apparatus_assignments": seeded,
                        }
                        self.result["selection_fingerprints"] = {
                            "actor": digest(["aparatchi", profile["credentials"]["phone"]]),
                            "apparatus": digest(apparatus),
                            "order": digest(order),
                            "shared_principals": [
                                digest(["aparatchi", item["credentials"]["phone"]])
                                for item in self.shared_profiles
                            ],
                            "disjoint_principals": [
                                digest(["aparatchi", item["credentials"]["phone"]])
                                for item in self.disjoint_profiles
                            ],
                        }
                        self.save()
                        return
        raise RuntimeError("Fixture has no visible eligible print-preflight order")

    def connect_state(self, token, snapshot=None):
        if snapshot is None:
            _, snapshot = self.snapshot(token)
        socket_ = WS(self.port, token, {
            "protocol": "state_delta_v1", "worker_scope": "true", "queue_limit": "10",
            "epoch": snapshot["epoch"], "rev": snapshot["rev"], "scope": snapshot["scope"],
        })
        metric, ready = socket_.event()
        assert ready["type"] == "state_ready", "Expected state stream bootstrap"
        bootstrap_reload = None
        if ready.get("resync"):
            bootstrap_reload, snapshot = self.snapshot(token)
        return {"socket": socket_, "token": token, "view": snapshot, "ready": ready,
                "bootstrap_reload": bootstrap_reload}

    def mutate(self, action, hold):
        client = HTTP(self.port, self.actor["token"])
        try:
            metric, body = client.request(
                "/v1/mobile/admin/production-maps/print-preflight", "POST", {
                    "apparatus": self.apparatus, "order_id": self.order,
                    "hold_id": hold, "idempotency_key": hold,
                    "action": action, "include_control": True,
                }
            )
            returned_hold = body.get("hold") or {}
            assert returned_hold.get("hold_id") == hold, "Mutation hold identity differs"
            assert returned_hold.get("status") == (
                "running" if action == "hold" else "failed"
            ), "Mutation status differs"
            control = body.get("control_state")
            metric.update(
                control_returned=control is not None,
                control_revision=control.get("rev") if control else None,
                # Optional control is delivered inside this same HTTP response.
                # This is response availability, not isolated server build time.
                control_available_ms=metric["latency_ms"] if control else None,
            )
            return metric, body
        finally:
            client.close()

    def receive(self, client, start, timeout=10, require_change=False, transition=None):
        if transition:
            prior_revision = client["view"]["rev"]
            previous = self.target_control(client["view"]).get("print_preflight") or {}
            if transition["action"] == "failed":
                assert previous.get("hold_id") == transition["hold"], (
                    "Release is missing the exact previous hold"
                )
            deadline = time.monotonic() + timeout
            frames, payloads, empties, count = 0, 0, 0, 0
            while time.monotonic() < deadline:
                event = self.receive(
                    client, start, max(.001, deadline - time.monotonic())
                )
                frames += event.get("frame_bytes", 0)
                payloads += event.get("payload_bytes", 0)
                empties += int(bool(event.get("patch_empty")))
                count += 1
                control = self.target_control(client["view"])
                active = control.get("print_preflight") or {}
                if transition["action"] == "hold":
                    matched = (
                        active.get("hold_id") == transition["hold"]
                        and active.get("status") == "running"
                    )
                else:
                    matched = (
                        active.get("hold_id") != transition["hold"]
                        and self.target_queue_state(client["view"]) == transition["queue_before"]
                    )
                if matched and client["view"]["rev"] > prior_revision:
                    return dict(
                        event, frame_bytes=frames, payload_bytes=payloads,
                        preceding_empty_events=empties,
                        applied_events=count, exact_target_transition=True,
                        revision_advance=client["view"]["rev"] - prior_revision,
                        applied_revision=client["view"]["rev"],
                    )
            raise AssertionError("Exact target mutation did not arrive on stream")
        if require_change:
            deadline = time.monotonic() + timeout
            frames, payloads, empties = 0, 0, 0
            while True:
                result = self.receive(
                    client, start, max(.001, deadline - time.monotonic())
                )
                frames += result.get("frame_bytes", 0)
                payloads += result.get("payload_bytes", 0)
                empty = result.get("patch_empty") and result["type"] in (
                    "state_delta", "state_checkpoint"
                )
                empties += int(bool(empty))
                if not empty or time.monotonic() >= deadline:
                    if empty:
                        result["type"] = "no_event"
                        result["latency_ms"] = None
                    return dict(result, frame_bytes=frames, payload_bytes=payloads,
                                preceding_empty_events=empties)
        try:
            metric, event = client["socket"].event(timeout)
        except socket.timeout:
            return {"type": "no_event", "frame_bytes": 0, "latency_ms": None}
        raw_complete = metric.pop("raw_complete_monotonic_s", None)
        if "type" not in event:
            code = event.get("error", "unknown")
            safe_code = code if isinstance(code, str) and code.isidentifier() else "withheld"
            # Real clients recover an unavailable stream through authorized HTTP.
            # Keep the error and its cost visible; do not replace a mismatching
            # successfully delivered delta with an oracle snapshot.
            assert safe_code == "store_failed", (
                "Unrecoverable stream error without a data event type: " + safe_code
            )
            attempts = client.get("error_recoveries", 0) + 1
            assert attempts <= 3, "Repeated stream errors failed to converge"
            client["socket"].close()
            snapshot_metric, snapshot = self.snapshot(client["token"])
            replacement = self.connect_state(client["token"], snapshot)
            client.update(replacement, error_recoveries=attempts)
            extra = replacement["bootstrap_reload"]
            recovery = {
                "error": safe_code, "snapshot_gets": 1 + int(extra is not None),
                "snapshot_body_bytes": snapshot_metric["body_bytes"] +
                    (extra["body_bytes"] if extra else 0),
                "elapsed_ms": (time.perf_counter() - start) * 1000,
            }
            self.result["stream_error_recovery"].append(recovery)
            # Receivers run concurrently. The owning phase/finally persists the
            # completed records, avoiding competing atomic-file replacements.
            return dict(
                metric, type="state_error_recovered", latency_ms=recovery["elapsed_ms"],
                frame_bytes=metric["frame_bytes"] + replacement["socket"].received,
                patch_empty=False, resync_gets=recovery["snapshot_gets"],
                recovery_snapshot_bytes=recovery["snapshot_body_bytes"],
                reconnect_frame_bytes=replacement["socket"].received,
            )
        kind = event["type"]
        reduce_started = time.perf_counter()
        if kind == "state_delta":
            client["view"] = apply_state(client["view"], event)
        elif kind == "state_resync":
            _, client["view"] = self.snapshot(client["token"])
        elif kind == "state_checkpoint":
            # A content-neutral cursor advance must preserve the existing view.
            client["view"]["rev"] = event["rev"]
        else:
            raise AssertionError("Unexpected stream event: " + kind)
        return dict(metric, type=kind, latency_ms=(time.perf_counter() - start) * 1000,
                    socket_read_complete_ms=(raw_complete - start) * 1000
                    if raw_complete is not None else None,
                    reducer_ms=(time.perf_counter() - reduce_started) * 1000,
                    event_revision=event.get("rev"), base_revision=event.get("base_rev"),
                    patch_empty=not event.get("patch"),
                    resync_gets=int(kind == "state_resync"))

    def target_control(self, view):
        return view.get("queue_action_controls", {}).get(self.apparatus, {}).get(self.order, {})

    def target_queue_state(self, view):
        return view.get("queue_states", {}).get(self.apparatus, {}).get(self.order)

    def assert_views(self, clients, label):
        expected = {}
        for client in clients:
            token = client["token"]
            if token not in expected:
                _, expected[token] = self.snapshot(token)
            deadline = time.monotonic() + 10
            while comparable(client["view"]) != comparable(expected[token]):
                # A commit may produce multiple invalidations. Drain all real
                # stream transitions; never repair the reducer from the oracle.
                assert time.monotonic() < deadline, (
                    "Authorized REST and applied stream state differ: " + label
                )
                event = self.receive(client, time.perf_counter(), 1)
                assert event["type"] != "no_event", (
                    "Stream did not converge to authorized REST: " + label
                )
        self.result["correctness"].append({
            "scenario": label, "clients": len(clients),
            "distinct_principals": len(expected), "exact_rest_view_match": True,
        })
        self.save()

    def fanout(self, count, mixed=False):
        profiles = self.shared_profiles if mixed else [self.actor]
        clients = [
            self.connect_state(profiles[index % len(profiles)]["token"])
            for index in range(count)
        ]
        label = ("shared_apparatus_workers" if mixed else "same_principal") + "_" + str(count)
        records = []
        try:
            with concurrent.futures.ThreadPoolExecutor(max_workers=count) as pool:
                with Phase(self, label):
                    for cycle in range(self.status_cycles):
                        hold = "bench-stream-" + str(time.time_ns())
                        queues_before = [
                            self.target_queue_state(client["view"]) for client in clients
                        ]
                        for action in ("hold", "failed"):
                            started = time.perf_counter()
                            pending = [
                                pool.submit(
                                    self.receive, client, started, 10, True,
                                    {"action": action, "hold": hold, "queue_before": queue_before},
                                )
                                for client, queue_before in zip(clients, queues_before)
                            ]
                            response, body = self.mutate(action, hold)
                            events = [future.result(timeout=20) for future in pending]
                            acknowledgement = body.get("control_state") or {}
                            for event in events:
                                event["applied_minus_control_revision"] = (
                                    event["applied_revision"] - acknowledgement["rev"]
                                    if isinstance(acknowledgement.get("rev"), int) else None
                                )
                            if any(event["type"] == "no_event" for event in events):
                                raise AssertionError("Relevant color update was not delivered")
                            records.append({
                                "action": action, "post": response,
                                "all_clients_ms": max(event["latency_ms"] for event in events),
                                "total_frame_bytes": sum(event["frame_bytes"] for event in events),
                                "events": events,
                            })
            self.assert_views(clients, label)
            self.result["fanout"].append({
                "name": label, "clients": count,
                "distinct_principals": len({client["token"] for client in clients}),
                "cycles": self.status_cycles, "records": records,
                "all_clients_latency_ms": stats([record["all_clients_ms"] for record in records]),
                "post_latency_ms": stats([record["post"]["latency_ms"] for record in records]),
                "control_returned": sum(record["post"]["control_returned"] for record in records),
                "control_null": sum(not record["post"]["control_returned"] for record in records),
                "total_frame_bytes": sum(record["total_frame_bytes"] for record in records),
            })
            self.save()
        finally:
            for client in clients:
                client["socket"].close()

    def disjoint(self):
        if not self.disjoint_profiles:
            self.result["limitations"].append("No disjoint worker scope in fixture.")
            return
        clients = [self.connect_state(profile["token"]) for profile in self.disjoint_profiles]
        records = []
        hold = "bench-disjoint-" + str(time.time_ns())
        try:
            with concurrent.futures.ThreadPoolExecutor(max_workers=len(clients)) as pool:
                with Phase(self, "other_apparatus_change"):
                    for action in ("hold", "failed"):
                        started = time.perf_counter()
                        pending = [pool.submit(self.receive, client, started, 5) for client in clients]
                        self.mutate(action, hold)
                        records.extend(future.result(timeout=15) for future in pending)
                        # Observe each mutation for the same five seconds even
                        # when every baseline socket emits immediately.
                        time.sleep(max(0, 5 - (time.perf_counter() - started)))
            # Some different-apparatus views legitimately contain this order's
            # downstream/upstream controls. Exact REST equality decides relevance.
            self.assert_views(clients, "other_apparatus_change")
            self.result["disjoint"] = {"clients": len(clients), "events": records}
            self.save()
        finally:
            for client in clients:
                client["socket"].close()

    def recovery(self):
        keeper = self.connect_state(self.actor["token"])
        returning = self.connect_state(self.actor["token"])
        saved = copy.deepcopy(returning["view"])
        returning["socket"].close()
        hold = "bench-recovery-" + str(time.time_ns())
        try:
            for action in ("hold", "failed"):
                started = time.perf_counter()
                self.mutate(action, hold)
                self.receive(keeper, started)
            started = time.perf_counter()
            socket_ = WS(self.port, self.actor["token"], {
                "protocol": "state_delta_v1", "worker_scope": "true", "queue_limit": "10",
                "epoch": saved["epoch"], "rev": saved["rev"], "scope": saved["scope"],
            })
            replayed, reloads = 0, 0
            client = {"socket": socket_, "token": self.actor["token"], "view": saved}
            try:
                while True:
                    metric, ready = socket_.event()
                    if ready["type"] == "state_ready":
                        break
                    assert ready["type"] == "state_delta", "Unexpected reconnect frame"
                    client["view"] = apply_state(client["view"], ready)
                    replayed += 1
                if ready.get("resync"):
                    _, client["view"] = self.snapshot(self.actor["token"])
                    reloads = 1
                else:
                    while client["view"]["rev"] < ready["rev"]:
                        event = self.receive(client, started, 3)
                        assert event["type"] != "no_event", "Replay announced but missing"
                        replayed += 1
                elapsed = (time.perf_counter() - started) * 1000
                self.assert_views([client], "short_disconnect")
                self.result["recovery"].append({
                    "scenario": "short_disconnect", "latency_ms": elapsed,
                    "snapshot_gets": reloads, "replayed_events": replayed,
                    "websocket_bytes": socket_.received,
                })
                self.save()
            finally:
                socket_.close()
        finally:
            keeper["socket"].close()

    def idle(self, seconds=30):
        clients = [self.connect_state(self.actor["token"]) for _ in range(max(self.clients))]
        received = sum(client["socket"].received for client in clients)
        sent = sum(client["socket"].sent for client in clients)
        try:
            with concurrent.futures.ThreadPoolExecutor(max_workers=len(clients)) as pool:
                with Phase(self, "idle_worker_streams"):
                    started = time.perf_counter()
                    events = list(pool.map(
                        lambda client: self.receive(client, started, seconds), clients
                    ))
            assert all(event["type"] == "no_event" for event in events)
            self.result["idle"] = {
                "clients": len(clients), "seconds": seconds,
                "http_gets": 0, "data_events": 0,
                "websocket_received_bytes": sum(client["socket"].received for client in clients) - received,
                "websocket_sent_bytes": sum(client["socket"].sent for client in clients) - sent,
                "note": "Bootstrap excluded; includes ping/pong and a full 25-second heartbeat interval.",
            }
        finally:
            for client in clients:
                client["socket"].close()

    def held_permissions(self):
        clients = [self.connect_state(profile["token"]) for profile in self.shared_profiles]
        other_clients = [self.connect_state(profile["token"]) for profile in self.disjoint_profiles]
        hold = "bench-permissions-" + str(time.time_ns())
        try:
            for action in ("hold", "failed"):
                started = time.perf_counter()
                self.mutate(action, hold)
                for client in clients:
                    assert self.receive(client, started, require_change=True)["type"] != "no_event"
                self.assert_views(clients, "distinct_worker_controls_" + action)
                if other_clients:
                    with concurrent.futures.ThreadPoolExecutor(max_workers=len(other_clients)) as pool:
                        list(pool.map(lambda client: self.receive(client, started, 2), other_clients))
                # In particular, verify the active hold, not just the reverted
                # end state: incorrect relevance filtering must fail this check.
                self.assert_views(other_clients, "other_apparatus_controls_" + action)
        finally:
            for client in clients + other_clients:
                client["socket"].close()

    def load_pages(self, view="sequence", apparatus=None, length=40):
        pages, metrics, cursor = [], [], ""
        while True:
            metric, page = self.get(self.admin, PAGE + "?" + urlencode({
                "view": view, "apparatus": self.apparatus if apparatus is None else apparatus,
                "target": "", "search": "", "limit": 20, "cursor": cursor,
            }))
            pages.append(page)
            metrics.append(metric)
            cursor = page.get("next_cursor")
            if not cursor or len(pages) * 20 >= length:
                return pages, metrics

    def apply_page(self, pages, event):
        assert event["base_version"] == pages[0]["version"]
        if event["type"] == "page_window_checkpoint":
            assert len(event["next_cursors"]) == len(pages)
            for index, page in enumerate(pages):
                page.update({key: event[key] for key in ("epoch", "rev", "version")})
                page["next_cursor"] = event["next_cursors"][index]
            return pages
        assert event["type"] == "page_window_delta"
        pages = copy.deepcopy(pages[:event["page_count"]])
        while len(pages) < event["page_count"]:
            pages.append({})
        for change in event["changes"]:
            for op in change["ops"]:
                path = op["path"]
                if not path:
                    assert op["op"] == "set"
                    pages[change["index"]] = op["value"]
                    continue
                target = pages[change["index"]]
                for component in path[:-1]:
                    target = target[component]
                if op["op"] == "set":
                    target[path[-1]] = op["value"]
                else:
                    del target[path[-1]]
        return pages

    def admin_page(self, view="sequence", apparatus=None, length=40, verify_each=False):
        pages, bootstrap = self.load_pages(view, apparatus, length)
        request = {
            "id": "1", "view": view,
            "apparatus": self.apparatus if apparatus is None else apparatus,
            "target": "", "search": "", "length": length,
            "version": pages[0]["version"],
        }
        query = (
            {"protocol": "delta", "paged": "true"}
            if self.legacy_protocols
            else {"protocol": "page_delta_v1", "pages": json.dumps([request])}
        )
        socket_ = WS(self.port, self.admin, query)
        _, ready = socket_.event()
        assert ready["type"] == ("page_ready" if self.legacy_protocols else "page_window_ready")
        assert not ready.get("resync")
        label = "admin_" + view + "_" + str(length)
        if verify_each:
            label += "_exact_each_event"
        records = []
        try:
            with (contextlib.nullcontext() if verify_each else Phase(self, label)):
                for cycle in range(1 if verify_each else self.cycles):
                    hold = "bench-page-" + str(time.time_ns())
                    for action in ("hold", "failed"):
                        started = time.perf_counter()
                        response, _ = self.mutate(action, hold)
                        wire, event = socket_.event()
                        kind = event["type"]
                        metrics = []
                        if kind in ("page_invalidated", "page_window_resync"):
                            pages, metrics = self.load_pages(view, apparatus, length)
                        else:
                            pages = self.apply_page(pages, event)
                        records.append({
                            "event": kind, "post": response, "websocket": wire,
                            "reload_gets": len(metrics),
                            "reload_body_bytes": sum(item["body_bytes"] for item in metrics),
                            "latency_ms": (time.perf_counter() - started) * 1000,
                        })
                        if verify_each:
                            expected, _ = self.load_pages(view, apparatus, length)
                            while pages != expected:
                                _, extra = socket_.event(3)
                                if extra["type"] in ("page_invalidated", "page_window_resync"):
                                    pages, _ = self.load_pages(view, apparatus, length)
                                else:
                                    pages = self.apply_page(pages, extra)
                            self.result["correctness"].append({
                                "scenario": label + "_" + action,
                                "exact_rest_page_match": True,
                            })
            expected, _ = self.load_pages(view, apparatus, length)
            assert pages == expected, "Applied admin pages differ from authoritative HTTP pages"
            self.result["correctness"].append({
                "scenario": label, "pages": len(pages), "exact_rest_page_match": True,
            })
            self.result["admin_pages"].append({
                "name": label, "loaded_pages": len(pages), "requested_length": length,
                "bootstrap_gets": len(bootstrap),
                "bootstrap_body_bytes": sum(item["body_bytes"] for item in bootstrap),
                "records": records,
                "reload_gets": sum(item["reload_gets"] for item in records),
                "total_body_and_frame_bytes": sum(
                    item["reload_body_bytes"] + item["websocket"]["frame_bytes"]
                    for item in records
                ),
                "latency_ms": stats([item["latency_ms"] for item in records]),
            })
            self.save()
        finally:
            socket_.close()

    def notifications(self):
        query = {} if self.legacy_protocols else {"protocol": "notifications_v1"}
        started = time.perf_counter()
        with Phase(self, "notifications_bootstrap"):
            socket_ = WS(self.port, self.admin, query)
            metric, payload = socket_.event()
            elapsed = (time.perf_counter() - started) * 1000
        try:
            assert "completion_requests" in payload
            self.result["notifications"] = {
                "bootstrap_ms": elapsed, "payload_bytes": metric["payload_bytes"],
                "frame_bytes": metric["frame_bytes"],
                "map_count_sent": len(payload.get("maps", [])),
                "completion_request_count": len(payload["completion_requests"]),
                "event": payload.get("type", "full_snapshot"),
            }
            self.save()
        finally:
            socket_.close()

    def reorder(self):
        clients = [self.connect_state(profile["token"]) for profile in self.shared_profiles]
        pages, _ = self.load_pages()
        sequence = clients[0]["view"]["sequences"][self.apparatus]
        states = clients[0]["view"]["queue_states"].get(self.apparatus, {})
        pair = next((
            (first, second) for first, second in zip(sequence, sequence[1:])
            if all(states.get(order) not in ("in_progress", "paused", "print_preflight", "frozen")
                   for order in (first, second))
        ), None)
        if pair is None:
            raise RuntimeError("Fixture lacks an adjacent movable order pair")
        request = {
            "id": "1", "view": "sequence", "apparatus": self.apparatus,
            "target": "", "search": "", "length": 40, "version": pages[0]["version"],
        }
        query = ({"protocol": "delta", "paged": "true"} if self.legacy_protocols else {
            "protocol": "page_delta_v1", "pages": json.dumps([request]),
        })
        socket_ = WS(self.port, self.admin, query)
        _, ready = socket_.event()
        assert not ready.get("resync")
        records = []
        try:
            for index, anchor_key in enumerate(("after_order_id", "before_order_id")):
                with Phase(self, "reorder_" + str(index)):
                    started = time.perf_counter()
                    http = HTTP(self.port, self.admin)
                    try:
                        post, response = http.request(SNAPSHOT, "POST", {
                            "apparatus": self.apparatus, "order_id": pair[0],
                            anchor_key: pair[1],
                            "expected_version": clients[0]["view"]["sequence_versions"][self.apparatus],
                            "idempotency_key": "bench-reorder-" + str(time.time_ns()),
                        })
                    finally:
                        http.close()
                    assert response.get("ops"), "Fixture move did not alter order"
                    events = [self.receive(client, started, require_change=True) for client in clients]
                    assert all(event["type"] != "no_event" for event in events)
                    wire, event = socket_.event()
                    metrics = []
                    if event["type"] in ("delta", "page_invalidated", "page_window_resync"):
                        pages, metrics = self.load_pages()
                    else:
                        pages = self.apply_page(pages, event)
                    records.append({
                        "post": post, "worker_frame_bytes": sum(e["frame_bytes"] for e in events),
                        "admin_event": event["type"], "admin_frame_bytes": wire["frame_bytes"],
                        "reload_gets": len(metrics),
                        "reload_body_bytes": sum(m["body_bytes"] for m in metrics),
                        "all_views_latency_ms": (time.perf_counter() - started) * 1000,
                    })
                self.assert_views(clients, "reorder_" + str(index))
                expected, _ = self.load_pages()
                assert pages == expected, "Reordered admin pages differ from HTTP"
                self.result["correctness"].append({
                    "scenario": "admin_reorder_" + str(index), "exact_rest_page_match": True,
                })
            self.result["reorder"] = {
                "distinct_worker_principals": len(clients), "records": records,
                "restored_original_order": clients[0]["view"]["sequences"][self.apparatus] == sequence,
            }
            assert self.result["reorder"]["restored_original_order"]
            self.save()
        finally:
            for client in clients:
                client["socket"].close()
            socket_.close()

    def run(self, query_counts_only=False, latency_repeat=False, status_repeat=False):
        self.result["command_config"].update(
            query_counts_only=query_counts_only, latency_repeat=latency_repeat,
            status_repeat=status_repeat,
        )
        try:
            self.profiles()
            if latency_repeat or status_repeat:
                assert not self.server["query_log"]
                assert self.server["mini_order_sync_interval_seconds"] == (
                    0 if latency_repeat else 30
                )
                self.result["measurement_variant"] = (
                    "controlled_latency_repeat" if latency_repeat
                    else "default_config_status_repeat"
                )
                self.fanout(1)
                self.fanout(max(self.clients), mixed=True)
                self.held_permissions()
                return
            if query_counts_only:
                assert self.server["query_log"], "SQL diagnostic requires --query-log fixture"
                for count in self.clients:
                    self.fanout(count)
                self.fanout(max(self.clients), mixed=True)
                self.disjoint()
                self.admin_page()
                return
            self.idle()
            for count in self.clients:
                self.fanout(count)
            self.fanout(max(self.clients), mixed=True)
            self.held_permissions()
            self.disjoint()
            self.recovery()
            self.admin_page()
            self.admin_page(view="orders", apparatus="", length=60)
            self.admin_page(verify_each=True)
            self.admin_page(view="orders", apparatus="", length=60, verify_each=True)
            self.reorder()
            self.notifications()
        finally:
            for token in self.sessions:
                client = HTTP(self.port, token)
                try:
                    client.request("/v1/mobile/auth/logout", "POST", {})
                except Exception:
                    pass
                finally:
                    client.close()
            self.save()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-dir", required=True)
    parser.add_argument("--mode", choices=("before", "after"), required=True)
    parser.add_argument("--clients", default="1,8,32")
    parser.add_argument("--cycles", type=int, default=5)
    parser.add_argument("--status-cycles", type=int, help="Override worker status cycles only")
    parser.add_argument("--protocols", choices=("auto", "legacy", "modern"), default="auto")
    variant = parser.add_mutually_exclusive_group()
    variant.add_argument("--query-counts-only", action="store_true")
    variant.add_argument("--latency-repeat", action="store_true")
    variant.add_argument("--status-repeat", action="store_true",
                         help="Repeat 1-client and shared-principal status phases at default30s sync")
    parser.add_argument("--output-suffix", default="results")
    args = parser.parse_args()
    clients = [int(value) for value in args.clients.split(",")]
    if any(value < 1 or value > 128 for value in clients) or not 1 <= args.cycles <= 50:
        parser.error("Use 1..128 clients and 1..50 cycles")
    if args.status_cycles is not None and not 1 <= args.status_cycles <= 50:
        parser.error("Use 1..50 status cycles")
    if not re.fullmatch(r"[a-z0-9-]+", args.output_suffix):
        parser.error("Output suffix accepts lowercase letters, digits and hyphens")
    Runner(Fixture(args.run_dir), args.mode, clients, args.cycles, args.output_suffix,
           args.protocols, args.status_cycles).run(
        query_counts_only=args.query_counts_only, latency_repeat=args.latency_repeat,
        status_repeat=args.status_repeat,
    )


if __name__ == "__main__":
    main()
