"""Enforce Rust hygiene without rewriting unrelated pre-existing main code.

Formatting exceptions apply only to exact old file contents. Edited/new Rust
must pass rustfmt. New production files are limited to 500 lines; legacy files
cannot exceed their pre-change size after normal rustfmt expansion. The pinned
baseline is generated from main d52d332, never from this optimization branch.
"""
from __future__ import annotations

import hashlib
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]


def formatting_errors(output: str, root: Path, baseline: dict) -> list[str]:
    errors = []
    if re.search(r"^(error(?:\[[^\]]+\])?:|Error writing files:)", output, re.MULTILINE):
        return ["rustfmt could not parse/check source: " + output[-2000:]]
    paths = re.findall(r"^Diff in (.+?):\d+:?\s*$", output, re.MULTILINE)
    for name in dict.fromkeys(paths):
        path = Path(name)
        if not path.is_absolute():
            path = root / path
        relative = path.resolve().relative_to(root.resolve()).as_posix()
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        if baseline.get("formatting_exemptions", {}).get(relative) != digest:
            errors.append(f"rustfmt: {relative}")
    if not paths:
        errors.append("rustfmt failed without a recognized formatting diff: " + output[-2000:])
    return errors


def size_errors(root: Path, baseline: dict) -> list[str]:
    errors = []
    for path in sorted((root / "src").rglob("*.rs")):
        relative = path.relative_to(root).as_posix()
        parts = path.relative_to(root).parts
        if "test" in path.name or any(part == "tests" or part.endswith("_tests") for part in parts[:-1]):
            continue
        if relative.startswith("src/bin/") and path.name.endswith("_bench.rs"):
            continue
        lines = len(path.read_bytes().splitlines())
        limit = baseline.get("legacy_line_limits", {}).get(relative, 500)
        if lines > limit:
            errors.append(f"size: {relative}: {lines} > {limit} lines")
    return errors


def main() -> int:
    baseline = json.loads((Path(__file__).with_name("baseline.json")).read_text())
    process = subprocess.run(
        ["cargo", "fmt", "--all", "--check"], cwd=ROOT,
        capture_output=True, text=True,
    )
    errors = []
    if process.returncode:
        errors.extend(formatting_errors(process.stdout + process.stderr, ROOT, baseline))
    errors.extend(size_errors(ROOT, baseline))
    if errors:
        print("\n".join(errors))
        return 1
    print("Rust hygiene passed: edited/new files formatted; production size limits respected.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
