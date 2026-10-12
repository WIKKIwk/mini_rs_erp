import hashlib
from pathlib import Path
import tempfile
import unittest

import check


class HygieneTests(unittest.TestCase):
    def test_formatting_exception_expires_on_any_edit(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "old.rs"
            path.write_text("fn old( ) {}\n")
            baseline = {"formatting_exemptions": {"old.rs": hashlib.sha256(path.read_bytes()).hexdigest()}}
            output = f"Diff in {path}:1:\n"
            self.assertEqual(check.formatting_errors(output, root, baseline), [])
            path.write_text("fn changed( ) {}\n")
            self.assertEqual(check.formatting_errors(output, root, baseline), ["rustfmt: old.rs"])
            self.assertEqual(check.formatting_errors(output, root, {}), ["rustfmt: old.rs"])

    def test_unrecognized_rustfmt_failure_is_not_suppressed(self):
        self.assertTrue(check.formatting_errors("error: cannot parse source", Path.cwd(), {}))
        self.assertTrue(check.formatting_errors("Diff in old.rs:1:\nerror[E0753]: bad doc comment", Path.cwd(), {}))

    def test_new_file_limit_and_no_growth_of_legacy_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "src").mkdir()
            path = root / "src" / "module.rs"
            path.write_text("line\n" * 501)
            self.assertTrue(check.size_errors(root, {}))
            baseline = {"legacy_line_limits": {"src/module.rs": 600}}
            self.assertEqual(check.size_errors(root, baseline), [])
            path.write_text("line\n" * 601)
            self.assertTrue(check.size_errors(root, baseline))


if __name__ == "__main__":
    unittest.main()
