"""Inventory and transactionally snapshot ERP state, including external paths."""
from __future__ import annotations

import base64
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import shlex
import shutil
import sqlite3
import subprocess
import sys
import time

PATHS = {
    "MOBILE_API_SESSION_STORE_PATH": "data/mobile_sessions.json",
    "MOBILE_API_PROFILE_STORE_PATH": "data/mobile_profile_prefs.json",
    "MOBILE_API_PUSH_TOKEN_STORE_PATH": "data/mobile_push_tokens.json",
    "MOBILE_API_ADMIN_SUPPLIER_STORE_PATH": "data/mobile_admin_suppliers.json",
    "MOBILE_API_ADMIN_STORE_PATH": "data/mobile_admin_store.json",
    "MOBILE_API_ROLE_STORE_PATH": "data/mobile_roles.json",
    "MOBILE_API_TELEGRAM_STORE_PATH": "data/mobile_telegram.json",
    "MOBILE_API_APPARATUS_GROUP_STORE_PATH": "data/mobile_apparatus_groups.sqlite",
    "MOBILE_API_CALCULATE_ORDER_STORE_PATH": "data/mobile_calculate_orders.sqlite",
    "MOBILE_API_CALCULATE_MATERIAL_STORE_PATH": "data/mobile_calculate_materials.sqlite",
    "MOBILE_API_CALCULATE_ORDER_IMAGE_DIR": "data/mobile_calculate_order_images",
    "MOBILE_PROFILE_AVATAR_STORE_DIR": "data/profile_avatars",
    "MOBILE_CHAT_MEDIA_STORE_DIR": "data/chat_media",
    "MOBILE_APP_RELEASE_DIR": "data/mobile_releases",
    "FCM_CONFIG_PATH": "data/fcm-config.enc",
    "MINI_ERP_ACCESS_CODE_KEY_FILE": "data/access-code.key",
    "IROH_SECRET_KEY_FILE": "garbage/iroh/endpoint.key",
}
for _name in ("SESSION", "PROFILE", "PUSH_TOKEN", "ADMIN_SUPPLIER", "RPS_BATCH"):
    _legacy = {
        "SESSION": PATHS["MOBILE_API_SESSION_STORE_PATH"],
        "PROFILE": PATHS["MOBILE_API_PROFILE_STORE_PATH"],
        "PUSH_TOKEN": PATHS["MOBILE_API_PUSH_TOKEN_STORE_PATH"],
        "ADMIN_SUPPLIER": PATHS["MOBILE_API_ADMIN_SUPPLIER_STORE_PATH"],
        "RPS_BATCH": "data/mobile_rps_batches.json",
    }[_name]
    PATHS[f"MOBILE_API_{_name}_LMDB_PATH"] = str(Path(_legacy).with_suffix(".lmdb"))

PREFIXES = ("MINI_ERP_", "MINI_RS_", "IROH_", "MOBILE_", "TELEGRAM_", "FCM_", "FIREBASE_",
            "GOOGLE_", "GEMINI_", "RP_", "RPS_")
EXTRA_ENV = {"PGPASSWORD", "PGSSLROOTCERT", "PGSSLCERT", "PGSSLKEY", "RUST_LOG",
             "ADMINKA_NAME", "MINI_ORDER_SYNC_INTERVAL_SECONDS"}
SKIP_DIRS = {".git", ".dart_tool", ".codegraph", "__pycache__", "target", "node_modules"}


def private_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    with path.open("x", encoding="utf-8") as stream:
        os.chmod(path, 0o600)
        json.dump(value, stream, ensure_ascii=False, indent=2)
        stream.flush()
        os.fsync(stream.fileno())


def environment(root, inherited, helper=None):
    """Read dotenv without executing shell code; inherited settings win."""
    values = {}
    dotenv = root / ".env"
    if helper:
        result = subprocess.run([str(helper), "--environment", str(root)], env=inherited,
                                check=True, capture_output=True, text=True)
        values = json.loads(result.stdout)
    elif dotenv.exists():
        pending = ""
        for line in dotenv.read_text().splitlines():
            pending += ("\n" if pending else "") + line
            try:
                tokens = shlex.split(pending, comments=True)
            except ValueError:
                continue
            pending = ""
            if not tokens:
                continue
            if tokens[0] == "export":
                tokens.pop(0)
            key, sep, value = " ".join(tokens).partition("=")
            if sep:
                values[key.strip()] = value.strip()
        if pending:
            raise RuntimeError("Cannot parse runtime .env; backup stopped")
    values.update(inherited)
    values = {k: v for k, v in values.items()
              if k.startswith(PREFIXES) or k in EXTRA_ENV}
    # The runtime launcher intentionally hides the maintenance password from
    # HTTP. Only the app's own Keychain secrets belong in its recovery copy.
    service = values.get("MINI_ERP_DATABASE_KEYCHAIN_SERVICE")
    if service and sys.platform == "darwin":
        from macos_keychain import Keychain, local_url
        from urllib.parse import unquote
        keychain = Keychain()
        runtime = values.get("MINI_ERP_KEYCHAIN_RUNTIME_URL",
                             values.get("MINI_ERP_DATABASE_URL", ""))
        url = local_url(runtime)
        values["MINI_ERP_DATABASE_URL"] = runtime
        values["PGPASSWORD"] = keychain.read(service, unquote(url.username))
        values["MINI_ERP_ACCESS_CODE_KEY"] = keychain.read(service, "access-code-encryption-key")
    return values


def configured_paths(env):
    paths = dict(PATHS)
    if "MOBILE_API_SESSION_STORE_PATH" not in env and env.get("MOBILE_API_SESSION_STORE"):
        paths["MOBILE_API_SESSION_STORE_PATH"] = env["MOBILE_API_SESSION_STORE"]
    for name in ("SESSION", "PROFILE", "PUSH_TOKEN", "ADMIN_SUPPLIER"):
        key = f"MOBILE_API_{name}_STORE_PATH"
        paths[f"MOBILE_API_{name}_LMDB_PATH"] = str(Path(env.get(key, paths[key])).with_suffix(".lmdb"))
    paths.update({key: value for key, value in env.items() if key in paths and value})
    paths["TELEGRAM_KEY_FILE"] = str(Path(paths["MOBILE_API_TELEGRAM_STORE_PATH"]).with_suffix(".key"))
    paths["FCM_CLIENTS_FILE"] = str(Path(paths["FCM_CONFIG_PATH"]).with_suffix(".clients.json"))
    for key in ("FCM_SERVICE_ACCOUNT_PATH", "GOOGLE_SHEETS_SERVICE_ACCOUNT_PATH",
                "GOOGLE_SERVICE_ACCOUNT_PATH", "PGSSLROOTCERT", "PGSSLCERT", "PGSSLKEY",
                "IROH_TICKET_FILE", "MINI_RS_GATEWAY_TLS_CERT_PATH", "MINI_RS_GATEWAY_TLS_KEY_PATH"):
        if env.get(key):
            paths[key] = env[key]
    for key, fallback in (("MOBILE_CHAT_MEDIA_FFMPEG_BIN", "ffmpeg"),
                          ("MOBILE_CHAT_MEDIA_FFPROBE_BIN", "ffprobe")):
        binary = shutil.which(env.get(key, fallback))
        if binary:
            paths[key] = binary
    return paths


def copy_file(source, target):
    target.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    for _ in range(3):
        with source.open("rb") as stream:
            before = os.fstat(stream.fileno())
            magic = stream.read(16)
        if magic == b"SQLite format 3\0":
            deadline = time.monotonic() + 60
            def progress(*_):
                if time.monotonic() > deadline:
                    raise RuntimeError("SQLite snapshot timed out")
            with closing(sqlite3.connect(source.as_uri() + "?mode=ro", uri=True)) as original:
                with closing(sqlite3.connect(target)) as recovered:
                    original.backup(recovered, pages=256, progress=progress)
                    recovered.execute("PRAGMA journal_mode=DELETE").fetchone()
                    if recovered.execute("PRAGMA integrity_check").fetchall() != [("ok",)]:
                        raise RuntimeError("SQLite recovery snapshot failed integrity_check")
            os.chmod(target, 0o600)
            return
        with source.open("rb") as original, target.open("wb") as recovered:
            before = os.fstat(original.fileno())
            shutil.copyfileobj(original, recovered, 1024 * 1024)
            recovered.flush()
            os.fsync(recovered.fileno())
            after = os.fstat(original.fileno())
        os.chmod(target, 0o700 if before.st_mode & 0o111 else 0o600)
        if (before.st_size, before.st_mtime_ns) == (after.st_size, after.st_mtime_ns):
            return
    raise RuntimeError("State file changed repeatedly during backup: " + source.name)


def snapshot_tree(source, target, helper, ancestors=()):
    resolved = source.resolve(strict=True)
    if resolved in ancestors:
        raise RuntimeError("Recursive state symlink: " + source.name)
    if source.is_dir():
        target.mkdir(parents=True, exist_ok=True, mode=0o700)
        if (source / "data.mdb").is_file():
            if not helper:
                raise RuntimeError("Build mini_rs_backup_state before backup: cargo build --release --bin mini_rs_backup_state")
            subprocess.run([str(helper), str(resolved), str(target / "data.mdb")],
                           check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
            os.chmod(target / "data.mdb", 0o600)
            return
        names = {item.name for item in source.iterdir()}
        for name in sorted(names):
            if name in {".DS_Store", "__pycache__", "lock.mdb"}:
                continue
            if name.endswith(("-wal", "-shm", "-journal")):
                base = source / name.rsplit("-", 1)[0]
                if base.is_file():
                    with base.open("rb") as stream:
                        if stream.read(16) == b"SQLite format 3\0":
                            continue  # sqlite3.backup includes committed WAL data.
            snapshot_tree(source / name, target / name, helper, ancestors + (resolved,))
    elif source.is_file():
        copy_file(resolved, target)
        if source.suffix == ".json":
            try:
                json.loads(target.read_text())
            except (ValueError, UnicodeError) as error:
                raise RuntimeError("Invalid runtime JSON state: " + source.name) from error
    else:
        raise RuntimeError("Unsupported state file: " + source.name)


def capture_state(runtime, root, stage, inherited):
    helper_candidates = [
        Path(inherited.get("MINI_ERP_BACKUP_STATE_HELPER", "/nonexistent")),
        Path(inherited.get("MINI_ERP_BACKUP_EXECUTABLE", "/nonexistent")).parent / "mini_rs_backup_state",
        root / "target/release/mini_rs_backup_state",
        root / "target/debug/mini_rs_backup_state",
    ]
    helper = next((p.resolve() for p in helper_candidates if p.is_file() and os.access(p, os.X_OK)), None)
    if helper:
        copy_file(helper, stage / "bin/mini_rs_backup_state")
    env = environment(runtime, inherited, helper)
    paths = configured_paths(env)
    for key in ("FCM_SERVICE_ACCOUNT_PATH", "GOOGLE_SHEETS_SERVICE_ACCOUNT_PATH",
                "GOOGLE_SERVICE_ACCOUNT_PATH", "PGSSLROOTCERT", "PGSSLCERT", "PGSSLKEY",
                "MINI_RS_GATEWAY_TLS_CERT_PATH", "MINI_RS_GATEWAY_TLS_KEY_PATH"):
        if env.get(key) and not (runtime / env[key]).is_file():
            raise RuntimeError("Configured credential file is missing: " + key)
    telegram = runtime / paths["MOBILE_API_TELEGRAM_STORE_PATH"]
    if telegram.is_file() and not env.get("TELEGRAM_USER_SESSION_KEY"):
        settings = json.loads(telegram.read_text())
        if (settings.get("user_sessions") or settings.get("user_api_hash")) and not (
                runtime / paths["TELEGRAM_KEY_FILE"]).is_file() and not env.get("TELEGRAM_API_HASH"):
            raise RuntimeError("Telegram encrypted state has no recoverable session key")
    data = (runtime / "data").resolve()
    roots = [(data, "state/data")]
    bindings = []
    for key, value in paths.items():
        path = (runtime / value).resolve()
        parent = next(((src, dst) for src, dst in roots if path == src or src in path.parents), None)
        if parent:
            payload = (Path(parent[1]) / path.relative_to(parent[0])).as_posix()
        else:
            # Related files must remain siblings after relocation:
            # telegram.json -> telegram.key; fcm.enc -> fcm.clients.json.
            group = hashlib.sha256(str(path.parent).encode()).hexdigest()[:20]
            payload = f"state/external/{group}/{path.name}"
            roots.append((path, payload))
        bindings.append({"key": key, "payload": payload, "exists": path.exists(),
                         "directory": path.is_dir()})
    for name in sorted(runtime.glob("*firebase-adminsdk*.json")) + [runtime / "service-account.json"]:
        if name.is_file() and all(name.resolve() != src for src, _ in roots):
            roots.append((name.resolve(), f"state/credentials/{name.name}"))
            # Make auto-discovered service accounts discoverable after relocation.
            for key in ("FCM_SERVICE_ACCOUNT_PATH", "GOOGLE_SERVICE_ACCOUNT_PATH"):
                if key not in env:
                    bindings.append({"key": key, "payload": roots[-1][1], "exists": True})
    for source, payload in roots:
        if source.exists():
            if source == stage or source in stage.parents:
                raise RuntimeError("Backup staging directory overlaps runtime state")
            snapshot_tree(source, stage / payload, helper)
    encoded = env.get("MINI_ERP_ACCESS_CODE_KEY")
    if encoded is None:
        key_path = runtime / paths["MINI_ERP_ACCESS_CODE_KEY_FILE"]
        if not key_path.is_file():
            raise RuntimeError("Access-code/FCM encryption key is missing; recovery backup cannot be complete")
        encoded = key_path.read_text().strip()
    if len(base64.b64decode(encoded.strip(), validate=True)) != 32:
        raise RuntimeError("Invalid access-code/FCM encryption key")
    env["MINI_ERP_ACCESS_CODE_KEY"] = encoded
    # Recovered startup must not require the original machine's Keychain.
    env = {k: v for k, v in env.items()
           if not k.startswith(("MINI_ERP_BACKUP_", "MINI_ERP_RESTORE_", "MINI_ERP_KEYCHAIN_"))
           and k != "MINI_ERP_DATABASE_KEYCHAIN_SERVICE"}
    private_json(stage / "runtime/environment.json", env)
    if (runtime / ".env").is_file():
        copy_file(runtime / ".env", stage / "runtime/original.env")
    return bindings, env


def capture_source(root, stage, executable=None):
    destination = stage / "source"
    names = ["Cargo.toml", "Cargo.lock", "LICENSE", "README.md", "Makefile", ".env.example",
             "src", "crates", "migrations", "tools", "docs", "examples", "build.rs", ".cargo"]
    for name in names:
        source = root / name
        if source.is_dir():
            shutil.copytree(source, destination / name, ignore=shutil.ignore_patterns(*SKIP_DIRS, ".DS_Store", "*.pyc"))
        elif source.is_file():
            copy_file(source, destination / name)
    if not (destination / "Cargo.lock").is_file() or not (destination / "src/main.rs").is_file():
        raise RuntimeError("Exact backend source and Cargo.lock must be available for recovery")
    binary = Path(executable) if executable else root / "target/release/mini_rs_erp"
    if binary.is_file():
        copy_file(binary, stage / "bin/mini_rs_erp")
        for name in ("mini_rs_migrate", "mini_rs_backup_state"):
            if (binary.parent / name).is_file():
                copy_file(binary.parent / name, stage / "bin" / name)
    revision = subprocess.run(["git", "-C", str(root), "rev-parse", "HEAD"],
                              capture_output=True, text=True)
    status = subprocess.run(["git", "-C", str(root), "status", "--porcelain"],
                            capture_output=True, text=True)
    return {"git_commit": revision.stdout.strip() if revision.returncode == 0 else None,
            "git_dirty": bool(status.stdout.strip()) if status.returncode == 0 else None,
            "platform": sys.platform, "machine": os.uname().machine,
            "binary_included": (stage / "bin/mini_rs_erp").is_file()}
