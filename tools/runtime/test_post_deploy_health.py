import contextlib
import io
import subprocess
import unittest
from unittest.mock import patch

import post_deploy_health as health


LOCAL = "http://127.0.0.1:18081"
PUBLIC = "https://erp.example.test"


class PostDeployHealthTests(unittest.TestCase):
    def test_only_http_200_with_true_health_json_counts_as_ready(self):
        responses = [
            (b'{"ok":true}\n200', None),
            (b'{"ok":false}\n200', "not_ready"),
            (b'{"ok":1}\n200', "not_ready"),
            (b'[]\n200', "not_ready"),
            (b'<html>login</html>\n200', "request_failed"),
            (b'{"ok":true}\n302', "http_302"),
            (b'{"ok":false}\n503', "http_503"),
        ]
        for output, expected in responses:
            with self.subTest(output=output), patch.object(
                health.subprocess, "run",
                return_value=subprocess.CompletedProcess([], 0, output, b""),
            ):
                self.assertEqual(health.probe_endpoint(PUBLIC + "/readyz", 5), expected)

    def test_timeout_is_a_failed_check(self):
        with patch.object(health.subprocess, "run", side_effect=subprocess.TimeoutExpired([], 5)):
            self.assertEqual(health.probe_endpoint(PUBLIC + "/healthz", 5), "request_failed")

    def test_first_pass_checks_all_four_endpoints_without_waiting(self):
        checked = []

        def probe(url, timeout):
            checked.append(url)
            return None

        with contextlib.redirect_stdout(io.StringIO()):
            ready = health.check_deployment(
                LOCAL, PUBLIC, probe=probe,
                sleep=lambda _: self.fail("healthy deployment must not wait"),
            )
        self.assertTrue(ready)
        self.assertCountEqual(checked, [
            LOCAL + "/healthz", LOCAL + "/readyz",
            PUBLIC + "/healthz", PUBLIC + "/readyz",
        ])

    def test_public_liveness_does_not_hide_public_readiness_failure(self):
        errors = io.StringIO()
        with contextlib.redirect_stderr(errors):
            ready = health.check_deployment(
                LOCAL, PUBLIC, wait_seconds=0,
                probe=lambda url, _: "http_503" if url == PUBLIC + "/readyz" else None,
            )
        self.assertFalse(ready)
        self.assertIn("public/readyz: http_503", errors.getvalue())
        self.assertIn("post-deploy health failed", errors.getvalue())

    def test_initially_unready_deployment_is_rechecked_until_ready(self):
        clock = [0]
        delays = []
        public_checks = []

        def sleep(seconds):
            delays.append(seconds)
            clock[0] += seconds

        def probe(url, _):
            if url == PUBLIC + "/readyz":
                public_checks.append(url)
                return "http_503" if len(public_checks) == 1 else None
            return None

        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            ready = health.check_deployment(
                LOCAL, PUBLIC, wait_seconds=10, probe=probe,
                now=lambda: clock[0], sleep=sleep,
            )
        self.assertTrue(ready)
        self.assertEqual(public_checks, [PUBLIC + "/readyz"] * 2)
        self.assertEqual(delays, [2])

    def test_failed_deployment_stops_at_deadline(self):
        clock = [0]

        def sleep(seconds):
            clock[0] += seconds

        with contextlib.redirect_stderr(io.StringIO()):
            ready = health.check_deployment(
                LOCAL, PUBLIC, wait_seconds=3,
                probe=lambda *_: "request_failed",
                now=lambda: clock[0], sleep=sleep,
            )
        self.assertFalse(ready)
        self.assertEqual(clock[0], 3)


if __name__ == "__main__":
    unittest.main()
