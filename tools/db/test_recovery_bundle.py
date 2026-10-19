"""Recovery checks use only fixture files and an isolated temporary cluster."""
from __future__ import annotations

import base64
import io
import json
import os
from pathlib import Path
import shutil
import socket
import sqlite3
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import recovery_bundle as bundle
import recovery_state as state


class RecoveryFixture:
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="accord-recovery-unit-")
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.root = self.base / "application"
        (self.root / "src").mkdir(parents=True)
        (self.root / "data").mkdir()
        (self.root / "tools/db").mkdir(parents=True)
        (self.root / "src/main.rs").write_text("fn main() {}\n")
        (self.root / "Cargo.toml").write_text('[package]\nname="recovery-fixture"\n')
        (self.root / "Cargo.lock").write_text("version = 4\n")
        for name in ("restore_postgres.sh", "qolip_backup_validation.sh"):
            shutil.copy2(bundle.ROOT / "tools/db" / name, self.root / "tools/db" / name)
        (self.root / "migrations/postgres").mkdir(parents=True)
        shutil.copy2(bundle.ROOT / "migrations/postgres/0104_runtime_security_boundaries.sql",
                     self.root / "migrations/postgres/0104_runtime_security_boundaries.sql")
        self.key = base64.b64encode(bytes(range(32))).decode()
        (self.root / "data/access-code.key").write_text(self.key)
        (self.root / "data/mobile_telegram.json").write_text('{"users":{"worker":"encrypted-session"}}')
        (self.root / "data/mobile_telegram.key").write_text("separate-telegram-key")
        (self.root / "data/fcm-config.enc").write_bytes(b"encrypted-fcm-configuration")
        (self.root / "data/fcm-config.clients.json").write_text('{"android":{"projectId":"fixture"}}')
        self.dump = self.base / "fixture.dump"
        self.dump.write_bytes(b"PGDMP-fixture")
        self.output = self.base / "fixture.accord-backup.tar.gz"
        self.env = {"MINI_ERP_DATABASE_URL": "postgres://mini_rs_erp@127.0.0.1/source",
                    "PGPASSWORD": "fixture-password"}

    def create(self, env=None):
        with self.dump.open("rb") as stream:
            fake_database = stream.read(13) == b"PGDMP-fixture"
        if fake_database:
            with patch.object(bundle, "validate_media"), patch.object(bundle, "pg_tool", return_value="pg_restore"):
                return bundle.create(self.root, self.root, self.dump, self.output, env or self.env)
        return bundle.create(self.root, self.root, self.dump, self.output, env or self.env)

    def extract(self):
        destination = self.base / "unpacked"
        destination.mkdir()
        manifest = bundle.verify(self.output, destination)
        return destination, manifest

    def rewrite(self, transform):
        changed = self.base / "changed.accord-backup.tar.gz"
        with tarfile.open(self.output, "r:gz") as original, tarfile.open(changed, "w:gz") as output:
            for member in original:
                data = original.extractfile(member).read()
                member, data = transform(member, data)
                output.addfile(member, io.BytesIO(data))
        return changed


class BundleTests(RecoveryFixture, unittest.TestCase):
    def test_database_state_keys_and_exact_source_are_in_one_private_archive(self):
        manifest = self.create()
        destination, checked = self.extract()
        env = json.loads((destination / "runtime/environment.json").read_text())
        self.assertEqual(env["MINI_ERP_ACCESS_CODE_KEY"], self.key)
        self.assertEqual(env["PGPASSWORD"], "fixture-password")
        self.assertEqual((destination / "state/data/mobile_telegram.key").read_text(), "separate-telegram-key")
        self.assertEqual((destination / "database/database.dump").read_bytes(), self.dump.read_bytes())
        self.assertIn("source/Cargo.lock", checked["files"])
        self.assertIn("state/data/fcm-config.enc", checked["files"])
        self.assertFalse(manifest["restore_tested"])
        self.assertEqual(self.output.stat().st_mode & 0o777, 0o600)

    def test_committed_sqlite_wal_data_is_copied_without_live_wal_files(self):
        database = self.root / "data/mobile_calculate_orders.sqlite"
        with sqlite3.connect(database) as connection:
            connection.execute("PRAGMA journal_mode=WAL")
            connection.execute("PRAGMA wal_autocheckpoint=0")
            connection.execute("CREATE TABLE orders(id TEXT, state TEXT)")
            connection.execute("INSERT INTO orders VALUES('order-7','running')")
            connection.commit()
            self.assertTrue(Path(str(database) + "-wal").exists())
            self.create()
        destination, manifest = self.extract()
        restored = destination / "state/data/mobile_calculate_orders.sqlite"
        with sqlite3.connect(restored) as connection:
            self.assertEqual(connection.execute("SELECT * FROM orders").fetchall(), [("order-7", "running")])
            self.assertEqual(connection.execute("PRAGMA integrity_check").fetchone(), ("ok",))
        self.assertNotIn("state/data/mobile_calculate_orders.sqlite-wal", manifest["files"])

    def test_external_configuration_paths_and_auxiliary_keys_are_inventoried(self):
        custom = self.base / "external/telegram.json"
        custom.parent.mkdir()
        custom.write_text('{"custom":true}')
        custom.with_suffix(".key").write_text("external-telegram-key")
        empty = self.base / "empty-avatars"
        empty.mkdir()
        env = {**self.env, "MOBILE_API_TELEGRAM_STORE_PATH": str(custom),
               "MOBILE_PROFILE_AVATAR_STORE_DIR": str(empty)}
        manifest = self.create(env)
        destination, _ = self.extract()
        bindings = {item["key"]: item for item in manifest["state"]}
        self.assertEqual((destination / bindings["TELEGRAM_KEY_FILE"]["payload"]).read_text(), "external-telegram-key")
        self.assertEqual(Path(bindings["TELEGRAM_KEY_FILE"]["payload"]).parent,
                         Path(bindings["MOBILE_API_TELEGRAM_STORE_PATH"]["payload"]).parent)
        self.assertTrue(bindings["MOBILE_PROFILE_AVATAR_STORE_DIR"]["directory"])

    def test_missing_required_encryption_key_cannot_produce_ready_archive(self):
        (self.root / "data/access-code.key").unlink()
        with self.assertRaisesRegex(RuntimeError, "encryption key is missing"):
            self.create()
        self.assertFalse(self.output.exists())

    def test_environment_key_takes_precedence_over_old_key_file(self):
        replacement = base64.b64encode(b"z" * 32).decode()
        self.create({**self.env, "MINI_ERP_ACCESS_CODE_KEY": replacement})
        destination, _ = self.extract()
        saved = json.loads((destination / "runtime/environment.json").read_text())
        self.assertEqual(saved["MINI_ERP_ACCESS_CODE_KEY"], replacement)

    def test_same_size_corruption_is_rejected_before_extraction(self):
        self.create()
        def corrupt(member, data):
            if member.name == "database/database.dump":
                data = b"X" * len(data)
            return member, data
        with self.assertRaisesRegex(RuntimeError, "checksum mismatch"):
            bundle.verify(self.rewrite(corrupt))

    def test_archive_traversal_and_symlinks_are_rejected(self):
        self.create()
        def traversal(member, data):
            if member.name == "database/database.dump":
                member.name = "../outside"
            return member, data
        with self.assertRaisesRegex(RuntimeError, "Unsafe"):
            bundle.verify(self.rewrite(traversal))
        def symlink(member, data):
            if member.name == "database/database.dump":
                member.type, member.linkname, member.size = tarfile.SYMTYPE, "/etc/passwd", 0
                data = b""
            return member, data
        with self.assertRaisesRegex(RuntimeError, "links"):
            bundle.verify(self.rewrite(symlink))

    def test_restore_requires_explicit_database_and_new_directory(self):
        self.create()
        with self.assertRaisesRegex(RuntimeError, "explicitly"):
            bundle.restore(self.output, self.base / "restored", {})
        with self.assertRaisesRegex(RuntimeError, "must not exist"):
            bundle.restore(self.output, self.root, {"MINI_ERP_RESTORE_DATABASE_URL": "postgres://localhost/test"})

    def test_remote_failure_does_not_publish_partial_archive(self):
        with patch.object(bundle, "capture_remote", side_effect=RuntimeError("R2 unavailable")):
            with self.assertRaisesRegex(RuntimeError, "R2 unavailable"):
                self.create()
        self.assertFalse(self.output.exists())
        self.assertEqual(list(self.base.glob(".recovery-*")), [])

    def test_credentials_are_not_in_postgres_command_line_url(self):
        url, env = bundle.connection_env("postgres://user:secret%40value@localhost:5432/test?sslmode=require", {})
        self.assertNotIn("secret", url)
        self.assertEqual(env["PGPASSWORD"], "secret@value")
        self.assertTrue(url.endswith("?sslmode=require"))


class PostgresRecoveryTests(RecoveryFixture, unittest.TestCase):
    def setUp(self):
        super().setUp()
        if os.geteuid() == 0:
            self.skipTest("initdb requires a non-root OS account")
        try:
            self.pg_ctl = bundle.pg_tool("pg_ctl")
            initdb = bundle.pg_tool("initdb")
        except RuntimeError:
            self.skipTest("PostgreSQL tools not installed")
        self.cluster = self.base / "cluster"
        subprocess.run([initdb, "-D", str(self.cluster), "-A", "trust", "-U", "recovery_test"],
                       check=True, capture_output=True)
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            self.port = listener.getsockname()[1]
        socket_directory = self.base / "socket"
        socket_directory.mkdir()
        subprocess.run([self.pg_ctl, "-D", str(self.cluster), "-l", str(self.base / "postgres.log"),
                        "-w", "-o", f"-h 127.0.0.1 -p {self.port} -k {socket_directory} -F", "start"],
                       check=True, capture_output=True)
        self.addCleanup(lambda: subprocess.run([self.pg_ctl, "-D", str(self.cluster), "-m", "immediate", "-w", "stop"],
                                              capture_output=True))
        self.admin = f"postgres://recovery_test@127.0.0.1:{self.port}/postgres"
        self.source = self.admin.rsplit("/", 1)[0] + "/source"
        self.target = self.admin.rsplit("/", 1)[0] + "/recovered"
        self.pg_env = {key: value for key, value in os.environ.items()
                       if not key.startswith(state.PREFIXES) and key not in state.EXTRA_ENV}
        bundle.psql(self.admin, "CREATE DATABASE source;\nCREATE DATABASE recovered;", self.pg_env)
        tables = ("mini_items", "mini_warehouses", "mini_warehouse_assignments",
                  "mini_qolip_product_specs", "mini_qolip_locations", "mini_qolip_cell_qrs",
                  "mini_qolip_checkouts", "mini_qolip_order_notes")
        sql = "CREATE TABLE mini_schema_migrations(version TEXT PRIMARY KEY);\n"
        sql += "INSERT INTO mini_schema_migrations VALUES('fixture');\n"
        for name in tables:
            sql += f"CREATE TABLE {name}(id TEXT PRIMARY KEY, payload JSONB);\n"
            sql += f"""INSERT INTO {name} VALUES('fixture', '{{"state":"kept"}}');\n"""
        sql += """CREATE TABLE mini_work_sessions(id TEXT PRIMARY KEY, state JSONB);
INSERT INTO mini_work_sessions VALUES('running-session',
    '{"apparatus":"pechat-1","order":"order-830","meters":1234.5,"status":"running"}');
CREATE TABLE mini_auth_code_vault(id TEXT, ciphertext TEXT);
INSERT INTO mini_auth_code_vault VALUES('worker','encrypted-with-original-key');
CREATE SEQUENCE mini_production_order_number_seq START 830;
SELECT nextval('mini_production_order_number_seq');
"""
        bundle.psql(self.source, sql, self.pg_env)
        subprocess.run([bundle.pg_tool("pg_dump"), "-w", "-Fc", "--compress=0",
                        "--file", str(self.dump), self.source], env=self.pg_env, check=True, capture_output=True)
        self.env = {"MINI_ERP_DATABASE_URL": self.source}

    def test_full_recovery_into_fresh_database_preserves_active_work_and_sequence(self):
        custom = self.base / "custom-state/telegram.json"
        custom.parent.mkdir()
        shutil.copy2(self.root / "data/mobile_telegram.json", custom)
        shutil.copy2(self.root / "data/mobile_telegram.key", custom.with_suffix(".key"))
        self.env["MOBILE_API_TELEGRAM_STORE_PATH"] = str(custom)
        self.create()
        # Simulate total loss of the original runtime, including its encryption key.
        shutil.rmtree(self.root / "data")
        shutil.rmtree(custom.parent)
        target = self.base / "restored-application"
        bundle.restore(self.output, target, {**self.pg_env, "MINI_ERP_RESTORE_DATABASE_URL": self.target})
        sql = "SELECT state::text FROM mini_work_sessions WHERE id='running-session';"
        self.assertEqual(bundle.psql(self.source, sql, self.pg_env), bundle.psql(self.target, sql, self.pg_env))
        self.assertEqual(bundle.psql(self.target, "SELECT nextval('mini_production_order_number_seq');", self.pg_env), "831")
        env = json.loads((target / "recovery-environment.json").read_text())
        self.assertEqual(env["MINI_ERP_ACCESS_CODE_KEY"], self.key)
        self.assertEqual(env["MINI_ERP_DATABASE_URL"], self.target)
        self.assertEqual(Path(env["MOBILE_API_TELEGRAM_STORE_PATH"]).with_suffix(".key").read_text(),
                         "separate-telegram-key")
        self.assertEqual((target / "run-restored.sh").stat().st_mode & 0o777, 0o700)
        self.assertEqual(bundle.psql(self.target, "SELECT ciphertext FROM mini_auth_code_vault;", self.pg_env),
                         "encrypted-with-original-key")

    def test_nonempty_destination_is_not_replaced(self):
        self.create()
        bundle.psql(self.target, "CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('keep');", self.pg_env)
        with self.assertRaisesRegex(RuntimeError, "not empty"):
            bundle.restore(self.output, self.base / "restored", {**self.pg_env, "MINI_ERP_RESTORE_DATABASE_URL": self.target})
        self.assertEqual(bundle.psql(self.target, "SELECT * FROM sentinel;", self.pg_env), "keep")

    def test_backup_script_publishes_and_checksums_complete_artifact(self):
        bundle.psql(self.source, """
CREATE ROLE mini_rs_erp LOGIN PASSWORD 'fixture-source-password' NOSUPERUSER NOCREATEDB NOCREATEROLE;
GRANT SELECT ON ALL TABLES IN SCHEMA public TO mini_rs_erp;
GRANT SELECT ON ALL SEQUENCES IN SCHEMA public TO mini_rs_erp;
CREATE ROLE fixture_owner NOLOGIN;
ALTER TABLE mini_work_sessions OWNER TO fixture_owner;
""", self.pg_env)
        self.env = {"MINI_ERP_DATABASE_URL": self.source.replace("recovery_test@", "mini_rs_erp@"),
                    "PGPASSWORD": "fixture-source-password"}
        binary = self.base / "fixture-server"
        binary.write_text("#!/bin/sh\nexit 0\n")
        binary.chmod(0o700)
        env = {**self.pg_env, **self.env, "MINI_ERP_BACKUP_RUNTIME_ROOT": str(self.root),
               "MINI_ERP_BACKUP_EXECUTABLE": str(binary),
               "MINI_ERP_BACKUP_DIR": str(self.base / "backups"), "MINI_ERP_BACKUP_TIMESTAMP": "full"}
        result = subprocess.run(["bash", str(bundle.ROOT / "tools/db/backup_postgres.sh")],
                                cwd=self.root, env=env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        directory = self.base / "backups/full"
        archive = directory / "source.accord-backup.tar.gz"
        manifest = bundle.verify(archive)
        self.assertTrue(manifest["source"]["binary_included"])
        self.assertIn("state/data/mobile_telegram.key", manifest["files"])
        for line in (directory / "SHA256SUMS").read_text().splitlines():
            expected, name = line.split(maxsplit=1)
            self.assertEqual(bundle.checksum(directory / name.strip()), expected)
        original = bundle.checksum(archive)
        repeated = subprocess.run(["bash", str(bundle.ROOT / "tools/db/backup_postgres.sh")],
                                  cwd=self.root, env=env, capture_output=True)
        self.assertNotEqual(repeated.returncode, 0)
        self.assertEqual(bundle.checksum(archive), original)
        # Recover on a genuinely separate cluster: none of the source's
        # application or ownership roles are already present here.
        second = self.base / "second-cluster"
        subprocess.run([bundle.pg_tool("initdb"), "-D", str(second), "-A", "trust", "-U", "second_admin"],
                       check=True, capture_output=True)
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            port = listener.getsockname()[1]
        socket_directory = self.base / "socket2"
        socket_directory.mkdir()
        subprocess.run([self.pg_ctl, "-D", str(second), "-l", str(self.base / "second.log"),
                        "-w", "-o", f"-h 127.0.0.1 -p {port} -k {socket_directory} -F", "start"],
                       check=True, capture_output=True)
        self.addCleanup(lambda: subprocess.run([self.pg_ctl, "-D", str(second), "-m", "immediate", "-w", "stop"],
                                              capture_output=True))
        admin = f"postgres://second_admin@127.0.0.1:{port}/postgres"
        destination = admin.rsplit("/", 1)[0] + "/recovered"
        bundle.psql(admin, "CREATE DATABASE recovered;", self.pg_env)
        installed = self.base / "portable-recovery"
        bundle.restore(archive, installed, {**self.pg_env, "MINI_ERP_RESTORE_DATABASE_URL": destination})
        saved = json.loads((installed / "recovery-environment.json").read_text())
        hba = second / "pg_hba.conf"
        hba.write_text("host all mini_rs_erp 127.0.0.1/32 scram-sha-256\n" + hba.read_text())
        subprocess.run([self.pg_ctl, "-D", str(second), "reload"], check=True, capture_output=True)
        result = bundle.psql(saved["MINI_ERP_DATABASE_URL"], "SELECT count(*) FROM mini_work_sessions;",
                             {**self.pg_env, "PGPASSWORD": saved["PGPASSWORD"]})
        self.assertEqual(result, "1")
        self.assertEqual(bundle.psql(admin, "SELECT rolsuper FROM pg_roles WHERE rolname='mini_rs_erp';", self.pg_env), "f")
        with self.assertRaises(RuntimeError):
            bundle.psql(saved["MINI_ERP_DATABASE_URL"], "SELECT 1;", {**self.pg_env, "PGPASSWORD": "wrong"})

    def test_current_schema_restores_later_security_rules_and_function_bodies(self):
        result = subprocess.run(
            ["cargo", "test", "--locked", "--test", "recovery_schema", "--", "--ignored", "--nocapture"],
            cwd=bundle.ROOT,
            env={**self.pg_env, "MINI_ERP_RECOVERY_TEST_SOURCE": self.source,
                 "MINI_ERP_RECOVERY_TEST_TARGET": self.target,
                 "MINI_ERP_RECOVERY_TEST_RUNTIME": str(self.root),
                 "MINI_ERP_RECOVERY_TEST_PG_DUMP": bundle.pg_tool("pg_dump")},
            capture_output=True, text=True,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_truncated_dump_cannot_commit_a_partial_schema_restore(self):
        self.dump.write_bytes(self.dump.read_bytes()[:-512])
        bundle.psql(self.target, "CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('keep');", self.pg_env)
        env = {**self.pg_env, "MINI_ERP_RESTORE_DATABASE_URL": self.target,
               "MINI_ERP_RESTORE_DUMP": str(self.dump)}
        result = subprocess.run(["bash", str(bundle.ROOT / "tools/db/restore_postgres.sh")],
                                env=env, capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(bundle.psql(self.target, "SELECT * FROM sentinel;", self.pg_env), "keep")


if __name__ == "__main__":
    unittest.main()
