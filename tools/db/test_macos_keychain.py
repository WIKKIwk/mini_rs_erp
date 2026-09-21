import importlib.util
import os
from pathlib import Path
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location("macos_keychain", Path(__file__).with_name("macos_keychain.py"))
helper = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helper)


class LaunchTests(unittest.TestCase):
    config = {
        "MINI_ERP_DATABASE_URL": "postgres://runtime@127.0.0.1:5432/erp",
        "MINI_ERP_MIGRATION_DATABASE_URL": "postgres://maintenance@127.0.0.1:5432/erp",
        "MINI_ERP_DATABASE_KEYCHAIN_SERVICE": "test-only-service",
    }

    def launch(self, mode, admin=False):
        with patch.object(helper, "settings", return_value=self.config), \
                patch.object(helper, "Keychain") as keychain, \
                patch.object(helper.os, "execvpe") as execute, \
                patch.dict(os.environ, {"PGPASSWORD": "stale", "MINI_ERP_ADMIN_DATABASE_URL": "stale"}):
            keychain.return_value.read.side_effect = lambda service, account: account + ":/@ secret"
            helper.launch(mode, ["example-command", "argument"], admin)
            return execute.call_args.args, keychain.return_value.read.call_args_list

    def test_runtime_never_reads_or_receives_maintenance_secret(self):
        (program, argv, env), reads = self.launch("run")
        self.assertEqual(argv, ["example-command", "argument"])
        self.assertEqual([call.args[1] for call in reads], ["access-code-encryption-key", "runtime"])
        self.assertEqual(env["MINI_ERP_ACCESS_CODE_KEY"], "access-code-encryption-key:/@ secret")
        self.assertEqual(env["MINI_ERP_DATABASE_URL"], env["MINI_ERP_MIGRATION_DATABASE_URL"])
        self.assertEqual(env["MINI_ERP_DATABASE_URL"], self.config["MINI_ERP_DATABASE_URL"])
        self.assertEqual(env["PGPASSWORD"], "runtime:/@ secret")
        self.assertNotIn("MINI_ERP_ADMIN_DATABASE_URL", env)

    def test_migration_explicitly_uses_separate_credentials(self):
        (_, _, env), reads = self.launch("migrate")
        self.assertEqual([call.args[1] for call in reads], ["access-code-encryption-key", "runtime", "maintenance"])
        self.assertIn("maintenance%3A%2F%40%20secret", env["MINI_ERP_MIGRATION_DATABASE_URL"])
        self.assertEqual(env["MINI_ERP_DATABASE_URL"], self.config["MINI_ERP_DATABASE_URL"])
        self.assertEqual(env["PGPASSWORD"], "runtime:/@ secret")

    def test_postgres_tool_argv_and_urls_have_no_secret(self):
        (_, argv, env), reads = self.launch("db", admin=True)
        self.assertEqual(argv, ["example-command", "argument"])
        self.assertEqual([call.args[1] for call in reads], ["maintenance"])
        self.assertEqual(env["PGPASSWORD"], "maintenance:/@ secret")
        self.assertEqual(env["MINI_ERP_DATABASE_URL"], self.config["MINI_ERP_MIGRATION_DATABASE_URL"])
        self.assertEqual(env["PGUSER"], "maintenance")
        self.assertEqual(env["MINI_ERP_KEYCHAIN_RUNTIME_URL"], self.config["MINI_ERP_DATABASE_URL"])
        self.assertEqual(env["MINI_ERP_KEYCHAIN_MAINTENANCE_URL"], self.config["MINI_ERP_MIGRATION_DATABASE_URL"])

    def test_locked_or_missing_keychain_fails_without_exec(self):
        with patch.object(helper, "settings", return_value=self.config), \
                patch.object(helper, "Keychain", side_effect=RuntimeError("unavailable")), \
                patch.object(helper.os, "execvpe") as execute:
            with self.assertRaises(RuntimeError):
                helper.launch("run", ["example-command"], if_configured=True)
            execute.assert_not_called()

    def test_nonlocal_and_password_bearing_urls_are_rejected(self):
        for url in ("postgres://runtime@example.com/erp", "postgres://runtime:secret@localhost/erp",
                    "postgres://runtime@localhost/erp?host=example.com"):
            with self.subTest(url=url), self.assertRaises(RuntimeError):
                helper.local_url(url)


if __name__ == "__main__":
    unittest.main()
