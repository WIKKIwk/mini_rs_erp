#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "tree-sitter==0.25.2",
#   "tree-sitter-rust==0.24.2",
# ]
# ///
"""Extract fail-closed verifier contracts from statically auditable Rust tests."""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import audit
import tomllib
from tree_sitter import Node

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_MANIFEST = Path(__file__).with_name("migrations") / "generic_http_975078a.json"
DEFAULT_OUTPUT = ROOT / "tools" / "mini_erp_verifier" / "generated_contracts.json"
STATUS_VALUES = {
    "OK": 200,
    "CREATED": 201,
    "NO_CONTENT": 204,
    "PARTIAL_CONTENT": 206,
    "BAD_REQUEST": 400,
    "UNAUTHORIZED": 401,
    "FORBIDDEN": 403,
    "NOT_FOUND": 404,
    "METHOD_NOT_ALLOWED": 405,
    "CONFLICT": 409,
    "UNPROCESSABLE_ENTITY": 422,
    "INTERNAL_SERVER_ERROR": 500,
    "SERVICE_UNAVAILABLE": 503,
}
ROLE_PATTERNS = {
    "supplier": (r"\bPrincipalRole::Supplier\b", r"\bsupplier_session\s*\("),
    "werka": (r"\bPrincipalRole::Werka\b", r"\bwerka_session\s*\("),
    "customer": (r"\bPrincipalRole::Customer\b", r"\bcustomer_session\s*\("),
    "aparatchi": (r"\bPrincipalRole::Aparatchi\b", r"\baparatchi_session\s*\("),
    "qolipchi": (r"\bPrincipalRole::Qolipchi\b", r"\bqolipchi_session\s*\("),
    "boyoqchi": (r"\bPrincipalRole::Boyoqchi\b", r"\bboyoqchi_session\s*\("),
    "material_taminotchi": (
        r"\bPrincipalRole::MaterialTaminotchi\b",
        r"\bmaterial_taminotchi_session\s*\(",
    ),
    "admin": (r"\bPrincipalRole::Admin\b", r"\badmin_session\s*\("),
}
BOUND_JSON_PATTERN = re.compile(
    r"\blet\s+(?P<name>[a-zA-Z_][a-zA-Z0-9_]*)\s*=\s*json_body\([^;]+?\)\.await\s*;",
    re.DOTALL,
)
JSON_PATH_PATTERN = re.compile(r'\[\s*"([^"\\]+)"\s*\]')


class ExtractionFailure(RuntimeError):
    pass


@dataclass(frozen=True)
class ExtractedCase:
    case: dict[str, Any]
    assertions: int


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--check", action="store_true", help="fail if output is stale")
    parser.add_argument(
        "--stdout", action="store_true", help="write generated JSON to stdout"
    )
    return parser.parse_args(argv)


def load_manifest(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as handle:
        manifest = json.load(handle)
    if (
        manifest.get("protocol") != 1
        or not isinstance(manifest.get("git_ref"), str)
        or not isinstance(manifest.get("paths"), list)
    ):
        raise ExtractionFailure(f"malformed migration manifest: {path}")
    return manifest


def rust_literal(source: str) -> str | None:
    source = source.strip()
    raw = re.fullmatch(
        r'(?:br|rb|r)(?P<hash>#+)?"(?P<body>.*)"(?P=hash)', source, re.DOTALL
    )
    if raw:
        return raw.group("body")
    if source.startswith('b"'):
        source = source[1:]
    if not (source.startswith('"') and source.endswith('"')):
        return None
    try:
        value = json.loads(source)
    except json.JSONDecodeError:
        return None
    return value if isinstance(value, str) else None


def string_literals(content: bytes, node: Node) -> list[str]:
    values: list[str] = []
    for child in audit.walk(node):
        if child.type not in {"string_literal", "raw_string_literal"}:
            continue
        value = rust_literal(audit.source_text(content, child))
        if value is not None:
            values.append(value)
    return values


def oneshot_request_argument(content: bytes, function: Node) -> Node:
    requests: list[Node] = []
    for node in audit.walk(function):
        if (
            node.type != "call_expression"
            or audit.call_field_name(content, node) != "oneshot"
        ):
            continue
        arguments = node.child_by_field_name("arguments")
        if arguments is None or len(arguments.named_children) != 1:
            raise ExtractionFailure("oneshot does not have one request argument")
        requests.append(arguments.named_children[0])
    if len(requests) != 1:
        raise ExtractionFailure(f"expected one HTTP execution, found {len(requests)}")
    return requests[0]


def request_body(content: bytes, request: Node) -> dict[str, Any]:
    text = audit.source_text(content, request)
    json_values: list[Any] = []
    for value in string_literals(content, request):
        if not value.lstrip().startswith(("{", "[")):
            continue
        try:
            json_values.append(json.loads(value))
        except json.JSONDecodeError as error:
            raise ExtractionFailure(
                f"request contains invalid literal JSON: {error}"
            ) from error
    if len(json_values) > 1:
        raise ExtractionFailure("request contains multiple JSON body candidates")
    if "Body::from" in text and not json_values:
        raise ExtractionFailure("request body is not a JSON literal")
    return {"body": json_values[0]} if json_values else {}


def request_role(function_text: str, request_text: str) -> str | None:
    roles = {
        role
        for role, patterns in ROLE_PATTERNS.items()
        if any(re.search(pattern, function_text) for pattern in patterns)
    }
    has_token = bool(
        re.search(r"AUTHORIZATION|Bearer\s|&\s*token\b|\btoken\s*\)", request_text)
    )
    if len(roles) > 1:
        raise ExtractionFailure(f"multiple request roles detected: {sorted(roles)}")
    if has_token and not roles:
        raise ExtractionFailure("authenticated request role could not be resolved")
    if roles and not has_token:
        raise ExtractionFailure(
            "session role exists but request authentication was not detected"
        )
    return next(iter(roles), None)


def split_macro_arguments(source: str) -> list[str]:
    start = source.find("(")
    end = source.rfind(")")
    if start < 0 or end <= start:
        return []
    body = source[start + 1 : end]
    arguments: list[str] = []
    current: list[str] = []
    depths = {"(": 0, "[": 0, "{": 0}
    closing = {")": "(", "]": "[", "}": "{"}
    in_string = False
    escaped = False
    for character in body:
        if in_string:
            current.append(character)
            if escaped:
                escaped = False
            elif character == "\\":
                escaped = True
            elif character == '"':
                in_string = False
            continue
        if character == '"':
            in_string = True
            current.append(character)
        elif character in depths:
            depths[character] += 1
            current.append(character)
        elif character in closing:
            depths[closing[character]] -= 1
            current.append(character)
        elif character == "," and not any(depths.values()):
            arguments.append("".join(current).strip())
            current = []
        else:
            current.append(character)
    if current:
        arguments.append("".join(current).strip())
    return arguments


def json_scalar(source: str, package_version: str | None = None) -> Any:
    source = source.strip()
    if source == 'env!("CARGO_PKG_VERSION")' and package_version is not None:
        return package_version
    literal = rust_literal(source)
    if literal is not None:
        return literal
    normalized = source.replace("f32", "").replace("f64", "")
    try:
        value = json.loads(normalized)
    except json.JSONDecodeError as error:
        raise ExtractionFailure(f"unsupported assertion value: {source}") from error
    if not isinstance(value, (str, int, float, bool)) and value is not None:
        raise ExtractionFailure(f"assertion value is not a JSON scalar: {source}")
    return value


def set_body_path(body: dict[str, Any], path: list[str], value: Any) -> None:
    if not path:
        raise ExtractionFailure("empty response body path")
    current = body
    for key in path[:-1]:
        existing = current.setdefault(key, {})
        if not isinstance(existing, dict):
            raise ExtractionFailure(f"conflicting response body path: {'.'.join(path)}")
        current = existing
    current[path[-1]] = value


def response_expectation(
    content: bytes,
    function: Node,
    status: int,
    package_version: str | None = None,
) -> tuple[dict[str, Any], int]:
    function_text = audit.source_text(content, function)
    bound_json = set(BOUND_JSON_PATTERN.findall(function_text))
    body: dict[str, Any] = {}
    handled = 0
    for node in audit.walk(function):
        if node.type != "macro_invocation":
            continue
        macro = node.child_by_field_name("macro")
        if macro is None:
            continue
        macro_name = audit.source_text(content, macro)
        if macro_name not in {"assert", "assert_eq", "assert_ne", "matches"}:
            continue
        source = audit.source_text(content, node)
        if "status()" in source and "StatusCode::" in source:
            handled += 1
            continue
        if macro_name != "assert_eq":
            raise ExtractionFailure(f"unsupported response assertion: {source}")
        arguments = split_macro_arguments(source)
        if len(arguments) < 2:
            raise ExtractionFailure(f"could not split assertion: {source}")
        left, right = arguments[:2]
        is_direct_json = "json_body(" in left and ".await" in left
        bound_name = left.split("[", 1)[0].strip()
        if not is_direct_json and bound_name not in bound_json:
            raise ExtractionFailure(f"assertion is not a response JSON path: {source}")
        path = JSON_PATH_PATTERN.findall(left)
        if not path:
            raise ExtractionFailure(f"response assertion has no object path: {source}")
        set_body_path(body, path, json_scalar(right, package_version))
        handled += 1
    expectation: dict[str, Any] = {"status": status}
    if body:
        expectation["body"] = body
    return expectation, handled


def extract_case(
    source: audit.SourceFile,
    function: Node,
    package_version: str | None = None,
) -> ExtractedCase:
    name_node = function.child_by_field_name("name")
    if name_node is None:
        raise ExtractionFailure("test function has no name")
    name = audit.source_text(source.content, name_node)
    signals = audit.collect_signals(source.content, function)
    classification, _, reasons = audit.classify(signals)
    if classification != "automatic_contract":
        raise ExtractionFailure("; ".join(reasons))
    if signals.status_codes[0] not in STATUS_VALUES:
        raise ExtractionFailure(f"unsupported status: {signals.status_codes[0]}")

    request_node = oneshot_request_argument(source.content, function)
    function_text = audit.source_text(source.content, function)
    request_text = audit.source_text(source.content, request_node)
    role = request_role(function_text, request_text)
    request: dict[str, Any] = {
        "method": signals.http_methods[0],
        "uri": signals.literal_uris[0],
        "fixture": "isolated",
        **request_body(source.content, request_node),
    }
    if role is not None:
        request["role"] = role
    expect, handled_assertions = response_expectation(
        source.content,
        function,
        STATUS_VALUES[signals.status_codes[0]],
        package_version,
    )
    if handled_assertions != signals.assertion_count:
        raise ExtractionFailure(
            f"handled {handled_assertions} of {signals.assertion_count} assertions"
        )
    return ExtractedCase(
        case={
            "name": name,
            "request": request,
            "expect": expect,
            "source": {"path": source.path, "line": function.start_point.row + 1},
        },
        assertions=handled_assertions,
    )


def generate(root: Path, manifest: dict[str, Any]) -> dict[str, Any]:
    with (root / "Cargo.toml").open("rb") as handle:
        package_version = tomllib.load(handle).get("package", {}).get("version")
    if not isinstance(package_version, str):
        raise ExtractionFailure("Cargo.toml package.version is required")
    sources = audit.git_sources(root, manifest["git_ref"], manifest["paths"])
    cases: list[dict[str, Any]] = []
    skipped: list[dict[str, Any]] = []
    automatic = 0
    tests = 0
    for source in sources:
        tree = audit.RUST_PARSER.parse(source.content)
        if tree.root_node.has_error:
            raise ExtractionFailure(f"Rust parse error: {source.path}")
        for function in audit.test_functions(source.content, tree.root_node):
            tests += 1
            name_node = function.child_by_field_name("name")
            name = (
                audit.source_text(source.content, name_node) if name_node else "unknown"
            )
            signals = audit.collect_signals(source.content, function)
            classification, _, reasons = audit.classify(signals)
            if classification != "automatic_contract":
                skipped.append(
                    {
                        "id": f"{source.path}::{name}",
                        "classification": classification,
                        "reasons": reasons,
                    }
                )
                continue
            automatic += 1
            try:
                cases.append(extract_case(source, function, package_version).case)
            except ExtractionFailure as error:
                raise ExtractionFailure(f"{source.path}::{name}: {error}") from error

    if tests != manifest.get("expected_tests"):
        raise ExtractionFailure(
            f"migration source test count changed: expected {manifest.get('expected_tests')}, got {tests}"
        )
    if automatic != manifest.get("expected_automatic_contracts"):
        raise ExtractionFailure(
            "automatic contract count changed: "
            f"expected {manifest.get('expected_automatic_contracts')}, got {automatic}"
        )
    if len(cases) != automatic:
        raise ExtractionFailure(
            f"only extracted {len(cases)} of {automatic} automatic contracts"
        )

    return {
        "protocol": 1,
        "generated": True,
        "source": {
            "git_ref": manifest["git_ref"],
            "manifest": DEFAULT_MANIFEST.relative_to(ROOT).as_posix(),
        },
        "summary": {
            "source_tests": tests,
            "generated_cases": len(cases),
            "skipped_scenarios": len(skipped),
        },
        "cases": sorted(cases, key=lambda case: case["name"]),
        "skipped": sorted(skipped, key=lambda item: item["id"]),
    }


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    root = args.root.resolve()
    manifest = load_manifest(args.manifest)
    report = generate(root, manifest)
    rendered = json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    if args.stdout:
        sys.stdout.write(rendered)
        return 0
    if args.check:
        current = (
            args.output.read_text(encoding="utf-8") if args.output.is_file() else ""
        )
        if current != rendered:
            raise ExtractionFailure(f"generated contracts are stale: {args.output}")
        return 0
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(rendered, encoding="utf-8")
    print(
        f"generated {report['summary']['generated_cases']} contracts from "
        f"{report['summary']['source_tests']} historical Rust tests; "
        f"skipped {report['summary']['skipped_scenarios']} scenarios"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, audit.AuditFailure, ExtractionFailure) as error:
        print(f"contract extraction failed: {error}", file=sys.stderr)
        raise SystemExit(2) from error
