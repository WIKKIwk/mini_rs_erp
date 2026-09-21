#!/usr/bin/env python3
"""Launch local ERP/PostgreSQL tools with passwords from macOS Keychain.

No passwords are printed, written to .env, or passed as command arguments.
The HTTP service receives only the runtime credential. Maintenance access is
explicit and short lived. See docs/macos-postgres-access.md.
"""

import argparse
import ctypes
import ctypes.util
import os
from pathlib import Path
import shlex
import sys
from urllib.parse import quote, unquote, urlsplit, urlunsplit


ROOT = Path(__file__).resolve().parents[2]
KEYS = (
    "MINI_ERP_DATABASE_URL",
    "MINI_ERP_MIGRATION_DATABASE_URL",
    "MINI_ERP_DATABASE_KEYCHAIN_SERVICE",
)


class Keychain:
    def __init__(self):
        if sys.platform != "darwin":
            raise RuntimeError("Keychain access requires macOS")
        self.security = ctypes.CDLL(ctypes.util.find_library("Security"))
        self.core = ctypes.CDLL(ctypes.util.find_library("CoreFoundation"))
        void = ctypes.c_void_p
        u32 = ctypes.c_uint32
        bindings = {
            "SecKeychainSetUserInteractionAllowed": ([ctypes.c_bool], ctypes.c_int32),
            "SecKeychainFindGenericPassword": (
                [void, u32, ctypes.c_char_p, u32, ctypes.c_char_p,
                 ctypes.POINTER(u32), ctypes.POINTER(void), ctypes.POINTER(void)], ctypes.c_int32),
            "SecKeychainAddGenericPassword": (
                [void, u32, ctypes.c_char_p, u32, ctypes.c_char_p,
                 u32, void, ctypes.POINTER(void)], ctypes.c_int32),
            "SecKeychainItemFreeContent": ([void, void], ctypes.c_int32),
        }
        for name, (arguments, result) in bindings.items():
            fn = getattr(self.security, name)
            fn.argtypes, fn.restype = arguments, result
        self.core.CFRelease.argtypes = [void]
        self.core.CFRelease.restype = None
        # Background startup must fail clearly, not hang on an invisible prompt.
        self.security.SecKeychainSetUserInteractionAllowed(False)

    @staticmethod
    def check(status):
        if status != 0:
            raise RuntimeError(
                "Keychain access failed (OSStatus %s). Unlock the login Keychain "
                "and check the configured service/account." % status
            )

    def read(self, service, account):
        service, account = service.encode(), account.encode()
        length, data = ctypes.c_uint32(), ctypes.c_void_p()
        status = self.security.SecKeychainFindGenericPassword(
            None, len(service), service, len(account), account,
            ctypes.byref(length), ctypes.byref(data), None)
        self.check(status)
        try:
            password = ctypes.string_at(data, length.value).decode("utf-8")
            if not password:
                raise RuntimeError("Keychain password is empty")
            return password
        finally:
            self.security.SecKeychainItemFreeContent(None, data)

    def add(self, service, account, password):
        """Provision in memory; deliberately refuse to overwrite an existing item."""
        service, account, password = service.encode(), account.encode(), password.encode()
        item = ctypes.c_void_p()
        self.check(self.security.SecKeychainAddGenericPassword(
            None, len(service), service, len(account), account,
            len(password), password, ctypes.byref(item)))
        if item:
            self.core.CFRelease(item)


def settings():
    values = {}
    env_file = ROOT / ".env"
    if env_file.exists():
        for line in env_file.read_text().splitlines():
            key, separator, value = line.strip().removeprefix("export ").partition("=")
            key = key.strip()
            if separator and key in KEYS:
                tokens = shlex.split(value, comments=True)
                if len(tokens) > 1:
                    raise RuntimeError("Invalid local database configuration: " + key)
                values[key] = tokens[0] if tokens else ""
    values.update({key: os.environ[key] for key in KEYS if key in os.environ})
    # Nested maintenance tools inherit password-free metadata, while the HTTP
    # process's ordinary migration URL deliberately points at its runtime role.
    for purpose, key in (("RUNTIME", "MINI_ERP_DATABASE_URL"),
                         ("MAINTENANCE", "MINI_ERP_MIGRATION_DATABASE_URL")):
        inherited = os.environ.get("MINI_ERP_KEYCHAIN_" + purpose + "_URL")
        if inherited:
            values[key] = inherited
    return values


def local_url(value):
    url = urlsplit(value)
    if (url.scheme not in ("postgres", "postgresql")
            or url.hostname not in ("127.0.0.1", "localhost", "::1")
            or not url.username or not url.path.strip("/") or url.password is not None
            or url.query or url.fragment):
        raise RuntimeError("Keychain mode requires a password-free local PostgreSQL URL")
    # Evaluate the port now so malformed configuration fails before secrets are read.
    _ = url.port
    return url


def password_url(url, password):
    host = "[::1]" if url.hostname == "::1" else url.hostname
    authority = "%s:%s@%s:%s" % (
        quote(unquote(url.username), safe=""), quote(password, safe=""), host, url.port or 5432)
    return urlunsplit((url.scheme, authority, url.path, "", ""))


def launch(mode, command, admin=False, if_configured=False):
    config = settings()
    service = config.get("MINI_ERP_DATABASE_KEYCHAIN_SERVICE", "").strip()
    if not service:
        if if_configured and mode == "run":
            os.execvpe(command[0], command, os.environ.copy())
        raise RuntimeError("MINI_ERP_DATABASE_KEYCHAIN_SERVICE is not configured")
    runtime = local_url(config.get("MINI_ERP_DATABASE_URL", ""))
    maintenance = local_url(config.get("MINI_ERP_MIGRATION_DATABASE_URL", ""))
    if runtime.username == maintenance.username:
        raise RuntimeError("Runtime and maintenance must use different database accounts")
    keychain = Keychain()
    env = os.environ.copy()
    for key in ("PGPASSWORD", "PGPASSFILE", "PGSERVICE", "PGSERVICEFILE",
                "MINI_ERP_ADMIN_DATABASE_URL", "MINI_ERP_KEYCHAIN_MAINTENANCE_ACTIVE"):
        env.pop(key, None)
    env["MINI_ERP_DATABASE_KEYCHAIN_SERVICE"] = service
    env["MINI_ERP_KEYCHAIN_RUNTIME_URL"] = runtime.geturl()
    env["MINI_ERP_KEYCHAIN_MAINTENANCE_URL"] = maintenance.geturl()
    if mode in ("run", "migrate"):
        env["MINI_ERP_ACCESS_CODE_KEY"] = keychain.read(service, "access-code-encryption-key")
        runtime_password = keychain.read(service, unquote(runtime.username))
        # SQLx and libpq both support PGPASSWORD. Keep backup command argv and
        # diagnostic database URLs password-free in the long-running service.
        env["PGPASSWORD"] = runtime_password
        env["MINI_ERP_DATABASE_URL"] = runtime.geturl()
        # Override .env's maintenance URL even when no migration is pending.
        env["MINI_ERP_MIGRATION_DATABASE_URL"] = env["MINI_ERP_DATABASE_URL"]
        if mode == "migrate":
            password = keychain.read(service, unquote(maintenance.username))
            env["MINI_ERP_MIGRATION_DATABASE_URL"] = password_url(maintenance, password)
    else:
        url = maintenance if admin else runtime
        env.update(PGHOST=url.hostname, PGPORT=str(url.port or 5432),
                   PGDATABASE=unquote(url.path[1:]), PGUSER=unquote(url.username),
                   PGPASSWORD=keychain.read(service, unquote(url.username)),
                   PGCONNECT_TIMEOUT="5", PGPASSFILE="/dev/null")
        # Existing backup scripts pass this URL in argv: keep it password-free.
        env["MINI_ERP_DATABASE_URL"] = url.geturl()
        env["MINI_ERP_MIGRATION_DATABASE_URL"] = url.geturl()
        if admin:
            env["MINI_ERP_ADMIN_DATABASE_URL"] = url.geturl()
            env["MINI_ERP_RESTORE_DATABASE_URL"] = url.geturl()
            env["MINI_ERP_KEYCHAIN_MAINTENANCE_ACTIVE"] = "1"
        candidates = sorted((ROOT.parent / ".tools/postgres").glob("*/bin"))
        if len(candidates) == 1:
            env["PATH"] = str(candidates[0]) + os.pathsep + env.get("PATH", "")
    os.execvpe(command[0], command, env)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--admin", action="store_true", help="Use maintenance account with db mode")
    parser.add_argument("--if-configured", action="store_true", help="Run unchanged when not configured")
    parser.add_argument("mode", choices=("run", "migrate", "db"))
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.admin and args.mode != "db":
        parser.error("--admin is only valid with db mode")
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        if args.mode == "migrate":
            command = [str(ROOT / "target/release/mini_rs_migrate")]
        elif args.mode == "run":
            command = [str(ROOT / "target/release/mini_rs_erp")]
        else:
            command = ["psql", "-X", "-w"]
    try:
        launch(args.mode, command, args.admin, args.if_configured)
    except (RuntimeError, OSError, ValueError):
        # Never include URLs, subprocess environments, or password material.
        print("Local database launch failed. Check the login Keychain, database URL "
              "metadata and executable; see docs/macos-postgres-access.md.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
