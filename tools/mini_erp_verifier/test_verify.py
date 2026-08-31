from __future__ import annotations

import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import verify


class HarnessFreshnessTests(unittest.TestCase):
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
