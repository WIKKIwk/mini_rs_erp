#!/usr/bin/env python3
"""Actual two-OS-process PostgreSQL stream correctness check, clones only."""
import argparse
import copy
import json
from pathlib import Path
import shutil
import subprocess
import time
import urllib.request
from urllib.parse import urlencode

from order_stream_benchmark import HTTP, WS, LIVE, PAGE, SNAPSHOT, Runner, comparable
from order_stream_fixture import Fixture, private_json


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-dir", required=True)
    args = parser.parse_args()
    fixture = Fixture(args.run_dir)
    assert not fixture.metadata["servers"].get("before", {}).get("ready")
    runner = Runner(fixture, "after", [1], 1, "crossprocess-setup")
    runner.profiles()
    directory = fixture.run_dir / "replica"
    directory.mkdir(mode=0o700, exist_ok=True)
    if (directory / "data").exists():
        shutil.rmtree(directory / "data")
    # Role/config files are copied explicitly; the notification bus replicates
    # PostgreSQL domain changes, not deployment-local role configuration.
    shutil.copytree(fixture.run_dir / "after" / "data", directory / "data")
    executable = directory / "mini_rs_erp"
    shutil.copy2(fixture.run_dir / "after" / "mini_rs_erp", executable)
    executable.chmod(0o700)
    log_path = directory / "server.log"
    with open(log_path, "wb") as log:
        process = subprocess.Popen(
            [str(executable)], cwd=directory,
            env=fixture.server_env(runner.server["database"], 18091),
            stdout=log, stderr=log, start_new_session=True,
        )
    log_path.chmod(0o600)
    private_json(directory / "process.json", {"pid": process.pid, "binary": str(executable)})
    sockets, replica_tokens = [], []
    report = {
        "candidate_revision": runner.server["source_revision"],
        "candidate_binary_sha256": runner.server["binary_sha256"],
        "independent_os_processes": 2, "independent_logins": True,
        "shared_clone_database": True, "copied_role_config": True,
        "records": [],
    }

    def get(token, path):
        client = HTTP(18091, token)
        try:
            return client.request(path)[1]
        finally:
            client.close()

    def pages(token):
        result, cursor = [], ""
        while True:
            page = get(token, PAGE + "?" + urlencode({
                "view": "sequence", "apparatus": runner.apparatus,
                "limit": 20, "cursor": cursor,
            }))
            result.append(page)
            cursor = page.get("next_cursor")
            if not cursor or len(result) == 2:
                return result

    try:
        for attempt in range(120):
            if process.poll() is not None:
                raise RuntimeError("Replica failed to start; private log retained")
            try:
                with urllib.request.urlopen("http://127.0.0.1:18091/healthz", timeout=1) as health:
                    if health.status == 200:
                        break
            except OSError:
                time.sleep(.25)
        else:
            raise RuntimeError("Replica did not become ready")
        for credentials in (
            runner.actor["credentials"], fixture.admin_credentials("after"),
            runner.shared_profiles[-1]["credentials"],
        ):
            client = HTTP(18091)
            try:
                _, login = client.request("/v1/mobile/auth/login", "POST", credentials)
                replica_tokens.append(login["token"])
            finally:
                client.close()
        actor_token, admin_token, other_worker_token = replica_tokens
        workers = []
        for token in (actor_token, other_worker_token):
            snapshot = get(token, SNAPSHOT + "?worker_scope=true&queue_limit=10")
            stream = WS(18091, token, {
                "protocol": "state_delta_v1", "worker_scope": "true", "queue_limit": 10,
                "epoch": snapshot["epoch"], "rev": snapshot["rev"], "scope": snapshot["scope"],
            })
            sockets.append(stream)
            _, ready = stream.event()
            assert ready["type"] == "state_ready" and not ready["resync"]
            workers.append({"socket": stream, "token": token, "view": snapshot})
        window = pages(admin_token)
        page_stream = WS(18091, admin_token, {
            "protocol": "page_delta_v1", "pages": json.dumps([{
                "id": "1", "view": "sequence", "apparatus": runner.apparatus,
                "target": "", "search": "", "length": 40,
                "version": window[0]["version"],
            }]),
        })
        sockets.append(page_stream)
        _, ready = page_stream.event()
        assert ready["type"] == "page_window_ready" and not ready["resync"]

        def converge(label, started):
            nonlocal window
            for index, worker in enumerate(workers):
                # The same controller applies actual frames. A resync must use
                # the replica's own epoch/REST state, never process A's cursor.
                deadline = time.monotonic() + 15
                primary_token = runner.actor["token"] if index == 0 else runner.shared_profiles[-1]["token"]
                _, primary = runner.snapshot(primary_token)
                while comparable(worker["view"]) != comparable(primary):
                    assert time.monotonic() < deadline, "Replica worker remained stale"
                    metric, event = worker["socket"].event(5)
                    if event["type"] == "state_resync":
                        raise AssertionError("Unexpected replica resync during live mutation")
                    from order_stream_benchmark import apply_state
                    worker["view"] = apply_state(worker["view"], event)
                expected = get(worker["token"], SNAPSHOT + "?worker_scope=true&queue_limit=10")
                assert comparable(worker["view"]) == comparable(expected)
                assert comparable(worker["view"]) == comparable(primary), (
                    "Process A/B authorized worker state differs"
                )
            expected_pages = pages(admin_token)
            deadline = time.monotonic() + 15
            while window != expected_pages:
                assert time.monotonic() < deadline, "Replica administrator window remained stale"
                _, event = page_stream.event(5)
                if event["type"] == "page_window_resync":
                    raise AssertionError("Unexpected replica window resync")
                window = runner.apply_page(window, event)
                expected_pages = pages(admin_token)
            primary_pages, _ = runner.load_pages()
            strip_metadata = lambda values: [
                {key: value for key, value in page.items()
                 if key not in ("epoch", "rev", "version", "next_cursor")}
                for page in values
            ]
            assert strip_metadata(window) == strip_metadata(primary_pages)
            report["records"].append({
                "scenario": label, "convergence_ms": (time.perf_counter() - started) * 1000,
                "worker_principals": len(workers), "admin_pages": len(window),
                "replica_stream_equals_replica_http": True,
                "replica_stream_equals_primary_http_content": True,
            })
            private_json(fixture.run_dir / "crossprocess-results.json", report)

        hold = "bench-two-process-" + str(time.time_ns())
        for action in ("hold", "failed"):
            started = time.perf_counter()
            runner.mutate(action, hold)
            converge(action, started)
        _, current = runner.snapshot(runner.actor["token"])
        sequence = copy.deepcopy(current["sequences"][runner.apparatus])
        states = current["queue_states"].get(runner.apparatus, {})
        first, second = next(
            (a, b) for a, b in zip(sequence, sequence[1:])
            if all(states.get(order) not in ("in_progress", "paused", "print_preflight", "frozen")
                   for order in (a, b))
        )
        for anchor in ("after_order_id", "before_order_id"):
            started = time.perf_counter()
            _, current = runner.snapshot(runner.actor["token"])
            client = HTTP(runner.port, runner.admin)
            try:
                _, moved = client.request(SNAPSHOT, "POST", {
                    "apparatus": runner.apparatus, "order_id": first, anchor: second,
                    "expected_version": current["sequence_versions"][runner.apparatus],
                    "idempotency_key": "bench-two-process-" + str(time.time_ns()),
                })
                assert moved["ops"], "Expected an actual cross-process order move"
            finally:
                client.close()
            converge("move_" + anchor, started)
        _, current = runner.snapshot(runner.actor["token"])
        assert current["sequences"][runner.apparatus] == sequence
        report["restored_original_order"] = True
        private_json(fixture.run_dir / "crossprocess-results.json", report)
        print(json.dumps({
            "two_process_check": "passed", "actions": len(report["records"]),
            "worker_principals": len(workers), "admin_pages": len(window),
        }))
    finally:
        for stream in sockets:
            stream.close()
        for token in replica_tokens:
            client = HTTP(18091, token)
            try:
                client.request("/v1/mobile/auth/logout", "POST", {})
            except Exception:
                pass
            finally:
                client.close()
        process.terminate()
        try:
            process.wait(timeout=15)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
        private_json(directory / "process.json", {"pid": process.pid, "stopped": True})


if __name__ == "__main__":
    main()
