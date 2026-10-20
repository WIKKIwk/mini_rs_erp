#!/usr/bin/env python3
"""Private, localhost-only fixtures for order_stream_benchmark.py.

Run through tools/db/macos_keychain.py --admin db. Source business rows are
only read by pg_dump. Every mutation is restricted to newly restored databases
whose names begin with bench_order_stream_. Secrets stay in process memory.
"""
import argparse
import base64
import hashlib
import hmac
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import signal
import sqlite3
import subprocess
import time
from urllib.parse import urlunsplit
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
DB_PREFIX = "bench_order_stream_"


def private_json(path, value):
    path = Path(path)
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(value, indent=2) + "\n")
    temporary.chmod(0o600)
    temporary.replace(path)


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


class Fixture:
    def __init__(self, run_dir):
        self.run_dir = Path(run_dir).resolve()
        self.run_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
        self.run_dir.chmod(0o700)
        if os.environ.get("PGHOST") not in ("127.0.0.1", "localhost", "::1"):
            raise RuntimeError("Run via the local maintenance Keychain wrapper")
        if os.environ.get("MINI_ERP_KEYCHAIN_MAINTENANCE_ACTIVE") != "1":
            raise RuntimeError("Explicit maintenance wrapper is required")
        spec = importlib.util.spec_from_file_location(
            "order_stream_keys", ROOT / "tools/db/macos_keychain.py"
        )
        self.keys = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.keys)
        self.config = self.keys.settings()
        self.runtime = self.keys.local_url(self.config["MINI_ERP_DATABASE_URL"])
        self.keychain = self.keys.Keychain()
        self.service = self.config["MINI_ERP_DATABASE_KEYCHAIN_SERVICE"]
        self.pg_bin = ROOT.parent / ".tools/postgres/current/bin"
        self.metadata_path = self.run_dir / "fixture.json"
        self.metadata = (
            json.loads(self.metadata_path.read_text())
            if self.metadata_path.exists()
            else {}
        )

    def save(self):
        private_json(self.metadata_path, self.metadata)

    def sql(self, sql, database=None):
        env = os.environ.copy()
        if database:
            env["PGDATABASE"] = database
        result = subprocess.run(
            [str(self.pg_bin / "psql"), "-XAt", "-v", "ON_ERROR_STOP=1"],
            input=sql, text=True, capture_output=True, env=env,
        )
        if result.returncode:
            raise RuntimeError("Local benchmark SQL failed; query output withheld")
        return result.stdout.strip()

    def command(self, name, arguments, env=None):
        result = subprocess.run(
            [str(self.pg_bin / name), *arguments],
            env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
        )
        if result.returncode:
            raise RuntimeError("Local benchmark " + name + " failed; output withheld")

    def prepare(self, baseline_revision):
        if self.metadata:
            if self.metadata["baseline_revision"] != baseline_revision:
                raise RuntimeError("Existing fixture belongs to a different baseline")
            if sha256(self.run_dir / "source.dump") != self.metadata["dump_sha256"]:
                raise RuntimeError("Fixture dump fingerprint changed")
            return
        dump = self.run_dir / "source.dump"
        self.command("pg_dump", ["-Fc", "--file", str(dump)])
        dump.chmod(0o600)
        aux = self.run_dir / "auxiliary"
        aux.mkdir(mode=0o700, exist_ok=True)
        for name in ("mobile_roles.json", "mobile_admin_store.json"):
            source = ROOT / "data" / name
            if source.exists():
                shutil.copy2(source, aux / name)
                (aux / name).chmod(0o600)
        for name in (
            "mobile_calculate_orders.sqlite",
            "mobile_calculate_materials.sqlite",
            "mobile_apparatus_groups.sqlite",
        ):
            source = ROOT / "data" / name
            if source.exists():
                uri = "file:" + str(source) + "?mode=ro"
                if not Path(str(source) + "-wal").exists():
                    uri += "&immutable=1"
                with sqlite3.connect(uri, uri=True) as db:
                    with sqlite3.connect(aux / name) as target:
                        db.backup(target)
                (aux / name).chmod(0o600)
        counts = json.loads(self.sql("""
            SELECT json_build_object(
                'production_maps', (SELECT count(*) FROM mini_production_maps),
                'workers', (SELECT count(*) FROM mini_workers),
                'database_bytes', pg_database_size(current_database()));
        """))
        self.metadata = {
            "baseline_revision": baseline_revision,
            "dump_sha256": sha256(dump),
            "dump_bytes": dump.stat().st_size,
            "source_counts": counts,
            "tag": time.strftime("%Y%m%d%H%M%S"),
            "source_writes": False,
            "servers": {},
        }
        self.save()

    def server_env(self, database, port, migration=False, query_log=False):
        if not re.fullmatch(DB_PREFIX + r"[a-z0-9_]+", database):
            raise RuntimeError("Only isolated benchmark databases are accepted")
        env = {
            key: value for key, value in os.environ.items()
            if key in ("PATH", "HOME", "USER", "TMPDIR", "LANG")
        }
        database_url = urlunsplit(self.runtime._replace(path="/" + database))
        env.update(
            PGPASSWORD=self.keychain.read(self.service, self.runtime.username),
            MINI_ERP_ACCESS_CODE_KEY=self.keychain.read(
                self.service, "access-code-encryption-key"
            ),
            MINI_ERP_DATABASE_URL=database_url,
            MINI_ERP_MIGRATION_DATABASE_URL=database_url,
            MOBILE_API_ADDR="127.0.0.1:" + str(port),
            MINI_ERP_PG_MAX_CONNECTIONS="16",
            RUST_LOG="warn,sqlx::query=debug" if query_log else "warn",
        )
        if query_log:
            # SQL diagnostic isolates this controlled stream workload. The
            # normal timed runs retain the production-default 30 s reconciler.
            # Zero performs the initial reconciliation once, then exits its loop.
            env["MINI_ORDER_SYNC_INTERVAL_SECONDS"] = "0"
        if migration:
            maintenance = self.keys.local_url(
                self.config["MINI_ERP_MIGRATION_DATABASE_URL"]
            )._replace(path="/" + database)
            env["MINI_ERP_MIGRATION_DATABASE_URL"] = self.keys.password_url(
                maintenance, os.environ["PGPASSWORD"]
            )
        return env

    def seed_clone_credentials(self, database):
        """Give cloned accounts private benchmark credentials, keeping scopes."""
        from argon2 import PasswordHasher
        from cryptography.hazmat.primitives.ciphers.aead import AESGCM
        if not database.startswith(DB_PREFIX):
            raise RuntimeError("Credential fixtures are restricted to test clones")
        seed_path = self.run_dir / "credential-seed.bin"
        if not seed_path.exists():
            with open(seed_path, "xb") as target:
                target.write(os.urandom(32))
            seed_path.chmod(0o600)
        seed = seed_path.read_bytes()
        principals = json.loads(self.sql("""
            SELECT coalesce(json_agg(x),'[]') FROM (
              SELECT a.principal_ref,
                CASE WHEN w.id IS NOT NULL THEN '40'
                  WHEN s.role='qolipchi' THEN '50'
                  WHEN s.role='boyoqchi' THEN '80'
                  WHEN s.role='tayyorlov_masteri' THEN '90'
                  WHEN s.role='homashyo_rezkachi' THEN '91'
                  WHEN a.principal_ref='werka' THEN '20'
                  WHEN a.principal_ref='material_taminotchi' THEN '70'
                  ELSE '99' END AS prefix
              FROM mini_auth_accounts a
              LEFT JOIN mini_workers w ON w.id=a.principal_ref
              LEFT JOIN mini_system_users s ON s.id=a.principal_ref
              WHERE a.credential_hash IS NOT NULL AND
                (w.id IS NOT NULL OR s.id IS NOT NULL OR
                 a.principal_ref IN ('admin','werka','material_taminotchi'))
            ) x;
        """, database))
        key = AESGCM(base64.b64decode(
            self.keychain.read(self.service, "access-code-encryption-key")
        ))
        hasher = PasswordHasher(time_cost=2, memory_cost=19456, parallelism=1)
        quote = lambda value: "'" + value.replace("'", "''") + "'"
        statements = ["BEGIN"]
        for record in principals:
            principal = record["principal_ref"]
            code = record["prefix"] + str(int.from_bytes(
                hmac.new(seed, principal.encode(), hashlib.sha256).digest()[:6], "big"
            )).zfill(15)
            credential_hash = hasher.hash(code)
            nonce = os.urandom(12)
            binding = (
                "mini-rs-erp/access-code/v1\0" + principal + "\0" + credential_hash
            ).encode()
            encrypted = "v1:" + base64.b64encode(
                nonce + key.encrypt(nonce, code.encode(), binding)
            ).decode()
            statements.append(
                "UPDATE mini_auth_accounts SET credential_hash=" + quote(credential_hash)
                + " WHERE principal_ref=" + quote(principal)
            )
            statements.append(
                "INSERT INTO mini_auth_code_vault "
                "(principal_ref,credential_hash,encrypted_code) VALUES ("
                + ",".join(map(quote, (principal, credential_hash, encrypted)))
                + ") ON CONFLICT(principal_ref) DO UPDATE SET "
                "credential_hash=EXCLUDED.credential_hash,"
                "encrypted_code=EXCLUDED.encrypted_code"
            )
        statements.append("COMMIT")
        self.sql(";\n".join(statements) + ";", database)
        return len(principals)

    def restore_and_start(self, mode, binary, source_revision, migration_binary=None,
                          query_log=False):
        if mode not in ("before", "after"):
            raise RuntimeError("Mode must be before or after")
        self.stop(mode)
        database = DB_PREFIX + mode + "_" + self.metadata["tag"]
        role = self.runtime.username
        if not re.fullmatch(r"[a-zA-Z0-9_]+", role):
            raise RuntimeError("Unsupported local role name")
        exists = self.sql(
            "SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname='" + database + "')"
        )
        if exists == "t":
            self.command("dropdb", ["--force", database])
        self.command("createdb", ["--owner", role, database])
        self.command("pg_restore", [
            "--no-owner", "--no-privileges", "--role", role,
            "--dbname", database, str(self.run_dir / "source.dump"),
        ])
        credential_count = self.seed_clone_credentials(database)
        directory = self.run_dir / mode
        directory.mkdir(mode=0o700, exist_ok=True)
        data = directory / "data"
        if data.exists():
            shutil.rmtree(data)
        shutil.copytree(self.run_dir / "auxiliary", data)
        for path in data.iterdir():
            path.chmod(0o600)
        executable = directory / "mini_rs_erp"
        shutil.copy2(Path(binary).resolve(), executable)
        executable.chmod(0o700)
        port = 18091 if mode == "before" else 18092
        if migration_binary:
            result = subprocess.run(
                [str(Path(migration_binary).resolve())],
                cwd=directory, env=self.server_env(database, port, migration=True),
                stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
            )
            if result.returncode:
                raise RuntimeError("Benchmark-only migration failed; output withheld")
        log_path = directory / "server.log"
        with open(log_path, "wb") as log:
            process = subprocess.Popen(
                [str(executable)], cwd=directory,
                env=self.server_env(database, port, query_log=query_log),
                stdout=log, stderr=log, start_new_session=True,
            )
        log_path.chmod(0o600)
        server = {
            "pid": process.pid, "database": database, "port": port,
            "source_revision": source_revision,
            "binary_sha256": sha256(executable),
            "binary_bytes": executable.stat().st_size,
            "query_log": query_log, "ready": False,
            "mini_order_sync_interval_seconds": 0 if query_log else 30,
            "test_credentials_reseeded": credential_count,
        }
        self.metadata["servers"][mode] = server
        self.save()
        for _ in range(120):
            if process.poll() is not None:
                break
            try:
                with urllib.request.urlopen(
                    "http://127.0.0.1:" + str(port) + "/healthz", timeout=1
                ) as response:
                    server["ready"] = response.status == 200
                if server["ready"]:
                    break
            except OSError:
                time.sleep(0.25)
        self.save()
        if not server["ready"]:
            raise RuntimeError("Isolated server failed to start; private server.log retained")
        return server

    def admin_credentials(self, mode):
        # These values are used solely in memory against the disposable clone.
        from cryptography.hazmat.primitives.ciphers.aead import AESGCM
        database = self.metadata["servers"][mode]["database"]
        row = json.loads(self.sql("""
            SELECT json_build_object('phone', i.phone, 'hash', a.credential_hash,
                                     'envelope', v.encrypted_code)
              FROM mini_auth_accounts a
              JOIN mini_auth_builtin_identities i USING (principal_ref)
              JOIN mini_auth_code_vault v USING (principal_ref)
             WHERE a.principal_ref='admin' AND v.credential_hash=a.credential_hash;
        """, database))
        key = base64.b64decode(
            self.keychain.read(self.service, "access-code-encryption-key")
        )
        envelope = base64.b64decode(row["envelope"].removeprefix("v1:"))
        binding = ("mini-rs-erp/access-code/v1\0admin\0" + row["hash"]).encode()
        code = AESGCM(key).decrypt(envelope[:12], envelope[12:], binding).decode()
        return {"phone": row["phone"], "code": code}

    def stop(self, mode):
        server = self.metadata.get("servers", {}).get(mode)
        if not server or not server.get("ready"):
            return
        pid = server["pid"]
        found = subprocess.run(
            ["ps", "-p", str(pid), "-o", "comm="], capture_output=True, text=True
        )
        if found.returncode == 0:
            expected = str(self.run_dir / mode / "mini_rs_erp")
            if Path(found.stdout.strip()).resolve() != Path(expected).resolve():
                raise RuntimeError("Refusing to stop an unrelated process")
            os.kill(pid, signal.SIGTERM)
            for _ in range(100):
                found = subprocess.run(
                    ["ps", "-p", str(pid), "-o", "stat="],
                    capture_output=True, text=True,
                )
                if found.returncode or found.stdout.strip().startswith("Z"):
                    break
                time.sleep(0.1)
            else:
                raise RuntimeError("Benchmark server did not stop")
        server["ready"] = False
        self.save()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-dir", required=True)
    sub = parser.add_subparsers(dest="command", required=True)
    prepare = sub.add_parser("prepare")
    prepare.add_argument("--baseline-revision", required=True)
    start = sub.add_parser("start")
    start.add_argument("--mode", choices=("before", "after"), required=True)
    start.add_argument("--binary", required=True)
    start.add_argument("--source-revision", required=True)
    start.add_argument("--migration-binary")
    start.add_argument("--query-log", action="store_true")
    stop = sub.add_parser("stop")
    stop.add_argument("--mode", choices=("before", "after"), required=True)
    args = parser.parse_args()
    fixture = Fixture(args.run_dir)
    if args.command == "prepare":
        fixture.prepare(args.baseline_revision)
        print(json.dumps({
            key: fixture.metadata[key]
            for key in ("baseline_revision", "dump_sha256", "dump_bytes", "source_counts")
        }))
    elif args.command == "start":
        print(json.dumps(fixture.restore_and_start(
            args.mode, args.binary, args.source_revision, args.migration_binary,
            args.query_log,
        )))
    else:
        fixture.stop(args.mode)
        print(json.dumps({"mode": args.mode, "stopped": True}))


if __name__ == "__main__":
    main()
