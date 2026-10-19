from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from recovery_assets import required_media_keys, validate_media


COPY = "COPY public.mini_chat_media (upload_status, source_object_key, processed_object_key, thumbnail_object_key, cleaned_at) FROM stdin;\n"


class MediaCoverageTests(unittest.TestCase):
    def test_ready_content_and_unfinished_processing_require_different_objects(self):
        lines = [
            COPY,
            "ready\tchat_media/old-source\tchat_media/a/processed\tchat_media/a/thumbnail\t\\N",
            "processing\tchat_media/b/source\t\\N\t\\N\t\\N",
            "pending\tchat_media/not-yet-uploaded\t\\N\t\\N\t\\N",
            "ready\tchat_media/removed\tchat_media/removed\tchat_media/removed\t2026-01-01",
            r"\.",
        ]
        self.assertEqual(required_media_keys(lines), {
            "chat_media/a/processed", "chat_media/a/thumbnail", "chat_media/b/source"})

    def test_missing_snapshot_references_fail_instead_of_silently_losing_media(self):
        with tempfile.TemporaryDirectory() as temporary:
            stage = Path(temporary)
            bindings = [{"key": "MOBILE_CHAT_MEDIA_STORE_DIR", "payload": "state/media"}]
            stdout = COPY + "processing\tchat_media/source\t\\N\t\\N\t\\N\n\\.\n"
            with patch("recovery_assets.subprocess.run") as execute:
                execute.return_value.stdout = stdout
                with self.assertRaisesRegex(RuntimeError, "missing referenced"):
                    validate_media(stage / "db.dump", stage, bindings, [], "pg_restore")
                source = stage / "state/media/chat_media/source"
                source.parent.mkdir(parents=True)
                source.write_bytes(b"source-content")
                validate_media(stage / "db.dump", stage, bindings, [], "pg_restore")

    def test_remote_reference_must_be_in_export_inventory(self):
        with tempfile.TemporaryDirectory() as temporary:
            stage = Path(temporary)
            stdout = COPY + "processing\tchat_media/source\t\\N\t\\N\t\\N\n\\.\n"
            with patch("recovery_assets.subprocess.run") as execute:
                execute.return_value.stdout = stdout
                inventory = [{"configuration": "MOBILE_CHAT_MEDIA_R2", "objects": []}]
                with self.assertRaisesRegex(RuntimeError, "missing referenced"):
                    validate_media(stage / "db.dump", stage, [], inventory, "pg_restore")
                inventory[0]["objects"].append({"key": "chat_media/source"})
                validate_media(stage / "db.dump", stage, [], inventory, "pg_restore")


if __name__ == "__main__":
    unittest.main()
