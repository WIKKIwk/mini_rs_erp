#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "tree-sitter==0.25.2",
#   "tree-sitter-rust==0.24.2",
# ]
# ///
"""Generate or check every registered Rust-to-verifier migration bundle."""

from __future__ import annotations

import argparse
import json
import sys
from collections.abc import Sequence
from pathlib import Path
from typing import Any

import audit
from extract_contracts import (
    ExtractionFailure,
    generate,
    load_manifest,
    verify_removals,
)

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_INDEX = Path(__file__).with_name("migrations") / "index.json"


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--index", type=Path, default=DEFAULT_INDEX)
    parser.add_argument("--check", action="store_true")
    return parser.parse_args(argv)


def load_index(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as handle:
        index = json.load(handle)
    bundles = index.get("bundles") if isinstance(index, dict) else None
    if (
        index.get("protocol") != 1
        or not isinstance(bundles, list)
        or not bundles
        or not all(
            isinstance(bundle, dict)
            and isinstance(bundle.get("manifest"), str)
            and isinstance(bundle.get("output"), str)
            for bundle in bundles
        )
    ):
        raise ExtractionFailure(f"malformed migration index: {path}")
    return index


def repository_path(root: Path, value: str) -> Path:
    relative = Path(value)
    if relative.is_absolute() or ".." in relative.parts:
        raise ExtractionFailure(
            f"migration index path must be repository-relative: {value}"
        )
    return root / relative


def process_bundle(
    root: Path, manifest_path: Path, output_path: Path, check: bool
) -> dict[str, Any]:
    manifest = load_manifest(manifest_path)
    report = generate(root, manifest, manifest_path)
    rendered = json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    if check:
        verify_removals(root, manifest)
        current = (
            output_path.read_text(encoding="utf-8") if output_path.is_file() else ""
        )
        if current != rendered:
            raise ExtractionFailure(f"generated contracts are stale: {output_path}")
    else:
        output_path.parent.mkdir(parents=True, exist_ok=True)
        output_path.write_text(rendered, encoding="utf-8")
    return report


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    root = args.root.resolve()
    index = load_index(args.index.resolve())
    total = 0
    for bundle in index["bundles"]:
        manifest_path = repository_path(root, bundle["manifest"])
        output_path = repository_path(root, bundle["output"])
        report = process_bundle(root, manifest_path, output_path, args.check)
        total += report["summary"]["generated_cases"]
    action = "checked" if args.check else "generated"
    print(f"{action} {total} contracts across {len(index['bundles'])} migrations")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, audit.AuditFailure, ExtractionFailure) as error:
        print(f"contract registry failed: {error}", file=sys.stderr)
        raise SystemExit(2) from error
