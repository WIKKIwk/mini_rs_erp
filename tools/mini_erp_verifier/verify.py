#!/usr/bin/env python3
"""Data-driven verifier for Mini ERP's production router.

The verifier compiles the production crate once with a tiny harness feature. It
does not enable or compile Rust `#[cfg(test)]` modules. Contract cases are then
sent to the in-process Axum router over a JSON-lines protocol.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
CONTRACT_PATH = Path(__file__).with_name("contracts.json")
HARNESS = ROOT / "target" / "debug" / "mini_rs_verifier_harness"
ROUTE_PATTERN = re.compile(r'\.route\(\s*"([^"]+)"', re.MULTILINE)
ROLES = (
    "supplier",
    "werka",
    "customer",
    "aparatchi",
    "qolipchi",
    "boyoqchi",
    "material_taminotchi",
    "admin",
)
MIGRATED_RUST_TEST_PATHS = (
    "src/http/router_tests.rs",
    "src/http/router_tests/auth_routes.rs",
    "src/http/router_tests/core_routes.rs",
    "src/http/router_tests/support.rs",
    "src/http/router_tests/werka_routes.rs",
    "src/http/supplier_items_route_tests.rs",
    "src/http/supplier_read_route_tests.rs",
    "src/http/werka_archive_route_tests.rs",
    "src/http/werka_directory_route_tests.rs",
    "src/http/werka_items_route_tests.rs",
    "src/http/werka_route_tests.rs",
)


class VerificationFailure(RuntimeError):
    pass


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--no-build",
        action="store_true",
        help="reuse the existing verifier harness binary",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="emit one compact JSON result",
    )
    parser.add_argument(
        "--print-route-snapshot",
        action="store_true",
        help="print the current route count and hash without running the harness",
    )
    return parser.parse_args()


def load_contract() -> dict[str, Any]:
    with CONTRACT_PATH.open(encoding="utf-8") as handle:
        contract = json.load(handle)
    if contract.get("protocol") != 1 or not isinstance(contract.get("cases"), list):
        raise VerificationFailure("unsupported or malformed verifier contract")
    return contract


def expanded_cases(contract: dict[str, Any]) -> list[dict[str, Any]]:
    cases = list(contract["cases"])
    for matrix in contract.get("access_matrices", []):
        allowed_roles = set(matrix["allowed_roles"])
        routes = matrix.get("routes") or [
            {"name": matrix["name"], "uri": matrix["uri"]}
        ]
        for route in routes:
            for method in matrix["methods"]:
                cases.append(
                    {
                        "name": f"{route['name']}_{method.lower()}_anonymous",
                        "request": {"method": method, "uri": route["uri"]},
                        "expect": {"status": matrix["anonymous_status"]},
                    }
                )
                for role in ROLES:
                    expected_status = (
                        matrix["allowed_status"]
                        if role in allowed_roles
                        else matrix["forbidden_status"]
                    )
                    cases.append(
                        {
                            "name": f"{route['name']}_{method.lower()}_{role}",
                            "request": {
                                "method": method,
                                "uri": route["uri"],
                                "role": role,
                            },
                            "expect": {"status": expected_status},
                        }
                    )
    return cases


def route_snapshot() -> dict[str, Any]:
    route_files = sorted((ROOT / "src" / "http" / "router").rglob("*.rs"))
    routes: list[str] = []
    for path in route_files:
        routes.extend(ROUTE_PATTERN.findall(path.read_text(encoding="utf-8")))
    normalized = sorted(routes)
    digest = hashlib.sha256(("\n".join(normalized) + "\n").encode()).hexdigest()
    duplicates = sorted({route for route in normalized if normalized.count(route) > 1})
    return {"count": len(normalized), "sha256": digest, "duplicates": duplicates}


def verify_route_snapshot(contract: dict[str, Any]) -> dict[str, Any]:
    actual = route_snapshot()
    expected = contract.get("route_snapshot", {})
    if actual["duplicates"]:
        raise VerificationFailure(
            "duplicate route declarations: " + ", ".join(actual["duplicates"])
        )
    if actual["count"] != expected.get("count") or actual["sha256"] != expected.get(
        "sha256"
    ):
        raise VerificationFailure(
            "route snapshot changed: "
            f"expected count={expected.get('count')} sha256={expected.get('sha256')}; "
            f"actual count={actual['count']} sha256={actual['sha256']}"
        )
    return actual


def verify_migrated_tests_stay_removed() -> None:
    restored = [path for path in MIGRATED_RUST_TEST_PATHS if (ROOT / path).exists()]
    if restored:
        raise VerificationFailure(
            "generic Rust tests were restored after verifier migration: "
            + ", ".join(restored)
        )


def build_harness() -> float:
    started = time.monotonic()
    process = subprocess.run(
        [
            "cargo",
            "build",
            "--quiet",
            "--locked",
            "--features",
            "verification",
            "--bin",
            "mini_rs_verifier_harness",
        ],
        cwd=ROOT,
        text=True,
        capture_output=True,
    )
    if process.returncode != 0:
        detail = process.stderr.strip() or process.stdout.strip()
        raise VerificationFailure(f"verifier harness build failed:\n{detail}")
    return time.monotonic() - started


def runtime_environment(workspace: Path) -> dict[str, str]:
    keep = (
        "CARGO_HOME",
        "DYLD_FALLBACK_LIBRARY_PATH",
        "HOME",
        "LANG",
        "LC_ALL",
        "PATH",
        "RUST_BACKTRACE",
        "RUSTUP_HOME",
        "SSL_CERT_DIR",
        "SSL_CERT_FILE",
        "TMPDIR",
        "USER",
    )
    env = {key: os.environ[key] for key in keep if key in os.environ}
    data = workspace / "data"
    release_dir = workspace / "mobile_releases"
    release_dir.mkdir()
    apk = b"accord-mobile-apk"
    apk_name = "accord-mobile-0.2.0-5.apk"
    (release_dir / apk_name).write_bytes(apk)
    (release_dir / "android.json").write_text(
        json.dumps(
            {
                "version_code": 5,
                "version_name": "0.2.0",
                "minimum_supported_version_code": 4,
                "mandatory": False,
                "apk_file": apk_name,
                "sha256": hashlib.sha256(apk).hexdigest(),
                "size_bytes": len(apk),
                "release_notes": "Verifier fixture",
                "published_at": "2026-07-23T12:00:00Z",
            }
        ),
        encoding="utf-8",
    )
    env.update(
        {
            "MINI_ERP_VERIFIER_TMP": str(workspace),
            "MOBILE_API_SESSION_STORE_BACKEND": "json",
            "MOBILE_API_PROFILE_STORE_BACKEND": "json",
            "MOBILE_API_PUSH_TOKEN_STORE_BACKEND": "json",
            "MOBILE_API_ADMIN_STORE_PATH": str(data / "admin.json"),
            "MOBILE_API_ROLE_STORE_PATH": str(data / "roles.json"),
            "MOBILE_API_TELEGRAM_STORE_PATH": str(data / "telegram.json"),
            "MOBILE_API_RPS_BATCH_LMDB_PATH": str(data / "rps_batches.lmdb"),
            "MOBILE_API_CALCULATE_ORDER_STORE_PATH": str(data / "orders.sqlite"),
            "MOBILE_API_CALCULATE_MATERIAL_STORE_PATH": str(data / "materials.sqlite"),
            "MOBILE_API_CALCULATE_ORDER_IMAGE_DIR": str(data / "order_images"),
            "MOBILE_API_LOCAL_STORE_ALLOW_JSON_FALLBACK": "1",
            "MOBILE_APP_RELEASE_DIR": str(release_dir),
        }
    )
    return env


def subset_errors(expected: Any, actual: Any, path: str = "body") -> list[str]:
    if isinstance(expected, dict):
        if not isinstance(actual, dict):
            return [f"{path}: expected object subset, got {actual!r}"]
        errors: list[str] = []
        for key, value in expected.items():
            if key not in actual:
                errors.append(f"{path}.{key}: missing")
            else:
                errors.extend(subset_errors(value, actual[key], f"{path}.{key}"))
        return errors
    if isinstance(expected, list):
        if not isinstance(actual, list):
            return [f"{path}: expected list, got {actual!r}"]
        if len(expected) != len(actual):
            return [f"{path}: expected {len(expected)} items, got {len(actual)}"]
        errors: list[str] = []
        for index, (expected_item, actual_item) in enumerate(zip(expected, actual)):
            errors.extend(subset_errors(expected_item, actual_item, f"{path}[{index}]"))
        return errors
    if expected != actual:
        return [f"{path}: expected {expected!r}, got {actual!r}"]
    return []


def response_errors(expect: dict[str, Any], actual: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if actual.get("status") != expect.get("status"):
        errors.append(
            f"status: expected {expect.get('status')}, got {actual.get('status')}"
        )
    if "body" in expect:
        errors.extend(subset_errors(expect["body"], actual.get("body")))
    if "body_starts_with" in expect:
        body = actual.get("body")
        if not isinstance(body, str) or not body.startswith(expect["body_starts_with"]):
            errors.append(f"body: missing prefix {expect['body_starts_with']!r}")
    if "body_ends_with" in expect:
        body = actual.get("body")
        if not isinstance(body, str) or not body.endswith(expect["body_ends_with"]):
            errors.append(f"body: missing suffix {expect['body_ends_with']!r}")
    if isinstance(actual.get("body"), dict):
        for key in expect.get("body_absent", []):
            if key in actual["body"]:
                errors.append(f"body.{key}: expected absent")
    actual_headers = actual.get("headers", {})
    for name, rule in expect.get("headers", {}).items():
        value = actual_headers.get(name.lower())
        if value is None:
            errors.append(f"header {name}: missing")
        elif "equals" in rule and value != rule["equals"]:
            errors.append(f"header {name}: expected {rule['equals']!r}, got {value!r}")
        elif "contains" in rule and rule["contains"].lower() not in value.lower():
            errors.append(f"header {name}: missing substring {rule['contains']!r}")
        elif "contains_token" in rule and rule["contains_token"].lower() not in {
            token.strip().lower() for token in value.split(",")
        }:
            errors.append(f"header {name}: missing token {rule['contains_token']!r}")
    return errors


def run_cases(contract: dict[str, Any]) -> tuple[list[dict[str, Any]], float]:
    if not HARNESS.is_file():
        raise VerificationFailure(f"verifier harness not found: {HARNESS}")
    started = time.monotonic()
    failures: list[dict[str, Any]] = []
    with tempfile.TemporaryDirectory(prefix="mini-rs-verifier-") as directory:
        workspace = Path(directory)
        process = subprocess.Popen(
            [str(HARNESS)],
            cwd=ROOT,
            env=runtime_environment(workspace),
            text=True,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            bufsize=1,
        )
        assert process.stdin is not None
        assert process.stdout is not None
        ready_line = process.stdout.readline()
        if not ready_line:
            stderr = process.stderr.read() if process.stderr else ""
            raise VerificationFailure(
                f"verifier harness exited before ready: {stderr.strip()}"
            )
        ready = json.loads(ready_line)
        if ready.get("ready") is not True or ready.get("protocol") != contract["protocol"]:
            raise VerificationFailure(f"unexpected verifier handshake: {ready!r}")

        for case in expanded_cases(contract):
            process.stdin.write(json.dumps(case["request"], separators=(",", ":")) + "\n")
            process.stdin.flush()
            line = process.stdout.readline()
            if not line:
                stderr = process.stderr.read() if process.stderr else ""
                raise VerificationFailure(
                    f"harness stopped during {case['name']}: {stderr.strip()}"
                )
            actual = json.loads(line)
            if "error" in actual:
                failures.append({"name": case["name"], "errors": [actual["error"]]})
                continue
            errors = response_errors(case["expect"], actual)
            if errors:
                failures.append(
                    {"name": case["name"], "errors": errors, "response": actual}
                )

        process.stdin.close()
        return_code = process.wait(timeout=10)
        if return_code != 0:
            stderr = process.stderr.read() if process.stderr else ""
            raise VerificationFailure(f"verifier harness failed: {stderr.strip()}")
    return failures, time.monotonic() - started


def main() -> int:
    args = parse_args()
    try:
        contract = load_contract()
        verify_migrated_tests_stay_removed()
        snapshot = route_snapshot()
        if args.print_route_snapshot:
            print(json.dumps(snapshot, sort_keys=True))
            return 0
        verified_snapshot = verify_route_snapshot(contract)
        build_seconds = 0.0 if args.no_build else build_harness()
        failures, probe_seconds = run_cases(contract)
        result = {
            "ok": not failures,
            "cases": len(expanded_cases(contract)),
            "failures": failures,
            "routes": verified_snapshot["count"],
            "build_seconds": round(build_seconds, 3),
            "probe_seconds": round(probe_seconds, 3),
            "rust_test_modules_compiled": False,
            "migrated_rust_tests": 80,
        }
    except (OSError, ValueError, VerificationFailure, subprocess.TimeoutExpired) as error:
        result = {"ok": False, "error": str(error)}

    if args.json:
        print(json.dumps(result, separators=(",", ":"), sort_keys=True))
    elif result.get("ok"):
        print(
            f"PASS: {result['cases']} contracts, {result['routes']} routes; "
            f"build {result['build_seconds']:.3f}s, probes {result['probe_seconds']:.3f}s; "
            "Rust test modules were not compiled"
        )
    else:
        print(json.dumps(result, indent=2, sort_keys=True), file=sys.stderr)
    return 0 if result.get("ok") else 1


if __name__ == "__main__":
    raise SystemExit(main())
