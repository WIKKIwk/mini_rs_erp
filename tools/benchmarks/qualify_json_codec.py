#!/usr/bin/env python3
"""Run compiled codec/stream tests in fresh directories with isolated app settings.

Pass Cargo's --no-run --message-format=json output. This never connects to a
production database; verification fixtures use memory stores and loopback TCP.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import platform
import re
import subprocess
import tempfile
import time

from json_codec_research import ROOT, command, digest, environment, host, process_metrics

TESTS = (
    "json_codec_research", "json_codec_qualification", "active_paddon_snapshot_cache",
    "worker_queue_window", "json_codec_stream_load",
)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifacts", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--profiles", nargs="+", choices=["serde_json", "sonic_encode", "sonic_both"],
                        default=["serde_json", "sonic_encode"])
    parser.add_argument("--load-updates", type=int, choices=range(32, 1025), default=128, metavar="32..1024")
    args = parser.parse_args()
    binaries = {}
    for line in args.artifacts.read_text().splitlines():
        item = json.loads(line)
        if item.get("reason") == "compiler-artifact" and item.get("executable"):
            name = item["target"]["name"]
            if name in TESTS:
                binaries[name] = Path(item["executable"]).resolve()
    if set(binaries) != set(TESTS):
        parser.error(f"missing test artifacts: {sorted(set(TESTS) - set(binaries))}")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    report = {"synthetic": True, "host": host(), "source_head": command("git", "rev-parse", "HEAD"),
              "tracked_diff_sha256": hashlib.sha256(subprocess.check_output(
                  ["git", "diff", "--binary"], cwd=ROOT)).hexdigest(),
              "cargo_lock_sha256": digest(ROOT / "Cargo.lock"), "load_updates": args.load_updates,
              "source_sha256": {str(path.relative_to(ROOT)): digest(path) for path in [
                  ROOT / "Cargo.toml", ROOT / "src/http/json_research.rs", Path(__file__).resolve(),
                  ROOT / "examples/json_codec_research/fixture.rs",
                  *(ROOT / f"tests/{name}.rs" for name in TESTS),
                  *sorted((ROOT / "tests/worker_queue_window").glob("*.rs")),
              ]},
              "binaries": {name: digest(path) for name, path in binaries.items()}, "runs": []}
    for profile in args.profiles:
        for name in TESTS:
            env = environment(profile)
            # Exercise the production encoder setting; only the decoder uses
            # the experimental switch in the explicit sonic_both profile.
            env["MINI_ERP_JSON_ENCODER"] = env.pop("MINI_ERP_RESEARCH_JSON_ENCODER")
            env["MINI_TEST_JSON_LOAD_UPDATES"] = str(args.load_updates)
            started = time.monotonic()
            timer = ["/usr/bin/time", "-l"] if platform.system() == "Darwin" else ["/usr/bin/time", "-v"]
            with tempfile.TemporaryDirectory(prefix="accord-codec-qualification-") as work:
                result = subprocess.run([*timer, str(binaries[name]), "--nocapture", "--test-threads=2"],
                    cwd=work, env=env, capture_output=True, text=True, timeout=240)
            text = result.stdout + result.stderr
            (output / f"{profile}-{name}.log").write_text(text.rstrip() + "\n")
            counts = re.search(r"test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored", text)
            run = {"profile": profile, "test": name, "exit_code": result.returncode,
                   **process_metrics(result.stderr, time.monotonic() - started)}
            if counts:
                run.update({key: int(value) for key, value in zip(
                    ["passed", "failed", "ignored"], counts.groups()[1:])})
            report["runs"].append(run)
            (output / "verification.json").write_text(json.dumps(report, indent=2) + "\n")
            print(f"{profile}/{name}: {result.returncode}, {run['wall_seconds']:.2f}s", flush=True)
            if result.returncode or not counts or int(counts[2]) == 0:
                raise RuntimeError(f"qualification failed: {profile}/{name}; see {output}")


if __name__ == "__main__":
    main()
