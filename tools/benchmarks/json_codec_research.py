#!/usr/bin/env python3
"""Run the same locked research binary with Serde/Sonic settings in isolation.

Only fake data and loopback sockets are used. The fixture refuses project and
deployment working directories. Raw samples, source/binary hashes and host
metadata are saved so slower cases remain visible alongside improvements.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import statistics
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
PROFILES = {
    "serde_json": ("serde_json", "serde_json"),
    "sonic_encode": ("sonic_rs", "serde_json"),
    "sonic_both": ("sonic_rs", "sonic_rs"),
}


def command(*args: str) -> str:
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def digest(path: Path) -> str:
    result = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            result.update(block)
    return result.hexdigest()


def environment(profile: str) -> dict[str, str]:
    # Keep the OS/tool runtime, discard application/deployment credentials and
    # feature overrides before starting a memory-only AppState in a fresh cwd.
    prefixes = ("MINI_ERP_", "MOBILE_", "FCM_", "TELEGRAM_", "RPS_", "PG", "GSCALE_", "GOOGLE_")
    result = {key: value for key, value in os.environ.items()
              if not key.startswith(prefixes) and key not in ("DATABASE_URL", "RUST_LOG")}
    result["MINI_ERP_RESEARCH_JSON_ENCODER"], result["MINI_ERP_RESEARCH_JSON_DECODER"] = PROFILES[profile]
    return result


def host() -> dict:
    result = {"platform": platform.platform(), "machine": platform.machine(),
              "rustc": command("rustc", "--version"), "cargo": command("cargo", "--version")}
    if platform.system() == "Darwin":
        for key in ("machdep.cpu.brand_string", "hw.ncpu", "hw.memsize", "hw.model"):
            result[key] = command("sysctl", "-n", key)
    return result


def process_metrics(stderr: str, elapsed: float) -> dict:
    result = {"wall_seconds": elapsed}
    found = re.search(r"(\d+)\s+maximum resident set size", stderr)
    if found:
        result["peak_rss_bytes"] = int(found.group(1))
    found = re.search(r"Maximum resident set size \(kbytes\):\s*(\d+)", stderr)
    if found:
        result["peak_rss_bytes"] = int(found.group(1)) * 1024
    return result


def aggregate(runs: list[dict]) -> list[dict]:
    groups: dict[tuple, list[float]] = {}
    for run in runs:
        for case in run["cases"]:
            if "timing" not in case:
                continue
            groups.setdefault((run["orders"], case["case"], run["profile"]), []).append(case["timing"]["p50"])
    rows = []
    for orders, case, profile in sorted(groups):
        values = groups[(orders, case, profile)]
        baseline = statistics.median(groups[(orders, case, "serde_json")])
        current = statistics.median(values)
        rows.append({"orders": orders, "case": case, "profile": profile,
                     "median_of_run_p50_us": current, "baseline_us": baseline,
                     "speedup": baseline / current, "time_change_percent": (current / baseline - 1) * 100,
                     "run_p50_us": values})
    return rows


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--orders", nargs="+", type=int, default=[20, 285, 1000])
    parser.add_argument("--samples", type=int, default=31)
    parser.add_argument("--batch", type=int, default=10)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--build-flags", default="unspecified")
    args = parser.parse_args()
    binary, output = args.binary.resolve(), args.output.resolve()
    if not binary.is_file() or args.repeats < 1:
        parser.error("existing binary and repeats >= 1 required")
    output.mkdir(parents=True, exist_ok=True)
    metadata = {"schema": 1, "synthetic": True, "host": host(), "binary_sha256": digest(binary),
                "build_flags": args.build_flags, "source_head": command("git", "rev-parse", "HEAD"),
                "tracked_diff_sha256": hashlib.sha256(subprocess.check_output(["git", "diff", "--binary"], cwd=ROOT)).hexdigest(),
                "cargo_lock_sha256": digest(ROOT / "Cargo.lock"),
                "parameters": {"orders": args.orders, "samples": args.samples, "batch": args.batch, "repeats": args.repeats}}
    research_sources = [ROOT / "examples/json_codec_research.rs", ROOT / "src/http/json_research.rs", Path(__file__).resolve()]
    research_sources.extend(sorted((ROOT / "examples/json_codec_research").glob("*.rs")))
    metadata["research_source_sha256"] = {str(path.relative_to(ROOT)): digest(path) for path in research_sources}
    (output / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
    profiles = list(PROFILES)
    runs = []
    for orders in args.orders:
        for repeat in range(args.repeats):
            order = profiles[repeat % len(profiles):] + profiles[:repeat % len(profiles)]
            for profile in order:
                name = f"orders-{orders}-run-{repeat + 1}-{profile}"
                timer = ["/usr/bin/time", "-l"] if platform.system() == "Darwin" else ["/usr/bin/time", "-v"]
                with tempfile.TemporaryDirectory(prefix="accord-json-research-") as work:
                    started = time.perf_counter()
                    process = subprocess.run(timer + [str(binary), "--synthetic-fixture", "--orders", str(orders),
                        "--samples", str(args.samples), "--batch", str(args.batch)],
                        cwd=work, env=environment(profile), text=True, capture_output=True, timeout=900)
                    elapsed = time.perf_counter() - started
                if process.returncode:
                    (output / f"{name}-failure.txt").write_text(process.stdout + process.stderr)
                    raise RuntimeError(f"{name} failed; see {output / (name + '-failure.txt')}")
                result = json.loads(process.stdout.strip().splitlines()[-1])
                result.update({"profile": profile, "repeat": repeat + 1,
                               "process": process_metrics(process.stderr, elapsed)})
                (output / f"{name}.json").write_text(json.dumps(result, indent=2) + "\n")
                runs.append(result)
                (output / "summary.json").write_text(json.dumps(aggregate(runs), indent=2) + "\n")
                print(f"{name}: {elapsed:.2f}s", flush=True)
    print(f"Saved {len(runs)} runs and raw samples in {output}")


if __name__ == "__main__":
    main()
