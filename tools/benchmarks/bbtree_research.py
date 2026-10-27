#!/usr/bin/env python3
"""Build opt-in collection variants and compare deterministic no-DB workloads.

All compilation finishes before measurements. Latency binaries have no allocation
instrumentation; separately compiled binaries measure allocator calls and bytes.
The script does not read .env, connect to PostgreSQL, or launch the ERP server.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]
ARTIFACTS = ROOT / "target" / "bbtree-research"
VARIANTS = {
    "std": "bbtree-bench",
    "bbtree-all": "bbtree-bench,bbtree-experiment",
    "std-alloc": "bbtree-alloc",
    "bbtree-all-alloc": "bbtree-alloc,bbtree-experiment",
}


def command(args: list[str]) -> str:
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def source_sha256() -> str:
    paths = command(["git", "ls-files", "-z", "src", "crates", "third_party",
                     "examples", "Cargo.toml", "Cargo.lock"]).split("\0")
    digest = hashlib.sha256()
    for name in sorted(filter(None, paths)):
        path = ROOT / name
        digest.update(name.encode() + b"\0" + path.read_bytes() + b"\0")
    return digest.hexdigest()


def build() -> dict:
    ARTIFACTS.mkdir(parents=True, exist_ok=True)
    builds = {}
    source = source_sha256()
    revision = command(["git", "rev-parse", "HEAD"])
    for label, features in VARIANTS.items():
        log = ARTIFACTS / f"build-{label}.log"
        started = time.monotonic()
        print(f"Building {label} ({features})", flush=True)
        args = ["cargo", "build", "--locked", "--release", "--example",
                "bbtree_research", "--features", features]
        with log.open("w") as output:
            result = subprocess.run(args, cwd=ROOT, stdout=output, stderr=subprocess.STDOUT)
        if result.returncode:
            raise RuntimeError(f"Build failed: {log}\n{log.read_text()[-9000:]}")
        if source_sha256() != source:
            raise RuntimeError("Source changed during compilation; rebuild before measuring")
        binary = ARTIFACTS / label
        shutil.copy2(ROOT / "target" / "release" / "examples" / "bbtree_research", binary)
        builds[label] = {"features": features, "binary_sha256": sha256(binary),
                         "build_seconds": time.monotonic() - started,
                         "source_commit": revision, "source_tree_sha256": source}
    (ARTIFACTS / "builds.json").write_text(json.dumps(builds, indent=2) + "\n")
    return builds


def run(label: str, args: list[str]) -> dict:
    result = subprocess.run([str(ARTIFACTS / label), *args], cwd=ROOT,
                            capture_output=True, text=True, timeout=180)
    if result.returncode:
        raise RuntimeError(f"{label} failed:\n{result.stderr}\n{result.stdout}")
    return json.loads(result.stdout)


def keyed(data: dict) -> dict:
    result = {(row["name"], row["size"]): row for row in data["results"]}
    if len(result) != len(data["results"]):
        raise RuntimeError("Duplicate workload name/size keys")
    return result


def require_same(reference: dict, candidate: dict) -> None:
    left, right = keyed(reference), keyed(candidate)
    if left.keys() != right.keys():
        raise RuntimeError("Different workload coverage between variants")
    for key in left:
        if left[key]["fingerprint"] != right[key]["fingerprint"]:
            raise RuntimeError(f"Output mismatch for {key}: "
                               f"{left[key]['fingerprint']} != {right[key]['fingerprint']}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--skip-build", action="store_true")
    parser.add_argument("--build-only", action="store_true")
    parser.add_argument("--rounds", type=int, default=6)
    parser.add_argument("--target-ms", type=int, default=200)
    parser.add_argument("--output", type=Path,
                        default=ROOT / "tools/benchmarks/bbtree_results_2026-10-25.json")
    args = parser.parse_args()
    if args.rounds < 2 or args.target_ms < 1:
        parser.error("use at least two rounds and a positive measurement duration")
    builds = json.loads((ARTIFACTS / "builds.json").read_text()) if args.skip_build else build()
    for label in VARIANTS:
        if sha256(ARTIFACTS / label) != builds[label]["binary_sha256"]:
            raise RuntimeError(f"Binary changed since build: {label}")
        if source_sha256() != builds[label]["source_tree_sha256"]:
            raise RuntimeError(f"Source changed since build: {label}")
    if args.build_only:
        return

    print("Checking exact output fingerprints before measurement", flush=True)
    reference = run("std", ["--iterations", "1"])
    for label in ("bbtree-all", "std-alloc", "bbtree-all-alloc"):
        require_same(reference, run(label, ["--iterations", "1"]))

    rounds = []
    for round_index in range(args.rounds):
        order = ["std", "bbtree-all"]
        if round_index % 2:
            order.reverse()
        results = {}
        for label in order:
            print(f"Latency round {round_index + 1}/{args.rounds}: {label}", flush=True)
            results[label] = run(label, ["--target-ms", str(args.target_ms)])
            require_same(reference, results[label])
        rounds.append({"order": order, "results": results})

    print("Measuring allocations separately (instrumented timings are excluded)", flush=True)
    allocations = {label: run(label, ["--iterations", "3"])
                   for label in ("std-alloc", "bbtree-all-alloc")}
    for data in allocations.values():
        require_same(reference, data)

    summary = []
    for name, size in keyed(reference):
        samples = {
            label: [keyed(round_["results"][label])[(name, size)]["ns_per_iteration"]
                    for round_ in rounds]
            for label in ("std", "bbtree-all")
        }
        medians = {label: statistics.median(values) for label, values in samples.items()}
        summary.append({
            "name": name, "size": size,
            "std_median_ns": medians["std"],
            "bbtree_median_ns": medians["bbtree-all"],
            "speedup_std_over_bbtree": medians["std"] / medians["bbtree-all"],
            "std_range_ns": [min(samples["std"]), max(samples["std"])],
            "bbtree_range_ns": [min(samples["bbtree-all"]), max(samples["bbtree-all"])],
            "fingerprint": keyed(reference)[(name, size)]["fingerprint"],
            "allocations": {
                label: keyed(data)[(name, size)]["allocations"]
                for label, data in allocations.items()
            },
        })
    report = {
        "schema": 1,
        "source_commit": command(["git", "rev-parse", "HEAD"]),
        "source_dirty": bool(command(["git", "status", "--porcelain"])),
        "host": {"platform": platform.platform(), "machine": platform.machine(),
                 "logical_cpus": os.cpu_count(), "rustc": command(["rustc", "--version"]),
                 "cargo": command(["cargo", "--version"])},
        "method": {"rounds": args.rounds, "target_ms_per_case": args.target_ms,
                   "build_profile": "release", "warmup_iterations": 1,
                   "ordering": "alternating whole-process baseline/candidate",
                   "scope": "synthetic CPU domain helpers; no SQL, HTTP, TLS or devices",
                   "allocations": "separate binaries; requested bytes are not RSS",
                   "fingerprints": "exact SHA256 of complete deterministic helper outputs",
                   "fixture_setup": "outside timing; all fixture cases are resident"},
        "builds": builds, "summary": summary, "rounds": rounds,
        "allocation_runs": allocations,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(f"Saved {args.output}", flush=True)
    for row in summary:
        print(f"{row['name']:44} n={row['size']:5} "
              f"std={row['std_median_ns']/1e6:9.3f}ms "
              f"bbtree={row['bbtree_median_ns']/1e6:9.3f}ms "
              f"speedup={row['speedup_std_over_bbtree']:.3f}x", flush=True)


if __name__ == "__main__":
    main()
