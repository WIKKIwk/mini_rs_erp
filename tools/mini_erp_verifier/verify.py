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
import shlex
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
CONTRACT_PATH = Path(__file__).with_name("contracts.json")
MIGRATION_INDEX_PATH = (
    ROOT / "tools" / "test_migration_audit" / "migrations" / "index.json"
)
HARNESS = ROOT / "target" / "debug" / "mini_rs_verifier_harness"
HARNESS_DEP_INFO = HARNESS.with_suffix(".d")
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


def load_json(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as handle:
        value = json.load(handle)
    if not isinstance(value, dict):
        raise VerificationFailure(f"expected JSON object: {path}")
    return value


def load_migration_manifest(path: Path) -> dict[str, Any]:
    manifest = load_json(path)
    if (
        manifest.get("protocol") != 1
        or not isinstance(manifest.get("paths"), list)
        or not isinstance(manifest.get("expected_tests"), int)
        or not isinstance(manifest.get("expected_automatic_contracts"), int)
        or not all(isinstance(path, str) for path in manifest["paths"])
    ):
        raise VerificationFailure("unsupported or malformed migration manifest")
    return manifest


def load_generated_bundles() -> list[tuple[Path, Path]]:
    index = load_json(MIGRATION_INDEX_PATH)
    bundles = index.get("bundles")
    if index.get("protocol") != 1 or not isinstance(bundles, list):
        raise VerificationFailure("unsupported or malformed migration index")
    resolved: list[tuple[Path, Path]] = []
    for bundle in bundles:
        if not isinstance(bundle, dict):
            raise VerificationFailure("malformed migration index bundle")
        manifest = bundle.get("manifest")
        output = bundle.get("output")
        if not isinstance(manifest, str) or not isinstance(output, str):
            raise VerificationFailure("malformed migration index paths")
        if any(
            Path(value).is_absolute() or ".." in Path(value).parts
            for value in (manifest, output)
        ):
            raise VerificationFailure(
                "migration index paths must be repository-relative"
            )
        resolved.append((ROOT / output, ROOT / manifest))
    return resolved


def load_contract() -> tuple[dict[str, Any], list[dict[str, Any]]]:
    contract = load_json(CONTRACT_PATH)
    if contract.get("protocol") != 1 or not isinstance(contract.get("cases"), list):
        raise VerificationFailure("unsupported or malformed verifier contract")

    manifests: list[dict[str, Any]] = []
    generated_cases: list[dict[str, Any]] = []
    generated_source_tests = 0
    for generated_path, manifest_path in load_generated_bundles():
        manifest = load_migration_manifest(manifest_path)
        generated = load_json(generated_path)
        if (
            generated.get("protocol") != contract["protocol"]
            or generated.get("generated") is not True
            or not isinstance(generated.get("cases"), list)
        ):
            raise VerificationFailure(
                f"unsupported or malformed generated contracts: {generated_path}"
            )
        generated_summary = generated.get("summary", {})
        expected_generated = manifest["expected_automatic_contracts"]
        if (
            generated_summary.get("source_tests") != manifest["expected_tests"]
            or generated_summary.get("generated_cases") != expected_generated
            or len(generated["cases"]) != expected_generated
        ):
            raise VerificationFailure(
                "generated contract counts do not match migration manifest: "
                f"{manifest_path}"
            )
        manifests.append(manifest)
        generated_cases.extend(generated["cases"])
        generated_source_tests += manifest["expected_tests"]

    contract["cases"] = [*contract["cases"], *generated_cases]
    if not all(
        isinstance(case, dict) and isinstance(case.get("name"), str)
        for case in contract["cases"]
    ):
        raise VerificationFailure("verifier cases must be named JSON objects")
    names = [case["name"] for case in contract["cases"]]
    duplicates = sorted({name for name in names if names.count(name) > 1})
    if duplicates:
        raise VerificationFailure(
            "duplicate verifier contract names: " + ", ".join(duplicates)
        )
    contract["generated_summary"] = {
        "generated_cases": len(generated_cases),
        "source_tests": generated_source_tests,
    }
    return contract, manifests


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


def verify_migrated_tests_stay_removed(manifests: list[dict[str, Any]]) -> None:
    restored = [
        path
        for manifest in manifests
        for path in manifest.get("removed_paths", [])
        if (ROOT / path).exists()
    ]
    if restored:
        raise VerificationFailure(
            "generic Rust tests were restored after verifier migration: "
            + ", ".join(restored)
        )
    restored_tests: list[str] = []
    for manifest in manifests:
        if manifest.get("remove_selected_tests") is not True:
            continue
        for identifier in manifest.get("tests", []):
            path, name = identifier.rsplit("::", 1)
            source = (ROOT / path).read_text(encoding="utf-8")
            if re.search(rf"\b(?:async\s+)?fn\s+{re.escape(name)}\s*\(", source):
                restored_tests.append(identifier)
    if restored_tests:
        raise VerificationFailure(
            "generic Rust test functions were restored after verifier migration: "
            + ", ".join(restored_tests)
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
        check=False,
    )
    if process.returncode != 0:
        detail = process.stderr.strip() or process.stdout.strip()
        raise VerificationFailure(f"verifier harness build failed:\n{detail}")
    return time.monotonic() - started


def build_configuration_inputs() -> list[Path]:
    inputs = [ROOT / "Cargo.lock", ROOT / "Cargo.toml"]
    inputs.extend((ROOT / "crates").rglob("Cargo.toml"))
    inputs.extend((ROOT / "crates").rglob("build.rs"))
    inputs.extend(
        path
        for path in (
            ROOT / "build.rs",
            ROOT / "rust-toolchain",
            ROOT / "rust-toolchain.toml",
            ROOT / ".cargo" / "config",
            ROOT / ".cargo" / "config.toml",
        )
        if path.exists()
    )
    return inputs


def harness_is_stale() -> bool:
    if not HARNESS.is_file() or not HARNESS_DEP_INFO.is_file():
        return True
    try:
        words = shlex.split(HARNESS_DEP_INFO.read_text(encoding="utf-8"))
        if not words or not words[0].endswith(":"):
            return True
        dependencies = [Path(word) for word in words[1:]]
        dependencies.extend(build_configuration_inputs())
        built_at = HARNESS.stat().st_mtime_ns
        return any(
            not dependency.is_file() or dependency.stat().st_mtime_ns > built_at
            for dependency in dependencies
        )
    except (OSError, ValueError):
        return True


def ensure_harness() -> tuple[float, bool]:
    if not harness_is_stale():
        return 0.0, False
    return build_harness(), True


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


def body_path_value(body: Any, path: list[str | int]) -> tuple[bool, Any]:
    current = body
    for part in path:
        if isinstance(part, int):
            if not isinstance(current, list) or part >= len(current):
                return False, None
            current = current[part]
        else:
            if not isinstance(current, dict) or part not in current:
                return False, None
            current = current[part]
    return True, current


def response_errors(expect: dict[str, Any], actual: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if actual.get("status") != expect.get("status"):
        errors.append(
            f"status: expected {expect.get('status')}, got {actual.get('status')}"
        )
    if "body" in expect:
        errors.extend(subset_errors(expect["body"], actual.get("body")))
    for rule in expect.get("body_paths", []):
        path = rule.get("path", [])
        found, value = body_path_value(actual.get("body"), path)
        label = "body" + "".join(
            f"[{part}]" if isinstance(part, int) else f".{part}" for part in path
        )
        if not found:
            errors.append(f"{label}: missing")
        elif "equals" in rule and value != rule["equals"]:
            errors.append(f"{label}: expected {rule['equals']!r}, got {value!r}")
        elif "length" in rule and (
            not hasattr(value, "__len__") or len(value) != rule["length"]
        ):
            actual_length = len(value) if hasattr(value, "__len__") else None
            errors.append(
                f"{label}: expected length {rule['length']}, got {actual_length}"
            )
        elif "greater_than" in rule and (
            not isinstance(value, (int, float)) or value <= rule["greater_than"]
        ):
            errors.append(
                f"{label}: expected greater than {rule['greater_than']}, got {value!r}"
            )
        elif "starts_with" in rule and (
            not isinstance(value, str) or not value.startswith(rule["starts_with"])
        ):
            errors.append(
                f"{label}: expected prefix {rule['starts_with']!r}, got {value!r}"
            )
        elif "contains" in rule and (
            not isinstance(value, str) or rule["contains"] not in value
        ):
            errors.append(
                f"{label}: expected substring {rule['contains']!r}, got {value!r}"
            )
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
        if (
            ready.get("ready") is not True
            or ready.get("protocol") != contract["protocol"]
        ):
            raise VerificationFailure(f"unexpected verifier handshake: {ready!r}")

        for case in expanded_cases(contract):
            process.stdin.write(
                json.dumps(case["request"], separators=(",", ":")) + "\n"
            )
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
        contract, migration_manifests = load_contract()
        verify_migrated_tests_stay_removed(migration_manifests)
        snapshot = route_snapshot()
        if args.print_route_snapshot:
            print(json.dumps(snapshot, sort_keys=True))
            return 0
        verified_snapshot = verify_route_snapshot(contract)
        build_seconds, harness_rebuilt = (
            (0.0, False) if args.no_build else ensure_harness()
        )
        failures, probe_seconds = run_cases(contract)
        result = {
            "ok": not failures,
            "cases": len(expanded_cases(contract)),
            "failures": failures,
            "routes": verified_snapshot["count"],
            "build_seconds": round(build_seconds, 3),
            "probe_seconds": round(probe_seconds, 3),
            "rust_test_modules_compiled": False,
            "migrated_rust_tests": sum(
                manifest["expected_tests"] for manifest in migration_manifests
            ),
            "generated_contracts": contract["generated_summary"].get(
                "generated_cases", 0
            ),
            "harness_rebuilt": harness_rebuilt,
        }
    except (
        OSError,
        ValueError,
        VerificationFailure,
        subprocess.TimeoutExpired,
    ) as error:
        result = {"ok": False, "error": str(error)}

    if args.json:
        print(json.dumps(result, separators=(",", ":"), sort_keys=True))
    elif result.get("ok"):
        print(
            f"PASS: {result['cases']} contracts, {result['routes']} routes; "
            f"build {result['build_seconds']:.3f}s, probes {result['probe_seconds']:.3f}s; "
            f"harness {'rebuilt' if result['harness_rebuilt'] else 'cached'}; "
            "Rust test modules were not compiled"
        )
    else:
        print(json.dumps(result, indent=2, sort_keys=True), file=sys.stderr)
    return 0 if result.get("ok") else 1


if __name__ == "__main__":
    raise SystemExit(main())
