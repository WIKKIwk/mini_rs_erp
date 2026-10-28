"""Run SQL guards and a two-connection race on an explicitly disposable PG server.

MINI_ERP_TEST_ADMIN_DATABASE_URL=... python3 tests/qolip_storage_guards_concurrency.py
Creates/drops only its own randomly named database. Requires psql on PATH.
"""
import os
from pathlib import Path
import subprocess
from urllib.parse import urlsplit, urlunsplit
import uuid


def run(url, sql):
    return subprocess.run(
        ["psql", "-X", "-qAt", "-v", "ON_ERROR_STOP=1", url],
        input=sql, text=True, capture_output=True, check=True,
    ).stdout


def main():
    admin = os.environ["MINI_ERP_TEST_ADMIN_DATABASE_URL"]
    name = "qolip_storage_guards_" + uuid.uuid4().hex
    parts = urlsplit(admin)
    url = urlunsplit(parts._replace(path="/" + name))
    root = Path(__file__).resolve().parent.parent
    run(admin, f'CREATE DATABASE "{name}";')
    writer = None
    try:
        fixture = (root / "tests/qolip_storage_guards.sql").read_text()
        fixture = fixture.replace("\\ir ../migrations/", "\\ir " + str(root / "migrations") + "/")
        fixture = fixture.replace("ROLLBACK;", "COMMIT;")
        run(url, fixture)
        run(url, "SET ROLE mini_rs_erp; INSERT INTO mini_qolip_product_specs VALUES ('RACE','alternative',420);")
        writer = subprocess.Popen(
            ["psql", "-X", "-qAt", "-v", "ON_ERROR_STOP=1", url],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
        writer.stdin.write("""
            SET ROLE mini_rs_erp; BEGIN;
            SELECT pg_advisory_xact_lock(hashtextextended('qolip:race',0));
            INSERT INTO mini_qolip_archive_entries
                (id,kind,code,warehouse,snapshot,archived_by_role,archived_by_ref,archived_by_name)
            VALUES ('race-visit','qolip','RACE','Mold warehouse','{}','qolipchi','worker','Worker');
            SELECT 'ready'; SELECT pg_sleep(2); COMMIT;
        """)
        writer.stdin.close()
        # Synchronize at the lock, rather than guessing which process starts first.
        while True:
            line = writer.stdout.readline()
            if line.strip() == "ready":
                break
            if not line:
                raise AssertionError("Archive writer stopped before acquiring the mold lock")
        try:
            run(url, "SET ROLE mini_rs_erp; INSERT INTO mini_qolip_checkouts VALUES ('RACE','open');")
        except subprocess.CalledProcessError as error:
            assert "qolip_archived" in error.stderr, error.stderr
        else:
            raise AssertionError("A checkout waiting on archive commit must be rejected")
        assert writer.wait(timeout=15) == 0, writer.stderr.read()
        assert run(url, "SELECT count(*) FROM mini_qolip_checkouts WHERE qolip_code='RACE';").strip() == "0"
        print("Qolip SQL guards and archive-vs-checkout concurrency passed")
    finally:
        if writer is not None and writer.poll() is None:
            writer.terminate()
            writer.wait(timeout=15)
        run(admin, f'DROP DATABASE "{name}" WITH (FORCE);')


if __name__ == "__main__":
    main()
