"""数据集清单测试。"""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from intimeui_dpo.manifest import (
    MANIFEST_FORMAT,
    build_manifest,
    sha256_file,
    verify_manifest,
    write_manifest,
)

DATA = Path(__file__).resolve().parents[1] / "data" / "seed_pairs.jsonl"


class ManifestTest(unittest.TestCase):
    def test_build_manifest_counts_and_hash(self) -> None:
        manifest = build_manifest(DATA, dataset_id="DS_TEST")
        self.assertEqual(manifest["dataset_id"], "DS_TEST")
        self.assertEqual(manifest["format"], MANIFEST_FORMAT)
        self.assertEqual(manifest["record_count"], 3)
        self.assertEqual(manifest["unique_user_count"], 3)
        self.assertEqual(sum(manifest["source_counts"].values()), 3)
        self.assertEqual(sum(manifest["scene_counts"].values()), 3)
        self.assertEqual(manifest["sha256"], sha256_file(DATA))

    def test_write_and_verify_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp) / "manifest.json"
            manifest = write_manifest(DATA, target, dataset_id="DS_TEST")
            loaded = json.loads(target.read_text(encoding="utf-8"))
            self.assertEqual(loaded["sha256"], manifest["sha256"])
            self.assertTrue(verify_manifest(DATA, loaded))

    def test_tampered_dataset_fails_verification(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            copy = Path(tmp) / "data.jsonl"
            copy.write_text(DATA.read_text(encoding="utf-8"), encoding="utf-8")
            manifest = build_manifest(copy)
            with copy.open("a", encoding="utf-8") as handle:
                handle.write("\n")
            self.assertFalse(verify_manifest(copy, manifest))

    def test_unknown_format_fails_verification(self) -> None:
        manifest = build_manifest(DATA)
        manifest["format"] = "dpo-jsonl-v0"
        self.assertFalse(verify_manifest(DATA, manifest))


if __name__ == "__main__":
    unittest.main()
