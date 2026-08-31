#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "tree-sitter==0.25.2",
#   "tree-sitter-rust==0.24.2",
# ]
# ///
"""Emit an apply_patch-compatible deletion for selected migrated Rust tests."""

from __future__ import annotations

import argparse
import difflib
import sys
from collections.abc import Sequence
from pathlib import Path

import audit
from extract_contracts import DEFAULT_MANIFEST, ExtractionFailure, load_manifest

ROOT = Path(__file__).resolve().parents[2]


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    return parser.parse_args(argv)


def removal_range(content: bytes, function: audit.Node) -> tuple[int, int]:
    start = function.start_byte
    sibling = function.prev_named_sibling
    while sibling is not None and sibling.type == "attribute_item":
        start = sibling.start_byte
        sibling = sibling.prev_named_sibling

    end = function.end_byte
    while end < len(content) and content[end : end + 1] in {b" ", b"\t"}:
        end += 1
    if content[end : end + 2] == b"\r\n":
        end += 2
    elif content[end : end + 1] == b"\n":
        end += 1
    if content[end : end + 2] == b"\r\n":
        end += 2
    elif content[end : end + 1] == b"\n":
        end += 1
    return start, end


def remove_selected_tests(
    source: audit.SourceFile, selected: set[str]
) -> tuple[str, set[str]]:
    tree = audit.RUST_PARSER.parse(source.content)
    if tree.root_node.has_error:
        raise ExtractionFailure(f"Rust parse error: {source.path}")
    ranges: list[tuple[int, int]] = []
    found: set[str] = set()
    for function in audit.test_functions(source.content, tree.root_node):
        name_node = function.child_by_field_name("name")
        if name_node is None:
            continue
        name = audit.source_text(source.content, name_node)
        identifier = f"{source.path}::{name}"
        if identifier not in selected:
            continue
        found.add(identifier)
        ranges.append(removal_range(source.content, function))

    updated = source.content
    for start, end in sorted(ranges, reverse=True):
        updated = updated[:start] + updated[end:]
    return updated.decode("utf-8"), found


def render_patch(root: Path, manifest: dict[str, object]) -> str:
    if manifest.get("remove_selected_tests") is not True:
        raise ExtractionFailure("manifest does not authorize selected-test removal")
    selected = set(manifest.get("tests", []))
    if not selected or not all(isinstance(item, str) for item in selected):
        raise ExtractionFailure("manifest has no selected tests")
    paths = sorted({identifier.rsplit("::", 1)[0] for identifier in selected})
    sources = audit.worktree_sources(root, paths)
    found: set[str] = set()
    sections: list[str] = ["*** Begin Patch"]
    for source in sources:
        original = source.content.decode("utf-8")
        updated, source_found = remove_selected_tests(source, selected)
        found.update(source_found)
        if original == updated:
            continue
        diff = list(
            difflib.unified_diff(
                original.splitlines(keepends=True),
                updated.splitlines(keepends=True),
                fromfile=source.path,
                tofile=source.path,
                n=3,
            )
        )
        sections.append(f"*** Update File: {root / source.path}")
        sections.extend(
            "@@" if line.startswith("@@") else line.rstrip("\n") for line in diff[2:]
        )
    missing = selected - found
    if missing:
        raise ExtractionFailure(
            "selected tests are missing from worktree: " + ", ".join(sorted(missing))
        )
    sections.append("*** End Patch")
    return "\n".join(sections) + "\n"


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    root = args.root.resolve()
    manifest = load_manifest(args.manifest.resolve())
    sys.stdout.write(render_patch(root, manifest))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, audit.AuditFailure, ExtractionFailure) as error:
        print(f"removal patch failed: {error}", file=sys.stderr)
        raise SystemExit(2) from error
