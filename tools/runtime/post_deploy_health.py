#!/usr/bin/env python3
"""Verify local ERP readiness and the public domain immediately after deployment."""

import argparse
from concurrent.futures import ThreadPoolExecutor
import json
import os
import subprocess
import sys
import time
from urllib.parse import urlsplit


def origin(raw, public=False):
    raw = raw.strip()
    if public and "://" not in raw:
        raw = "https://" + raw
    parsed = urlsplit(raw)
    if (
        parsed.scheme not in ({"https"} if public else {"http", "https"})
        or not parsed.hostname
        or parsed.username is not None
        or parsed.password is not None
        or parsed.path not in ("", "/")
        or parsed.query
        or parsed.fragment
        or parsed.port == 0
    ):
        raise ValueError("health checks require an origin without credentials or a path")
    return raw.rstrip("/")


def probe_endpoint(url, timeout):
    try:
        response = subprocess.run(
            [
                "curl", "--disable", "--silent", "--show-error", "--noproxy", "*",
                "--connect-timeout", str(min(3, timeout)),
                "--max-time", str(timeout), "--max-filesize", "4096",
                "--write-out", "\n%{http_code}", url,
            ],
            capture_output=True,
            timeout=timeout + 1,
            check=False,
        )
        if response.returncode:
            return "request_failed"
        body, status = response.stdout.rsplit(b"\n", 1)
        if status != b"200":
            return "http_" + status.decode("ascii", errors="replace")
        if len(body) > 4096:
            return "unexpected_response"
        payload = json.loads(body)
        if not isinstance(payload, dict) or payload.get("ok") is not True:
            return "not_ready"
        return None
    except (OSError, subprocess.TimeoutExpired, ValueError):
        return "request_failed"


def check_deployment(local_url, public_url, wait_seconds=90, *,
                     probe=probe_endpoint, now=time.monotonic, sleep=time.sleep):
    targets = [
        (label + path, base + path)
        for label, base in (("local", local_url), ("public", public_url))
        for path in ("/healthz", "/readyz")
    ]
    deadline = now() + wait_seconds
    previous_failures = None
    # The first pass begins immediately, including for an already-running ERP.
    with ThreadPoolExecutor(max_workers=len(targets)) as pool:
        while True:
            request_timeout = min(5, max(0.1, deadline - now()))
            futures = [pool.submit(probe, url, request_timeout) for _, url in targets]
            failures = [
                label + ": " + error
                for (label, _), future in zip(targets, futures)
                if (error := future.result()) is not None
            ]
            if not failures:
                print("post-deploy health verified: local and public /healthz + /readyz")
                return True
            if failures != previous_failures:
                print("post-deploy health pending: " + ", ".join(failures), file=sys.stderr)
                previous_failures = failures
            remaining = deadline - now()
            if remaining <= 0:
                print("post-deploy health failed", file=sys.stderr)
                return False
            sleep(min(2, remaining))


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--local-url", default=os.environ.get(
        "MINI_ERP_DOCTOR_DOMAIN_LOCAL_URL", os.environ.get("CORE_URL", "http://127.0.0.1:18081")))
    parser.add_argument("--public-url", default=os.environ.get(
        "MINI_ERP_DOCTOR_DOMAIN_URL", os.environ.get("PUBLIC_HOSTNAME", "")))
    parser.add_argument("--wait-seconds", type=float, default=90)
    args = parser.parse_args(argv)
    if not 0 <= args.wait_seconds <= 300:
        parser.error("--wait-seconds must be between 0 and 300")
    try:
        local_url = origin(args.local_url)
        public_url = origin(args.public_url, public=True)
    except ValueError as error:
        parser.error(str(error))
    return 0 if check_deployment(local_url, public_url, args.wait_seconds) else 1


if __name__ == "__main__":
    sys.exit(main())
