from __future__ import annotations

import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import verify


class HarnessFreshnessTests(unittest.TestCase):
    def test_workflows_are_batched_into_fresh_harnesses(self) -> None:
        contract = {
            "protocol": 1,
            "cases": [
                {"name": "shared", "request": {}, "expect": {}},
                {
                    "name": "a_1",
                    "workflow": "a",
                    "request": {},
                    "expect": {},
                },
                {
                    "name": "b_1",
                    "workflow": "b",
                    "request": {},
                    "expect": {},
                },
                {
                    "name": "a_2",
                    "workflow": "a",
                    "request": {},
                    "expect": {},
                },
            ],
        }

        self.assertEqual(
            [
                [case["name"] for case in batch]
                for batch in verify.case_batches(contract)
            ],
            [["shared"], ["a_1", "a_2"], ["b_1"]],
        )

    def test_reuses_binary_until_a_real_dependency_changes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "target" / "debug" / "mini_rs_verifier_harness"
            dep_info = binary.with_suffix(".d")
            source = root / "src" / "source file.rs"
            for path in (
                binary,
                dep_info,
                source,
                root / "Cargo.toml",
                root / "Cargo.lock",
            ):
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("fixture", encoding="utf-8")
            escaped_source = str(source).replace(" ", "\\ ")
            dep_info.write_text(f"{binary}: {escaped_source}\n", encoding="utf-8")
            for path in (source, root / "Cargo.toml", root / "Cargo.lock"):
                os.utime(path, ns=(100, 100))
            os.utime(binary, ns=(200, 200))

            with (
                patch.object(verify, "ROOT", root),
                patch.object(verify, "HARNESS", binary),
                patch.object(verify, "HARNESS_DEP_INFO", dep_info),
            ):
                self.assertFalse(verify.harness_is_stale())
                os.utime(source, ns=(300, 300))
                self.assertTrue(verify.harness_is_stale())

    def test_response_body_path_oracles(self) -> None:
        errors = verify.response_errors(
            {
                "status": 200,
                "body_paths": [
                    {"path": ["items", 1, "id"], "equals": "B"},
                    {"path": ["items"], "length": 2},
                    {"path": ["score"], "greater_than": 0},
                    {"path": ["prefix"], "starts_with": "ABC"},
                    {"path": ["detail"], "contains": "required"},
                ],
            },
            {
                "status": 200,
                "body": {
                    "items": [{"id": "A"}, {"id": "B"}],
                    "score": 1.5,
                    "prefix": "ABC-123",
                    "detail": "driver_url_required",
                },
            },
        )

        self.assertEqual(errors, [])

    def test_missing_dependency_file_requires_build(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory) / "mini_rs_verifier_harness"
            binary.write_bytes(b"binary")
            with (
                patch.object(verify, "HARNESS", binary),
                patch.object(verify, "HARNESS_DEP_INFO", binary.with_suffix(".d")),
            ):
                self.assertTrue(verify.harness_is_stale())


if __name__ == "__main__":
    unittest.main()
