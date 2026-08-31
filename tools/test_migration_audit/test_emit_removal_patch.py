from __future__ import annotations

import unittest

import audit
from emit_removal_patch import remove_selected_tests


class RemovalPatchTests(unittest.TestCase):
    def test_removes_only_selected_test_and_preserves_next_attribute(self) -> None:
        source = audit.SourceFile(
            "src/example.rs",
            b"""
#[tokio::test]
async fn migrated() {
    assert!(true);
}

#[tokio::test]
async fn retained() {
    assert!(true);
}
""",
        )

        updated, found = remove_selected_tests(source, {"src/example.rs::migrated"})

        self.assertEqual(found, {"src/example.rs::migrated"})
        self.assertNotIn("fn migrated", updated)
        self.assertIn("#[tokio::test]\nasync fn retained", updated)


if __name__ == "__main__":
    unittest.main()
