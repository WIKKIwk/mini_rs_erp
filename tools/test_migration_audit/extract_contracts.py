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
JSON_PATH_PATTERN = re.compile(r'\[\s*(?:"([^"\\]+)"|(\d+))\s*\]')
BOUND_STATUS_PATTERN = re.compile(
    r"\blet\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*=\s*[a-zA-Z_][a-zA-Z0-9_]*\.status\(\)\s*;"
)


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
        or not all(isinstance(item, str) for item in manifest["paths"])
        or not isinstance(manifest.get("expected_tests"), int)
        or not isinstance(manifest.get("expected_automatic_contracts"), int)
        or (
            "expected_generated_cases" in manifest
            and not isinstance(manifest["expected_generated_cases"], int)
        )
        or (
            "allow_multiple_requests" in manifest
            and not isinstance(manifest["allow_multiple_requests"], bool)
        )
        or (
            "tests" in manifest
            and (
                not isinstance(manifest["tests"], list)
                or not all(isinstance(item, str) for item in manifest["tests"])
            )
        )
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
    requests = oneshot_calls(content, function)
    if len(requests) != 1:
        raise ExtractionFailure(f"expected one HTTP execution, found {len(requests)}")
    return request_argument(content, requests[0])


def oneshot_calls(content: bytes, function: Node) -> list[Node]:
    return sorted(
        (
            node
            for node in audit.walk(function)
            if node.type == "call_expression"
            and audit.call_field_name(content, node) == "oneshot"
        ),
        key=lambda node: node.start_byte,
    )


def request_argument(content: bytes, oneshot: Node) -> Node:
    arguments = oneshot.child_by_field_name("arguments")
    if arguments is None or len(arguments.named_children) != 1:
        raise ExtractionFailure("oneshot does not have one request argument")
    return arguments.named_children[0]


def response_binding(content: bytes, oneshot: Node, function: Node) -> str:
    current: Node | None = oneshot
    while current is not None and current != function:
        if current.type == "let_declaration":
            pattern = current.child_by_field_name("pattern")
            if pattern is not None and pattern.type == "identifier":
                return audit.source_text(content, pattern)
            break
        current = current.parent
    raise ExtractionFailure("HTTP response is not bound to a simple local name")


def request_method_uri(content: bytes, request: Node) -> tuple[str, str]:
    text = audit.source_text(content, request)
    methods = {
        method
        for method in audit.HTTP_METHODS
        if re.search(rf'"{method}"|Method::{method}\b', text)
    }
    if not methods and "Request::builder()" in text and ".method(" not in text:
        methods.add("GET")
    uris = audit.string_literal_values(text)
    if len(methods) != 1:
        raise ExtractionFailure(f"request has {len(methods)} static method candidates")
    if len(uris) != 1 or "format!" in text or "{" in uris[0] or "}" in uris[0]:
        raise ExtractionFailure(f"request has {len(uris)} static URI candidates")
    return next(iter(methods)), uris[0]


def assertion_nodes(
    content: bytes, function: Node, start_byte: int, end_byte: int
) -> list[Node]:
    assertions: list[Node] = []
    for node in audit.walk(function):
        if node.type != "macro_invocation":
            continue
        macro = node.child_by_field_name("macro")
        if (
            macro is not None
            and audit.source_text(content, macro)
            in {"assert", "assert_eq", "assert_ne", "matches"}
            and node.start_byte >= start_byte
            and node.end_byte <= end_byte
        ):
            assertions.append(node)
    return sorted(assertions, key=lambda node: node.start_byte)


def request_body(content: bytes, request: Node) -> dict[str, Any]:
    text = audit.source_text(content, request)
    json_values: list[Any] = []
    for value in string_literals(content, request):
        if not value.lstrip().startswith(("{", "[")):
            continue
        try:
            json_values.append(json.loads(value))
        except json.JSONDecodeError:
            return {"raw_body": value}
    if len(json_values) > 1:
        raise ExtractionFailure("request contains multiple JSON body candidates")
    if "Body::from" in text and not json_values:
        raise ExtractionFailure("request body is not a JSON literal")
    return {"body": json_values[0]} if json_values else {}


def session_role_bindings(content: bytes, function: Node) -> dict[str, str]:
    bindings: dict[str, str] = {}
    for node in audit.walk(function):
        if node.type != "let_declaration":
            continue
        pattern = node.child_by_field_name("pattern")
        if pattern is None or pattern.type != "identifier":
            continue
        declaration = audit.source_text(content, node)
        roles = {
            role
            for role, patterns in ROLE_PATTERNS.items()
            if any(re.search(role_pattern, declaration) for role_pattern in patterns)
        }
        if len(roles) == 1:
            bindings[audit.source_text(content, pattern)] = next(iter(roles))
    return bindings


def request_role(
    function_text: str,
    request_text: str,
    role_bindings: dict[str, str] | None = None,
) -> str | None:
    bound_roles = {
        role
        for name, role in (role_bindings or {}).items()
        if re.search(rf"\b{re.escape(name)}\b", request_text)
    }
    if len(bound_roles) > 1:
        raise ExtractionFailure(
            f"multiple bound request roles detected: {sorted(bound_roles)}"
        )
    if len(bound_roles) == 1:
        return next(iter(bound_roles))

    has_token = bool(
        re.search(
            r"AUTHORIZATION|Bearer\s|&\s*[a-zA-Z_][a-zA-Z0-9_]*token[a-zA-Z0-9_]*\b"
            r"|\b[a-zA-Z_][a-zA-Z0-9_]*token[a-zA-Z0-9_]*\s*\)",
            request_text,
        )
    )
    if not has_token:
        return None
    roles = {
        role
        for role, patterns in ROLE_PATTERNS.items()
        if any(re.search(pattern, function_text) for pattern in patterns)
    }
    if len(roles) > 1:
        raise ExtractionFailure(f"multiple request roles detected: {sorted(roles)}")
    if has_token and not roles:
        raise ExtractionFailure("authenticated request role could not be resolved")
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


def json_path(source: str) -> list[str | int]:
    return [
        key if key else int(index) for key, index in JSON_PATH_PATTERN.findall(source)
    ]


def bound_body_paths(
    function_text: str, bound_json: set[str]
) -> dict[str, list[str | int]]:
    paths: dict[str, list[str | int]] = {name: [] for name in bound_json}
    pattern = re.compile(
        r"\blet\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*=\s*"
        r"([a-zA-Z_][a-zA-Z0-9_]*)"
        r"((?:\s*\[\s*(?:\"[^\"\\]+\"|\d+)\s*\])*)"
        r"\s*(?:\.as_(?:array|str)\(\)[^;]*)?;",
        re.DOTALL,
    )
    for alias, base, suffix in pattern.findall(function_text):
        if base in paths and alias != base:
            paths[alias] = [*paths[base], *json_path(suffix)]
    return paths


def resolve_body_path(
    expression: str, aliases: dict[str, list[str | int]]
) -> list[str | int] | None:
    base_match = re.match(r"\s*([a-zA-Z_][a-zA-Z0-9_]*)", expression)
    if base_match is None or base_match.group(1) not in aliases:
        return None
    return [*aliases[base_match.group(1)], *json_path(expression)]


def response_expectation(
    content: bytes,
    function: Node,
    status: int,
    package_version: str | None = None,
    start_byte: int | None = None,
    end_byte: int | None = None,
) -> tuple[dict[str, Any], int]:
    scope_start = function.start_byte if start_byte is None else start_byte
    scope_end = function.end_byte if end_byte is None else end_byte
    function_text = content[scope_start:scope_end].decode("utf-8", errors="replace")
    bound_json = set(BOUND_JSON_PATTERN.findall(function_text))
    body_aliases = bound_body_paths(function_text, bound_json)
    bound_status = set(BOUND_STATUS_PATTERN.findall(function_text))
    body: dict[str, Any] = {}
    body_paths: list[dict[str, Any]] = []
    handled = 0
    for node in audit.walk(function):
        if node.type != "macro_invocation":
            continue
        if node.start_byte < scope_start or node.end_byte > scope_end:
            continue
        macro = node.child_by_field_name("macro")
        if macro is None:
            continue
        macro_name = audit.source_text(content, macro)
        if macro_name not in {"assert", "assert_eq", "assert_ne", "matches"}:
            continue
        source = audit.source_text(content, node)
        arguments = split_macro_arguments(source)
        if "StatusCode::" in source and (
            "status()" in source or (arguments and arguments[0].strip() in bound_status)
        ):
            handled += 1
            continue
        if macro_name == "assert" and ".is_empty()" in source:
            expression = arguments[0]
            path = resolve_body_path(expression, body_aliases)
            if path is None:
                raise ExtractionFailure(f"unsupported response assertion: {source}")
            body_paths.append({"path": path, "length": 0})
            handled += 1
            continue
        if macro_name == "assert" and (
            ".starts_with(" in source or ".contains(" in source
        ):
            expression = arguments[0]
            operator = "starts_with" if ".starts_with(" in expression else "contains"
            receiver, argument = expression.split(f".{operator}(", 1)
            path = resolve_body_path(receiver, body_aliases)
            if path is None:
                raise ExtractionFailure(f"unsupported response assertion: {source}")
            body_paths.append(
                {"path": path, operator: json_scalar(argument.rsplit(")", 1)[0])}
            )
            handled += 1
            continue
        if macro_name == "assert" and ">" in source:
            left, right = arguments[0].rsplit(">", 1)
            path = resolve_body_path(left, body_aliases)
            if path is None or not path:
                raise ExtractionFailure(f"unsupported response assertion: {source}")
            body_paths.append({"path": path, "greater_than": json_scalar(right)})
            handled += 1
            continue
        if macro_name != "assert_eq":
            raise ExtractionFailure(f"unsupported response assertion: {source}")
        if len(arguments) < 2:
            raise ExtractionFailure(f"could not split assertion: {source}")
        left, right = arguments[:2]
        is_direct_json = "json_body(" in left and ".await" in left
        path = (
            json_path(left) if is_direct_json else resolve_body_path(left, body_aliases)
        )
        if not is_direct_json and path is None:
            raise ExtractionFailure(f"assertion is not a response JSON path: {source}")
        assert path is not None
        if ".len()" in left:
            body_paths.append(
                {"path": path, "length": json_scalar(right, package_version)}
            )
            handled += 1
            continue
        if not path:
            raise ExtractionFailure(f"response assertion has no object path: {source}")
        value = json_scalar(right, package_version)
        if any(isinstance(part, int) for part in path):
            body_paths.append({"path": path, "equals": value})
        else:
            set_body_path(body, path, value)
        handled += 1
    expectation: dict[str, Any] = {"status": status}
    if body:
        expectation["body"] = body
    if body_paths:
        expectation["body_paths"] = body_paths
    return expectation, handled


def extract_workflow_cases(
    source: audit.SourceFile,
    function: Node,
    package_version: str | None = None,
) -> list[ExtractedCase]:
    name_node = function.child_by_field_name("name")
    if name_node is None:
        raise ExtractionFailure("test function has no name")
    test_name = audit.source_text(source.content, name_node)
    function_text = audit.source_text(source.content, function)
    role_bindings = session_role_bindings(source.content, function)
    calls = oneshot_calls(source.content, function)
    if len(calls) < 2:
        raise ExtractionFailure(
            f"expected multiple HTTP executions, found {len(calls)}"
        )

    cases: list[ExtractedCase] = []
    for index, call in enumerate(calls):
        request_node = request_argument(source.content, call)
        request_text = audit.source_text(source.content, request_node)
        method, uri = request_method_uri(source.content, request_node)
        role = request_role(function_text, request_text, role_bindings)
        binding = response_binding(source.content, call, function)
        scope_start = call.end_byte
        scope_end = (
            calls[index + 1].start_byte if index + 1 < len(calls) else function.end_byte
        )
        assertions = assertion_nodes(source.content, function, scope_start, scope_end)
        if not assertions:
            raise ExtractionFailure(f"workflow step {binding} has no assertions")
        statuses = {
            status
            for assertion in assertions
            for status in audit.STATUS_PATTERN.findall(
                audit.source_text(source.content, assertion)
            )
        }
        if len(statuses) != 1:
            raise ExtractionFailure(
                f"workflow step {binding} has {len(statuses)} asserted statuses"
            )
        status_name = next(iter(statuses))
        if status_name not in STATUS_VALUES:
            raise ExtractionFailure(f"unsupported status: {status_name}")

        request: dict[str, Any] = {
            "method": method,
            "uri": uri,
            "fixture": "isolated",
            **request_body(source.content, request_node),
        }
        if role is not None:
            request["role"] = role
        expect, handled_assertions = response_expectation(
            source.content,
            function,
            STATUS_VALUES[status_name],
            package_version,
            start_byte=scope_start,
            end_byte=scope_end,
        )
        if handled_assertions != len(assertions):
            raise ExtractionFailure(
                f"workflow step {binding} handled {handled_assertions} of "
                f"{len(assertions)} assertions"
            )
        cases.append(
            ExtractedCase(
                case={
                    "name": f"{test_name}__{index + 1:02}_{binding}",
                    "workflow": test_name,
                    "request": request,
                    "expect": expect,
                    "source": {
                        "path": source.path,
                        "line": call.start_point.row + 1,
                    },
                },
                assertions=handled_assertions,
            )
        )
    return cases


def extract_case(
    source: audit.SourceFile,
    function: Node,
    package_version: str | None = None,
    allow_scenario: bool = False,
) -> ExtractedCase:
    name_node = function.child_by_field_name("name")
    if name_node is None:
        raise ExtractionFailure("test function has no name")
    name = audit.source_text(source.content, name_node)
    signals = audit.collect_signals(source.content, function)
    classification, _, reasons = audit.classify(signals)
    allowed = classification == "automatic_contract" or (
        allow_scenario and classification == "scenario_contract"
    )
    if not allowed:
        raise ExtractionFailure("; ".join(reasons))
    if signals.status_codes[0] not in STATUS_VALUES:
        raise ExtractionFailure(f"unsupported status: {signals.status_codes[0]}")

    request_node = oneshot_request_argument(source.content, function)
    function_text = audit.source_text(source.content, function)
    request_text = audit.source_text(source.content, request_node)
    role = request_role(
        function_text,
        request_text,
        session_role_bindings(source.content, function),
    )
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


def verify_removals(root: Path, manifest: dict[str, Any]) -> None:
    restored_paths = [
        path for path in manifest.get("removed_paths", []) if (root / path).exists()
    ]
    if restored_paths:
        raise ExtractionFailure(
            "migrated Rust test files were restored: " + ", ".join(restored_paths)
        )

    removed_tests = set(manifest.get("removed_tests", []))
    if manifest.get("remove_selected_tests") is True:
        removed_tests.update(manifest.get("tests", []))
    if not removed_tests:
        return
    paths = sorted({identifier.rsplit("::", 1)[0] for identifier in removed_tests})
    present: set[str] = set()
    for source in audit.worktree_sources(root, paths):
        tree = audit.RUST_PARSER.parse(source.content)
        for function in audit.test_functions(source.content, tree.root_node):
            name_node = function.child_by_field_name("name")
            if name_node is None:
                continue
            name = audit.source_text(source.content, name_node)
            identifier = f"{source.path}::{name}"
            if identifier in removed_tests:
                present.add(identifier)
    if present:
        raise ExtractionFailure(
            "migrated Rust test functions were restored: " + ", ".join(sorted(present))
        )


def generate(
    root: Path,
    manifest: dict[str, Any],
    manifest_path: Path = DEFAULT_MANIFEST,
) -> dict[str, Any]:
    with (root / "Cargo.toml").open("rb") as handle:
        package_version = tomllib.load(handle).get("package", {}).get("version")
    if not isinstance(package_version, str):
        raise ExtractionFailure("Cargo.toml package.version is required")
    sources = audit.git_sources(root, manifest["git_ref"], manifest["paths"])
    cases: list[dict[str, Any]] = []
    skipped: list[dict[str, Any]] = []
    automatic = 0
    tests = 0
    selected = set(manifest.get("tests", []))
    allow_scenario = manifest.get("allow_scenario_contracts") is True
    allow_multiple_requests = manifest.get("allow_multiple_requests") is True
    found: set[str] = set()
    for source in sources:
        tree = audit.RUST_PARSER.parse(source.content)
        if tree.root_node.has_error:
            raise ExtractionFailure(f"Rust parse error: {source.path}")
        for function in audit.test_functions(source.content, tree.root_node):
            name_node = function.child_by_field_name("name")
            name = (
                audit.source_text(source.content, name_node) if name_node else "unknown"
            )
            identifier = f"{source.path}::{name}"
            if selected and identifier not in selected:
                continue
            found.add(identifier)
            tests += 1
            signals = audit.collect_signals(source.content, function)
            classification, _, reasons = audit.classify(signals)
            allowed = classification == "automatic_contract" or (
                allow_scenario and classification == "scenario_contract"
            )
            if not allowed:
                skipped.append(
                    {
                        "id": identifier,
                        "classification": classification,
                        "reasons": reasons,
                    }
                )
                continue
            automatic += 1
            try:
                extracted = (
                    extract_workflow_cases(
                        source,
                        function,
                        package_version,
                    )
                    if allow_multiple_requests and signals.oneshot_calls > 1
                    else [
                        extract_case(
                            source,
                            function,
                            package_version,
                            allow_scenario=allow_scenario,
                        )
                    ]
                )
                cases.extend(case.case for case in extracted)
            except ExtractionFailure as error:
                raise ExtractionFailure(f"{identifier}: {error}") from error

    missing = selected - found
    if missing:
        raise ExtractionFailure(
            "selected migration tests are missing from source snapshot: "
            + ", ".join(sorted(missing))
        )

    if tests != manifest.get("expected_tests"):
        raise ExtractionFailure(
            f"migration source test count changed: expected {manifest.get('expected_tests')}, got {tests}"
        )
    if automatic != manifest.get("expected_automatic_contracts"):
        raise ExtractionFailure(
            "automatic contract count changed: "
            f"expected {manifest.get('expected_automatic_contracts')}, got {automatic}"
        )
    expected_generated_cases = manifest.get("expected_generated_cases", automatic)
    if len(cases) != expected_generated_cases:
        raise ExtractionFailure(
            "generated contract count changed: "
            f"expected {expected_generated_cases}, got {len(cases)}"
        )

    return {
        "protocol": 1,
        "generated": True,
        "source": {
            "git_ref": manifest["git_ref"],
            "manifest": manifest_path.resolve().relative_to(root).as_posix(),
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
    manifest_path = args.manifest.resolve()
    manifest = load_manifest(manifest_path)
    report = generate(root, manifest, manifest_path)
    rendered = json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    if args.stdout:
        sys.stdout.write(rendered)
        return 0
    if args.check:
        verify_removals(root, manifest)
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
