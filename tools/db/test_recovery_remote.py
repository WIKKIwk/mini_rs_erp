"""Exercise signed R2 backup/recovery against an isolated HTTP fixture."""
import hashlib
import hmac
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
import tempfile
import threading
import unittest
from urllib.parse import parse_qs, quote, unquote, urlsplit
from xml.sax.saxutils import escape

from recovery_remote import capture_remote, restore_remote


class RemoteTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="accord-r2-fixture-")
        self.addCleanup(self.temporary.cleanup)
        self.stage = Path(self.temporary.name)
        self.objects = {"chat_media/a video+1": b"first-video", "chat_media/b": b"second-video"}
        self.puts = []
        fixture = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def authorize(self, body=b""):
                authorization = self.headers.get("Authorization", "")
                prefix, fields = authorization.split(" ", 1)
                fields = dict(part.split("=", 1) for part in fields.split(", "))
                credential = fields["Credential"].split("/")
                scope = "/".join(credential[1:])
                parsed = urlsplit(self.path)
                query = parse_qs(parsed.query, keep_blank_values=True)
                canonical_query = "&".join(quote(k, safe="-_.~") + "=" + quote(v[0], safe="-_.~")
                                            for k, v in sorted(query.items()))
                headers = fields["SignedHeaders"].split(";")
                canonical_headers = "".join(name + ":" + self.headers[name].strip() + "\n" for name in headers)
                content_hash = hashlib.sha256(body).hexdigest()
                canonical = "\n".join((self.command, parsed.path, canonical_query,
                                       canonical_headers, fields["SignedHeaders"], content_hash))
                to_sign = "\n".join((prefix, self.headers["X-Amz-Date"], scope,
                                     hashlib.sha256(canonical.encode()).hexdigest()))
                key = b"AWS4fixture-secret"
                for value in credential[1:]:
                    key = hmac.new(key, value.encode(), hashlib.sha256).digest()
                expected = hmac.new(key, to_sign.encode(), hashlib.sha256).hexdigest()
                fixture.assertEqual(fields["Signature"], expected)
                fixture.assertEqual(self.headers["X-Amz-Content-Sha256"], content_hash)

            def respond(self, code, body=b"", content_type="application/octet-stream"):
                self.send_response(code)
                self.send_header("Content-Length", str(len(body)))
                self.send_header("Content-Type", content_type)
                self.end_headers()
                self.wfile.write(body)

            def do_GET(self):
                self.authorize()
                parsed = urlsplit(self.path)
                query = parse_qs(parsed.query)
                if "list-type" in query:
                    keys = sorted(key for key in fixture.objects if key.startswith(query["prefix"][0]))
                    page = 1 if query.get("continuation-token") == ["next + token"] else 0
                    key = keys[page]
                    body = ('<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">'
                            f'<Contents><Key>{escape(key)}</Key><ETag>"etag"</ETag>'
                            f'<Size>{len(fixture.objects[key])}</Size></Contents>'
                            f'<IsTruncated>{str(page == 0).lower()}</IsTruncated>'
                            + ('<NextContinuationToken>next + token</NextContinuationToken>' if page == 0 else "")
                            + '</ListBucketResult>').encode()
                    self.respond(200, body, "application/xml")
                    return
                key = unquote(parsed.path.removeprefix("/bucket/"))
                if key not in fixture.objects:
                    self.respond(404)
                    return
                self.respond(200, fixture.objects[key], "video/mp4")

            def do_PUT(self):
                body = self.rfile.read(int(self.headers["Content-Length"]))
                self.authorize(body)
                fixture.assertEqual(self.headers["If-None-Match"], "*")
                key = unquote(urlsplit(self.path).path.removeprefix("/bucket/"))
                if key in fixture.objects:
                    self.respond(412)
                    return
                fixture.objects[key] = body
                fixture.puts.append(key)
                self.respond(200)

        self.server = HTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.addCleanup(self.stop)
        self.env = {
            "MOBILE_CHAT_MEDIA_R2_ENDPOINT": f"http://127.0.0.1:{self.server.server_port}",
            "MOBILE_CHAT_MEDIA_R2_BUCKET": "bucket",
            "MOBILE_CHAT_MEDIA_R2_ACCESS_KEY_ID": "fixture-access",
            "MOBILE_CHAT_MEDIA_R2_SECRET_ACCESS_KEY": "fixture-secret",
        }

    def stop(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()

    def test_paginated_objects_are_saved_and_missing_objects_are_recovered(self):
        inventory = capture_remote(self.env, self.stage)
        items = inventory[0]["objects"]
        self.assertEqual(len(items), 2)
        for item in items:
            self.assertEqual((self.stage / item["payload"]).read_bytes(), self.objects[item["key"]])
        restore_remote(self.env, self.stage, inventory)
        self.assertEqual(self.puts, [])
        del self.objects["chat_media/a video+1"]
        restore_remote(self.env, self.stage, inventory)
        self.assertEqual(self.puts, ["chat_media/a video+1"])
        self.assertEqual(self.objects["chat_media/a video+1"], b"first-video")

    def test_existing_different_object_is_never_overwritten(self):
        inventory = capture_remote(self.env, self.stage)
        self.objects["chat_media/a video+1"] = b"changed-content"
        with self.assertRaisesRegex(RuntimeError, "conflicting"):
            restore_remote(self.env, self.stage, inventory)
        self.assertEqual(self.objects["chat_media/a video+1"], b"changed-content")
        self.assertEqual(self.puts, [])

    def test_partial_remote_configuration_cannot_claim_complete_backup(self):
        del self.env["MOBILE_CHAT_MEDIA_R2_SECRET_ACCESS_KEY"]
        with self.assertRaisesRegex(RuntimeError, "Incomplete R2"):
            capture_remote(self.env, self.stage)


if __name__ == "__main__":
    unittest.main()
