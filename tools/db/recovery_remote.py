"""Copy application R2 objects into the recovery archive, never just URLs."""
from __future__ import annotations

from datetime import datetime, timezone
import hashlib
import hmac
from pathlib import Path
import shutil
from urllib.error import HTTPError, URLError
from urllib.parse import quote, urlsplit
from urllib.request import Request, urlopen
import xml.etree.ElementTree as ET


class R2:
    def __init__(self, env, prefix):
        self.endpoint = env[prefix + "_ENDPOINT"].rstrip("/")
        self.bucket = env[prefix + "_BUCKET"]
        self.access = env[prefix + "_ACCESS_KEY_ID"]
        self.secret = env[prefix + "_SECRET_ACCESS_KEY"]
        self.region = env.get(prefix + "_REGION", "auto") or "auto"

    def request(self, method, key="", query=None, headers=None, content=None):
        endpoint = urlsplit(self.endpoint)
        if endpoint.scheme not in ("https", "http") or endpoint.query or endpoint.fragment:
            raise RuntimeError("Invalid R2 backup endpoint")
        path = quote(endpoint.path.rstrip("/") + "/" + self.bucket + "/" + key, safe="/-_.~")
        query_string = "&".join(quote(k, safe="-_.~") + "=" + quote(v, safe="-_.~")
                                for k, v in sorted((query or {}).items()))
        url = endpoint.scheme + "://" + endpoint.netloc + path
        if query_string:
            url += "?" + query_string
        now = datetime.now(timezone.utc)
        stamp, day = now.strftime("%Y%m%dT%H%M%SZ"), now.strftime("%Y%m%d")
        digest = hashlib.sha256()
        if content:
            with Path(content).open("rb") as stream:
                for block in iter(lambda: stream.read(1024 * 1024), b""):
                    digest.update(block)
        signed = {"host": endpoint.netloc, "x-amz-date": stamp,
                  "x-amz-content-sha256": digest.hexdigest(), **(headers or {})}
        signed_names = ";".join(sorted(signed))
        canonical_headers = "".join(k + ":" + signed[k].strip() + "\n" for k in sorted(signed))
        canonical = "\n".join((method, path, query_string, canonical_headers,
                               signed_names, digest.hexdigest()))
        scope = day + "/" + self.region + "/s3/aws4_request"
        to_sign = "AWS4-HMAC-SHA256\n" + stamp + "\n" + scope + "\n" + hashlib.sha256(canonical.encode()).hexdigest()
        signing_key = ("AWS4" + self.secret).encode()
        for value in (day, self.region, "s3", "aws4_request"):
            signing_key = hmac.new(signing_key, value.encode(), hashlib.sha256).digest()
        signature = hmac.new(signing_key, to_sign.encode(), hashlib.sha256).hexdigest()
        signed["Authorization"] = ("AWS4-HMAC-SHA256 Credential=" + self.access + "/" + scope
                                    + ", SignedHeaders=" + signed_names + ", Signature=" + signature)
        stream = Path(content).open("rb") if content else None
        try:
            return urlopen(Request(url, data=stream, headers=signed, method=method), timeout=120)
        except HTTPError:
            raise
        except (URLError, OSError) as error:
            raise RuntimeError("R2 recovery request failed") from error
        finally:
            if stream:
                stream.close()

    def objects(self, prefix):
        token = None
        seen = set()
        while True:
            query = {"list-type": "2", "prefix": prefix}
            if token:
                query["continuation-token"] = token
            with self.request("GET", query=query) as response:
                tree = ET.fromstring(response.read())
            ns = {"s": tree.tag.partition("}")[0][1:]} if "}" in tree.tag else {}
            def field(element, name):
                return element.findtext(("s:" if ns else "") + name, namespaces=ns)
            for item in tree.findall(("s:" if ns else "") + "Contents", ns):
                key, etag, size = field(item, "Key"), field(item, "ETag"), field(item, "Size")
                if not key or not key.startswith(prefix) or not etag or size is None:
                    raise RuntimeError("Invalid R2 inventory")
                yield key, etag, int(size)
            if field(tree, "IsTruncated") != "true":
                return
            token = field(tree, "NextContinuationToken")
            if not token or token in seen:
                raise RuntimeError("Invalid R2 pagination")
            seen.add(token)


def capture_remote(env, stage):
    inventories = []
    for name, object_prefix in (("MOBILE_CHAT_MEDIA_R2", "chat_media/"),
                                ("MOBILE_PROFILE_AVATAR_R2", "profile_avatars/")):
        required = [name + suffix for suffix in ("_ENDPOINT", "_BUCKET", "_ACCESS_KEY_ID", "_SECRET_ACCESS_KEY")]
        configured = [bool(env.get(key)) for key in required]
        if not any(configured):
            continue
        if not all(configured):
            raise RuntimeError("Incomplete R2 configuration: " + name)
        client = R2(env, name)
        objects = []
        for key, etag, size in client.objects(object_prefix):
            payload = "remote/" + name + "/" + hashlib.sha256(key.encode()).hexdigest()
            path = stage / payload
            path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
            with client.request("GET", key, headers={"if-match": etag}) as response:
                content_type = response.headers.get("Content-Type", "application/octet-stream")
                with path.open("xb") as target:
                    shutil.copyfileobj(response, target, 1024 * 1024)
            if path.stat().st_size != size:
                raise RuntimeError("R2 object size changed during backup")
            objects.append({"key": key, "payload": payload, "content_type": content_type, "size": size})
        inventories.append({"configuration": name, "objects": objects})
    return inventories


def restore_remote(env, stage, inventories):
    """Reuse identical objects; recover missing objects without overwriting."""
    for inventory in inventories:
        client = R2(env, inventory["configuration"])
        for item in inventory["objects"]:
            path = stage / item["payload"]
            try:
                with client.request("GET", item["key"]) as response:
                    existing = hashlib.sha256()
                    for block in iter(lambda: response.read(1024 * 1024), b""):
                        existing.update(block)
                digest = hashlib.sha256()
                with path.open("rb") as stream:
                    for block in iter(lambda: stream.read(1024 * 1024), b""):
                        digest.update(block)
                local = digest.hexdigest()
                if existing.hexdigest() != local:
                    raise RuntimeError("R2 recovery found conflicting existing content; no object overwritten")
            except HTTPError as error:
                if error.code != 404:
                    raise RuntimeError("Cannot validate R2 recovery destination") from error
                headers = {"if-none-match": "*", "content-type": item["content_type"],
                           "content-length": str(path.stat().st_size)}
                with client.request("PUT", item["key"], headers=headers, content=path):
                    pass
