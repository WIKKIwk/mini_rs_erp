from __future__ import annotations

import unittest
from pathlib import Path

from extract_all_contracts import repository_path
from extract_contracts import ExtractionFailure


class MigrationRegistryTests(unittest.TestCase):
    def test_rejects_path_outside_repository(self) -> None:
        with self.assertRaisesRegex(ExtractionFailure, "repository-relative"):
            repository_path(Path("/repo"), "../outside.json")


if __name__ == "__main__":
    unittest.main()
