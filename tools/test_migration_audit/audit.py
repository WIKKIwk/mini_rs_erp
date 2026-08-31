#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "tree-sitter==0.25.2",
#   "tree-sitter-rust==0.24.2",
# ]
# ///
"""Classify Rust tests for migration without compiling the Rust crate."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from collections import Counter
from collections.abc import Iterable, Iterator, Sequence
from dataclasses import asdict, dataclass
from pathlib import Path

try:
    import tree_sitter_rust
    from tree_sitter import Language, Node, Parser
except ImportError as error:  # pragma: no cover - exercised by CLI users
    raise SystemExit(
        "missing migration-audit dependencies; install with: "
        "python3 -m pip install -r tools/test_migration_audit/requirements.txt"
    ) from error


ROOT = Path(__file__).resolve().parents[2]
RUST_LANGUAGE = Language(tree_sitter_rust.language())
RUST_PARSER = Parser(RUST_LANGUAGE)
TEST_ATTRIBUTE = re.compile(
    r"#\s*\[\s*(?:(?:tokio|async_std|sqlx)::)?test\b|#\s*\[\s*rstest\b"
)
HTTP_METHODS = ("GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "HEAD")
STATUS_PATTERN = re.compile(r"StatusCode::([A-Z][A-Z0-9_]*)")
URI_PATTERN = re.compile(
    r"""(?x)
    (?:br|rb|r|b)?\#{0,16}"(?P<raw>/[^"\r\n]*?)"\#{0,16}
    |
    "(?P<normal>/(?:\\.|[^"\\])*)"
    """
)
DATABASE_PATTERN = re.compile(
    r"\b(?:sqlx|PgPool|PgConnection|Postgres|DATABASE_URL|transaction)\b"
    r"|\.begin\s*\(|\.commit\s*\(|\.rollback\s*\("
)
CUSTOM_FIXTURE_PATTERN = re.compile(
    r"\b(?:Fake|Mock)[A-Z][A-Za-z0-9_]*\b"
    r"|\bArc::new\s*\(\s*(?:Fake|Mock)"
    r"|\bwith_[a-zA-Z0-9_]+\s*\("
)
FIXTURE_SETUP_PATTERN = re.compile(
    r"\btest_state\s*\(\s*(?!\))"
    r"|\b(?:setup(?:_[a-zA-Z0-9_]+)?|state_with_[a-zA-Z0-9_]+)\s*\("
    r"|\blet\s+mut\s+state\b"
)
STATE_MUTATION_PATTERN = re.compile(
    r"\bstate\s*\.\s*(?!sessions\b)[a-zA-Z_][a-zA-Z0-9_]*\s*\."
    r"|\b(?:seed|insert|upsert|save|create_[a-zA-Z0-9_]+|put)\s*\("
)


@dataclass(frozen=True)
class SourceFile:
    path: str
    content: bytes


@dataclass(frozen=True)
class TestSignals:
    assertion_count: int
    build_router_calls: int
    custom_fixture: bool
    database_access: bool
    dynamic_request: bool
    fixture_setup: bool
    http_methods: list[str]
    literal_uris: list[str]
    oneshot_calls: int
    state_mutation: bool
    status_codes: list[str]


@dataclass(frozen=True)
class TestFinding:
    classification: str
    confidence: str
    line: int
    name: str
    path: str
    reasons: list[str]
    signals: TestSignals

    @property
    def identifier(self) -> str:
        return f"{self.path}::{self.name}"


@dataclass(frozen=True)
class FileFinding:
    has_parse_error: bool
    path: str
    test_count: int


class AuditFailure(RuntimeError):
    pass


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    argument_parser = argparse.ArgumentParser(description=__doc__)
    argument_parser.add_argument(
        "--root",
        type=Path,
        default=ROOT,
        help="repository root (defaults to the Mini ERP repository)",
    )
    argument_parser.add_argument(
        "--git-ref",
        help="audit Rust sources from this Git revision without checking it out",
    )
    argument_parser.add_argument(
        "--path",
        action="append",
        default=[],
        help="limit scanning to a repository-relative Rust path; repeatable",
    )
    argument_parser.add_argument(
        "--json",
        action="store_true",
        help="emit the complete machine-readable report",
    )
    argument_parser.add_argument(
        "--output",
        type=Path,
        help="write the report to this path instead of stdout",
    )
    argument_parser.add_argument(
        "--fail-on-parse-error",
        action="store_true",
        help="exit non-zero when any Rust source has a syntax error",
    )
    return argument_parser.parse_args(argv)


def run_git(root: Path, arguments: Sequence[str]) -> str:
    process = subprocess.run(
        ["git", *arguments],
        cwd=root,
        text=True,
        capture_output=True,
        check=False,
    )
    if process.returncode != 0:
        detail = process.stderr.strip() or process.stdout.strip()
        raise AuditFailure(f"git {' '.join(arguments)} failed: {detail}")
    return process.stdout


def normalize_requested_paths(paths: Sequence[str]) -> list[str]:
    normalized: list[str] = []
    for raw_path in paths or ["src"]:
        path = Path(raw_path)
        if path.is_absolute() or ".." in path.parts:
            raise AuditFailure(f"path must be repository-relative: {raw_path}")
        normalized.append(path.as_posix().rstrip("/"))
    return normalized


def worktree_sources(root: Path, requested_paths: Sequence[str]) -> list[SourceFile]:
    sources: list[SourceFile] = []
    seen: set[str] = set()
    for requested in normalize_requested_paths(requested_paths):
        target = root / requested
        candidates = [target] if target.is_file() else sorted(target.rglob("*.rs"))
        for path in candidates:
            if path.suffix != ".rs" or not path.is_file():
                continue
            relative = path.relative_to(root).as_posix()
            if relative in seen:
                continue
            seen.add(relative)
            sources.append(SourceFile(relative, path.read_bytes()))
    return sorted(sources, key=lambda item: item.path)


def git_sources(
    root: Path, git_ref: str, requested_paths: Sequence[str]
) -> list[SourceFile]:
    requested = normalize_requested_paths(requested_paths)
    listing = run_git(root, ["ls-tree", "-r", "--name-only", git_ref, "--", *requested])
    paths = sorted(path for path in listing.splitlines() if path.endswith(".rs"))
    sources: list[SourceFile] = []
    for path in paths:
        process = subprocess.run(
            ["git", "show", f"{git_ref}:{path}"],
            cwd=root,
            capture_output=True,
            check=False,
        )
        if process.returncode != 0:
            detail = process.stderr.decode(errors="replace").strip()
            raise AuditFailure(f"git show {git_ref}:{path} failed: {detail}")
        sources.append(SourceFile(path, process.stdout))
    return sources


def walk(node: Node) -> Iterator[Node]:
    stack = [node]
    while stack:
        current = stack.pop()
        yield current
        stack.extend(reversed(current.named_children))


def source_text(content: bytes, node: Node) -> str:
    return content[node.start_byte : node.end_byte].decode("utf-8", errors="replace")


def function_attributes(content: bytes, function: Node) -> str:
    attributes: list[str] = []
    sibling = function.prev_named_sibling
    while sibling is not None and sibling.type == "attribute_item":
        attributes.append(source_text(content, sibling))
        sibling = sibling.prev_named_sibling
    attributes.reverse()
    return "\n".join(attributes)


def test_functions(content: bytes, tree_root: Node) -> Iterator[Node]:
    for node in walk(tree_root):
        if node.type != "function_item":
            continue
        if TEST_ATTRIBUTE.search(function_attributes(content, node)):
            yield node


def string_literal_values(function_text: str) -> list[str]:
    values: list[str] = []
    for match in URI_PATTERN.finditer(function_text):
        value = match.group("raw") or match.group("normal")
        if value is None:
            continue
        value = value.replace(r"\n", "\n").replace(r"\"", '"').replace(r"\\", "\\")
        if value.startswith("/"):
            values.append(value)
    return sorted(set(values))


def assertion_signals(content: bytes, function: Node) -> tuple[int, list[str]]:
    count = 0
    statuses: set[str] = set()
    for node in walk(function):
        if node.type != "macro_invocation":
            continue
        macro = node.child_by_field_name("macro")
        if macro is None:
            continue
        if source_text(content, macro) in {
            "assert",
            "assert_eq",
            "assert_ne",
            "matches",
        }:
            count += 1
            statuses.update(STATUS_PATTERN.findall(source_text(content, node)))
    return count, sorted(statuses)


def call_field_name(content: bytes, call: Node) -> str | None:
    function = call.child_by_field_name("function")
    if function is None or function.type != "field_expression":
        return None
    field = function.child_by_field_name("field")
    return source_text(content, field) if field is not None else None


def request_signals(
    content: bytes, function: Node
) -> tuple[int, list[str], list[str], bool]:
    methods: set[str] = set()
    uris: set[str] = set()
    dynamic = False
    count = 0
    for node in walk(function):
        if (
            node.type != "call_expression"
            or call_field_name(content, node) != "oneshot"
        ):
            continue
        count += 1
        arguments = node.child_by_field_name("arguments")
        request_argument = (
            arguments.named_children[0]
            if arguments is not None and arguments.named_children
            else None
        )
        if request_argument is None:
            dynamic = True
            continue
        request_text = source_text(content, request_argument)
        methods.update(
            method
            for method in HTTP_METHODS
            if re.search(rf'"{method}"|Method::{method}\b', request_text)
        )
        request_uris = string_literal_values(request_text)
        uris.update(request_uris)
        if (
            request_argument.type != "call_expression"
            or "format!" in request_text
            or any("{" in uri or "}" in uri for uri in request_uris)
        ):
            dynamic = True
    if len(methods) != count or len(uris) != count:
        dynamic = True
    return count, sorted(methods), sorted(uris), dynamic


def collect_signals(content: bytes, function: Node) -> TestSignals:
    text = source_text(content, function)
    assertion_total, status_codes = assertion_signals(content, function)
    oneshot_calls, methods, literal_uris, dynamic_request = request_signals(
        content, function
    )
    return TestSignals(
        assertion_count=assertion_total,
        build_router_calls=text.count("build_router("),
        custom_fixture=bool(CUSTOM_FIXTURE_PATTERN.search(text)),
        database_access=bool(DATABASE_PATTERN.search(text)),
        dynamic_request=dynamic_request,
        fixture_setup=bool(FIXTURE_SETUP_PATTERN.search(text)),
        http_methods=methods,
        literal_uris=literal_uris,
        oneshot_calls=oneshot_calls,
        state_mutation=bool(STATE_MUTATION_PATTERN.search(text)),
        status_codes=status_codes,
    )


def classify(signals: TestSignals) -> tuple[str, str, list[str]]:
    is_http = signals.oneshot_calls > 0 or signals.build_router_calls > 0
    if not is_http:
        reason = (
            "direct database/transaction test"
            if signals.database_access
            else "no router HTTP execution"
        )
        return "rust_native", "high", [reason]

    if signals.database_access:
        return (
            "rust_native",
            "high",
            ["HTTP test directly owns a database/transaction boundary"],
        )

    blockers: list[str] = []
    if signals.oneshot_calls != 1:
        blockers.append(f"contains {signals.oneshot_calls} HTTP executions")
    if len(signals.literal_uris) != 1:
        blockers.append(f"contains {len(signals.literal_uris)} static URI candidates")
    if len(signals.http_methods) != 1:
        blockers.append(
            f"contains {len(signals.http_methods)} static method candidates"
        )
    if len(signals.status_codes) != 1 or signals.assertion_count == 0:
        blockers.append("does not have one unambiguous asserted HTTP status")
    if signals.custom_fixture:
        blockers.append("uses a custom fake/mock fixture")
    if signals.state_mutation:
        blockers.append("mutates or seeds domain state")
    if signals.dynamic_request:
        blockers.append("builds request method or URI dynamically")
    if signals.fixture_setup:
        blockers.append("depends on a test-specific state fixture")

    if not blockers:
        reasons = [
            "single router request",
            "static method and URI",
            "explicit status oracle",
            "no database, fake, or domain-state setup",
        ]
        return "automatic_contract", "high", reasons

    return "scenario_contract", "medium", blockers


def audit_source(source: SourceFile) -> tuple[FileFinding, list[TestFinding]]:
    tree = RUST_PARSER.parse(source.content)
    findings: list[TestFinding] = []
    for function in test_functions(source.content, tree.root_node):
        name_node = function.child_by_field_name("name")
        if name_node is None:
            continue
        signals = collect_signals(source.content, function)
        classification, confidence, reasons = classify(signals)
        findings.append(
            TestFinding(
                classification=classification,
                confidence=confidence,
                line=function.start_point.row + 1,
                name=source_text(source.content, name_node),
                path=source.path,
                reasons=reasons,
                signals=signals,
            )
        )
    return (
        FileFinding(
            has_parse_error=tree.root_node.has_error,
            path=source.path,
            test_count=len(findings),
        ),
        findings,
    )


def build_report(
    root: Path,
    sources: Iterable[SourceFile],
    git_ref: str | None,
) -> dict[str, object]:
    files: list[FileFinding] = []
    tests: list[TestFinding] = []
    for source in sources:
        file_finding, test_findings = audit_source(source)
        files.append(file_finding)
        tests.extend(test_findings)

    counts = Counter(test.classification for test in tests)
    test_files = sum(1 for file in files if file.test_count)
    parse_errors = [file.path for file in files if file.has_parse_error]
    return {
        "protocol": 1,
        "source": {"kind": "git" if git_ref else "worktree", "ref": git_ref},
        "summary": {
            "automatic_contract": counts["automatic_contract"],
            "parse_errors": len(parse_errors),
            "rust_files_scanned": len(files),
            "rust_native": counts["rust_native"],
            "scenario_contract": counts["scenario_contract"],
            "test_files": test_files,
            "test_functions": len(tests),
        },
        "parse_error_paths": parse_errors,
        "files": [
            asdict(file) for file in files if file.test_count or file.has_parse_error
        ],
        "tests": [
            {"id": test.identifier, **asdict(test)}
            for test in sorted(
                tests, key=lambda item: (item.path, item.line, item.name)
            )
        ],
        "repository": str(root.resolve()),
    }


def text_report(report: dict[str, object]) -> str:
    summary = report["summary"]
    assert isinstance(summary, dict)
    source = report["source"]
    assert isinstance(source, dict)
    source_name = source["ref"] if source["kind"] == "git" else "worktree"
    lines = [
        f"source: {source_name}",
        f"rust files scanned: {summary['rust_files_scanned']}",
        f"test files: {summary['test_files']}",
        f"test functions: {summary['test_functions']}",
        f"automatic contracts: {summary['automatic_contract']}",
        f"scenario contracts: {summary['scenario_contract']}",
        f"Rust-native tests: {summary['rust_native']}",
        f"parse errors: {summary['parse_errors']}",
    ]
    return "\n".join(lines) + "\n"


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    root = args.root.resolve()
    if not (root / "Cargo.toml").is_file() or not (root / ".git").exists():
        raise AuditFailure(f"not a Rust Git repository root: {root}")

    sources = (
        git_sources(root, args.git_ref, args.path)
        if args.git_ref
        else worktree_sources(root, args.path)
    )
    report = build_report(root, sources, args.git_ref)
    rendered = (
        json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
        if args.json or args.output
        else text_report(report)
    )

    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered, encoding="utf-8")
    else:
        sys.stdout.write(rendered)

    summary = report["summary"]
    assert isinstance(summary, dict)
    if args.fail_on_parse_error and summary["parse_errors"]:
        return 2
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except AuditFailure as error:
        print(f"migration audit failed: {error}", file=sys.stderr)
        raise SystemExit(2) from error
