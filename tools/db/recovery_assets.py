"""Check persistent media references from the actual PostgreSQL snapshot."""
from pathlib import PurePosixPath
import re
import subprocess


def required_media_keys(lines):
    columns = None
    required = set()
    for line in lines:
        line = line.rstrip("\r\n")
        match = re.fullmatch(r'COPY public\.mini_chat_media \((.+)\) FROM stdin;', line)
        if match:
            columns = [name.strip(' "') for name in match[1].split(",")]
            continue
        if columns is None:
            continue
        if line == r"\.":
            columns = None
            continue
        fields = line.split("\t")
        if len(fields) != len(columns):
            raise RuntimeError("Invalid chat media snapshot data")
        row = {key: (None if value == r"\N" else value) for key, value in zip(columns, fields)}
        if row.get("cleaned_at") is not None:
            continue
        status = row.get("upload_status")
        names = ("processed_object_key", "thumbnail_object_key") if status == "ready" else (
            ("source_object_key",) if status in ("uploaded", "processing") else ())
        for name in names:
            key = row.get(name)
            if not key or not key.startswith("chat_media/") or "\\" in key or any(
                    part in ("", ".", "..") for part in key.split("/")):
                raise RuntimeError("Invalid required chat media object key in database snapshot")
            required.add(key)
    return required


def validate_media(dump, stage, bindings, inventories, pg_restore):
    result = subprocess.run([pg_restore, "--data-only", "--table=mini_chat_media", "--file=-", str(dump)],
                            check=True, capture_output=True, text=True)
    required = required_media_keys(result.stdout.splitlines())
    remote = next((item for item in inventories if item["configuration"] == "MOBILE_CHAT_MEDIA_R2"), None)
    if remote is not None:
        present = {item["key"] for item in remote["objects"]}
    else:
        binding = next(item for item in bindings if item["key"] == "MOBILE_CHAT_MEDIA_STORE_DIR")
        present = {key for key in required if (stage / binding["payload"] / PurePosixPath(key)).is_file()}
    if required - present:
        raise RuntimeError("Backup is missing referenced chat media objects; retry with writers stopped")
