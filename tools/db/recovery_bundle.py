#!/usr/bin/env python3
"""Create, verify and recover a complete Accord backend archive.

Only restore into a NEW directory and an explicitly selected PostgreSQL database.
Credentials are read from the environment, never from command-line arguments.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import sys
import tarfile
import tempfile
from urllib.parse import quote, unquote, urlsplit, urlunsplit

from recovery_state import capture_source, capture_state, copy_file, private_json
from recovery_remote import capture_remote, restore_remote
from recovery_assets import validate_media

FORMAT = "accord-full-recovery"
MANIFEST = "recovery-manifest.json"
ROOT = Path(__file__).resolve().parents[2]


def checksum(path):
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def safe_name(value):
    path = PurePosixPath(value)
    if (not value or value != path.as_posix() or path.is_absolute()
            or any(part in ("", ".", "..") for part in value.split("/"))
            or "\\" in value or "\0" in value):
        raise RuntimeError("Unsafe recovery archive member")
    return value


def create(root, runtime, dump, output, inherited):
    root, runtime, dump, output = (p.resolve() for p in (root, runtime, dump, output))
    if output.exists():
        raise RuntimeError("Recovery archive already exists")
    output.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    with tempfile.TemporaryDirectory(prefix=".recovery-", dir=output.parent) as temporary:
        stage = Path(temporary)
        bindings, env = capture_state(runtime, root, stage, inherited)
        version = capture_source(root, stage, inherited.get("MINI_ERP_BACKUP_EXECUTABLE"))
        copy_file(dump, stage / "database/database.dump")
        for name in ("backup.meta", "globals.sql", "database-roles.json", "database-info.json"):
            if (dump.parent / name).is_file():
                copy_file(dump.parent / name, stage / "database" / name)
        remote = capture_remote(env, stage)
        validate_media(dump, stage, bindings, remote, pg_tool("pg_restore"))
        files = {}
        for path in sorted(stage.rglob("*")):
            if path.is_file():
                name = path.relative_to(stage).as_posix()
                files[name] = {"sha256": checksum(path), "size": path.stat().st_size,
                               "executable": bool(path.stat().st_mode & 0o111)}
        manifest = {
            "format": FORMAT, "version": 1,
            "created_at_utc": datetime.now(timezone.utc).isoformat(),
            "database_dump": "database/database.dump",
            "environment_file": "runtime/environment.json",
            "source": version, "state": bindings, "remote": remote, "files": files,
            "consistency": "PostgreSQL snapshot; transactional SQLite/LMDB snapshots; files copied during backup window",
            "restore_tested": False,
        }
        private_json(stage / MANIFEST, manifest)
        partial = output.with_name(output.name + ".part")
        try:
            with tarfile.open(partial, "w:gz", compresslevel=3) as archive:
                for name in [MANIFEST, *sorted(files)]:
                    info = archive.gettarinfo(stage / name, arcname=name)
                    info.uid = info.gid = 0
                    info.uname = info.gname = ""
                    info.mode = 0o700 if files.get(name, {}).get("executable") else 0o600
                    with (stage / name).open("rb") as stream:
                        archive.addfile(info, stream)
            os.chmod(partial, 0o600)
            verify(partial)
            with partial.open("rb") as stream:
                os.fsync(stream.fileno())
            partial.rename(output)
            private_json(output.with_name(output.name + ".json"), {
                "format": FORMAT, "version": 1, "sha256": checksum(output),
                "recovery_ready": True, "restore_tested": False,
            })
        finally:
            partial.unlink(missing_ok=True)
    return manifest


def verify(path, destination=None):
    """Validate every byte before extraction or any restore-side mutations."""
    with tarfile.open(path, "r:gz") as archive:
        members = {}
        for member in archive:
            safe_name(member.name)
            if not member.isfile() or member.name in members:
                raise RuntimeError("Recovery archive has links, special files or duplicate members")
            members[member.name] = member
        if MANIFEST not in members or members[MANIFEST].size > 32 * 1024 * 1024:
            raise RuntimeError("Recovery manifest missing or too large")
        manifest = json.load(archive.extractfile(members[MANIFEST]))
        if manifest.get("format") != FORMAT or manifest.get("version") != 1:
            raise RuntimeError("Unsupported recovery archive format")
        files = manifest["files"]
        if set(members) != set(files) | {MANIFEST}:
            raise RuntimeError("Recovery inventory does not match archive members")
        required = {manifest["database_dump"], manifest["environment_file"],
                    "source/Cargo.toml", "source/Cargo.lock", "source/src/main.rs",
                    "source/tools/db/restore_postgres.sh",
                    "source/migrations/postgres/0104_runtime_security_boundaries.sql"}
        if not required <= set(files):
            raise RuntimeError("Recovery archive lacks database, configuration or exact source")
        for binding in manifest["state"]:
            safe_name(binding["payload"])
            if not binding["payload"].startswith("state/"):
                raise RuntimeError("Invalid recovery state path")
            if binding["exists"] and not any(name == binding["payload"] or name.startswith(binding["payload"] + "/")
                                             for name in files):
                # Empty state directories are represented by their binding.
                if not binding.get("directory", False):
                    raise RuntimeError("Recovery state payload is missing")
        for inventory in manifest["remote"]:
            for item in inventory["objects"]:
                if safe_name(item["payload"]) not in files:
                    raise RuntimeError("Recovery remote object is missing")
        for name, expected in files.items():
            safe_name(name)
            member = members[name]
            if member.size != expected["size"]:
                raise RuntimeError("Recovery member size mismatch: " + name)
            digest = hashlib.sha256()
            with archive.extractfile(member) as stream:
                for block in iter(lambda: stream.read(1024 * 1024), b""):
                    digest.update(block)
            if digest.hexdigest() != expected["sha256"]:
                raise RuntimeError("Recovery member checksum mismatch: " + name)
        if destination:
            for name, member in members.items():
                target = destination / name
                target.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
                with archive.extractfile(member) as stream, target.open("xb") as recovered:
                    shutil.copyfileobj(stream, recovered, 1024 * 1024)
                os.chmod(target, 0o700 if files.get(name, {}).get("executable") else 0o600)
    return manifest


def pg_tool(name):
    found = shutil.which(name)
    if found:
        return found
    candidates = sorted((ROOT.parent / ".tools/postgres").glob("*/bin/" + name))
    if not candidates:
        candidates = sorted(Path("/usr/lib/postgresql").glob("*/bin/" + name),
                            key=lambda path: int(path.parent.parent.name))
    if candidates:
        return str(candidates[-1])
    raise RuntimeError("Required PostgreSQL tool unavailable: " + name)


def connection_env(url, inherited):
    """Keep URL passwords out of PostgreSQL argv and command diagnostics."""
    parsed = urlsplit(url)
    if parsed.scheme not in ("postgres", "postgresql") or not parsed.hostname or not parsed.path.strip("/"):
        raise RuntimeError("A PostgreSQL connection URL is required")
    env = dict(inherited)
    if parsed.password is not None:
        env["PGPASSWORD"] = unquote(parsed.password)
    host = ("[" + parsed.hostname + "]") if ":" in parsed.hostname else parsed.hostname
    user = quote(unquote(parsed.username), safe="") + "@" if parsed.username else ""
    authority = user + host + (":" + str(parsed.port) if parsed.port else "")
    return urlunsplit((parsed.scheme, authority, parsed.path, parsed.query, "")), env


def psql(url, sql, env):
    clean, env = connection_env(url, env)
    result = subprocess.run([pg_tool("psql"), "-X", "-w", "-A", "-t",
                             "-v", "ON_ERROR_STOP=1", "--dbname", clean],
                            input=sql, text=True, env=env, capture_output=True)
    if result.returncode:
        raise RuntimeError("PostgreSQL recovery check failed; no credentials printed")
    return result.stdout.strip()


def runtime_url(original, target):
    old, new = urlsplit(original), urlsplit(target)
    host = ("[" + new.hostname + "]") if ":" in new.hostname else new.hostname
    credentials = old.netloc.rsplit("@", 1)[0] + "@" if "@" in old.netloc else ""
    return urlunsplit((new.scheme, credentials + host + (":" + str(new.port) if new.port else ""),
                       new.path, new.query, ""))


def bootstrap_runtime_role(url, env, saved, dependencies):
    # Security migration 0104 reestablishes the fixed runtime/owner roles. Never
    # replay globals.sql into a shared cluster or reset an existing login.
    password = urlsplit(saved["MINI_ERP_DATABASE_URL"]).password
    password = unquote(password) if password else saved.get("PGPASSWORD")
    role = urlsplit(saved["MINI_ERP_DATABASE_URL"]).username
    role = unquote(role) if role else "mini_rs_erp"
    for name in dict.fromkeys(("mini_rs_erp", role, *dependencies)):
        literal = "'" + name.replace("'", "''") + "'"
        identifier = '"' + name.replace('"', '""') + '"'
        exists = psql(url, "SELECT EXISTS(SELECT 1 FROM pg_roles WHERE rolname=" + literal + ");", env)
        if exists == "t":
            continue
        clause = " LOGIN" if name == role else " NOLOGIN"
        if name == role and password:
            clause += " PASSWORD '" + password.replace("'", "''") + "'"
        psql(url, "SET standard_conforming_strings=on;\nCREATE ROLE " + identifier + clause
             + " NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;", env)
        if name == role and role != "mini_rs_erp":
            psql(url, "GRANT mini_rs_erp TO " + identifier + ";", env)


def restore(archive, target, inherited, replace_database=False, service_stopped=False):
    url = inherited.get("MINI_ERP_RESTORE_DATABASE_URL")
    if not url:
        raise RuntimeError("Set MINI_ERP_RESTORE_DATABASE_URL explicitly to the recovery database")
    target = target.absolute()
    if target.exists():
        raise RuntimeError("Recovery target directory must not exist")
    if replace_database and not service_stopped:
        raise RuntimeError("Replacing a database requires --service-stopped and a stopped backend")
    manifest = verify(archive)
    count = int(psql(url, "SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace "
                    "WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%' "
                    "AND c.relkind IN ('r','p','v','m','S');", inherited))
    if count and not replace_database:
        raise RuntimeError("Recovery database is not empty; use a new database")
    target.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".accord-restore-", dir=target.parent) as temporary:
        stage = Path(temporary)
        verify(archive, stage)
        clean, pg_env = connection_env(url, inherited)
        subprocess.run([pg_tool("pg_restore"), "--exit-on-error", "--file=" + os.devnull,
                        str(stage / manifest["database_dump"])],
                       env=pg_env, check=True, capture_output=True)
        info = stage / "database/database-info.json"
        if info.is_file():
            original = json.loads(info.read_text())
            current = json.loads(psql(url, "SELECT json_build_object('encoding', pg_encoding_to_char(encoding), "
                                      "'collate',datcollate,'ctype',datctype) FROM pg_database "
                                      "WHERE datname=current_database();", inherited))
            if any(current[key] != original[key] for key in ("encoding", "collate", "ctype")):
                raise RuntimeError("Recovery database encoding/collation does not match the backup")
        recovered = stage / "source"
        env = json.loads((stage / manifest["environment_file"]).read_text())
        restore_remote(env, stage, manifest["remote"])
        state = stage / "state"
        if (state / "data").is_dir():
            shutil.move(state / "data", recovered / "data")
        if state.exists():
            shutil.move(state, recovered / "recovery-state")
        for binding in manifest["state"]:
            relative = binding["payload"].removeprefix("state/")
            relocated = target / (relative if relative.startswith("data/") else "recovery-state/" + relative)
            env[binding["key"]] = str(relocated)
            if binding.get("directory") and binding["exists"]:
                source_path = recovered / relocated.relative_to(target)
                source_path.mkdir(parents=True, exist_ok=True, mode=0o700)
        if (stage / "bin").is_dir():
            shutil.move(stage / "bin", recovered / "bin")
        env["MINI_ERP_DATABASE_URL"] = inherited.get("MINI_ERP_RECOVERY_RUNTIME_DATABASE_URL") or runtime_url(
            env["MINI_ERP_DATABASE_URL"], url)
        env["MINI_ERP_MIGRATION_DATABASE_URL"] = env["MINI_ERP_DATABASE_URL"]
        # Do not contact the original database with its old admin URL.
        env.pop("MINI_ERP_ADMIN_DATABASE_URL", None)
        private_json(recovered / "recovery-environment.json", env)
        launcher = recovered / "run-restored.sh"
        launcher.write_text('#!/usr/bin/env bash\nset -euo pipefail\ncd -- "$(dirname -- "$0")"\n'
                            'exec python3 tools/db/recovery_bundle.py run --root .\n')
        os.chmod(launcher, 0o700)
        private_json(recovered / "recovery-manifest.json", manifest)
        roles_file = stage / "database/database-roles.json"
        dependencies = json.loads(roles_file.read_text()) if roles_file.exists() else []
        bootstrap_runtime_role(url, inherited, env, dependencies)
        pg_env.update(MINI_ERP_DATABASE_KEYCHAIN_SERVICE="", MINI_ERP_AUTO_MIGRATE_AFTER_RESTORE="0",
                      MINI_ERP_RESTORE_DATABASE_URL=clean,
                      MINI_ERP_RESTORE_DUMP=str(stage / manifest["database_dump"]))
        result = subprocess.run(["bash", str(recovered / "tools/db/restore_postgres.sh")],
                                env=pg_env, capture_output=True)
        if result.returncode:
            raise RuntimeError("Database restore failed atomically; recovery target was not installed")
        # Source, state, key and launcher are complete before committing their
        # final directory. Restore never modifies the original application's files.
        recovered.rename(target)
    return manifest


def run(root):
    env = dict(os.environ)
    env.update(json.loads((root / "recovery-environment.json").read_text()))
    binary = root / "bin/mini_rs_erp"
    if not binary.is_file():
        binary = root / "target/release/mini_rs_erp"
    if not binary.is_file():
        raise RuntimeError("Build the packaged source first: cargo build --release --locked --bin mini_rs_erp")
    os.chdir(root)
    os.execve(binary, [str(binary)], env)


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    create_parser = sub.add_parser("create")
    create_parser.add_argument("--root", type=Path, default=ROOT)
    create_parser.add_argument("--runtime", type=Path, default=Path.cwd())
    create_parser.add_argument("--dump", type=Path, required=True)
    create_parser.add_argument("--output", type=Path, required=True)
    verify_parser = sub.add_parser("verify")
    verify_parser.add_argument("archive", type=Path)
    restore_parser = sub.add_parser("restore")
    restore_parser.add_argument("archive", type=Path)
    restore_parser.add_argument("--target", type=Path, required=True)
    restore_parser.add_argument("--replace-database", action="store_true")
    restore_parser.add_argument("--service-stopped", action="store_true")
    run_parser = sub.add_parser("run")
    run_parser.add_argument("--root", type=Path, default=Path.cwd())
    args = parser.parse_args()
    try:
        if args.command == "create":
            create(args.root, args.runtime, args.dump, args.output, dict(os.environ))
            print("Complete recovery archive created and checksummed")
        elif args.command == "verify":
            verify(args.archive)
            print("Recovery archive: all members and SHA-256 checksums verified")
        elif args.command == "restore":
            restore(args.archive, args.target, dict(os.environ), args.replace_database, args.service_stopped)
            print("Recovery installed: " + str(args.target))
        else:
            run(args.root.resolve())
    except (OSError, ValueError, KeyError, TypeError, AttributeError, RuntimeError,
            tarfile.TarError, subprocess.SubprocessError) as error:
        # urllib/subprocess errors can contain signed URLs or secrets. Keep those
        # diagnostics out of server logs and the administrator's API response.
        message = str(error) if type(error) is RuntimeError else type(error).__name__
        print("Recovery failed: " + message, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
