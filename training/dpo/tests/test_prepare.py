"""数据切分与准备脚本测试。"""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

import prepare_dataset  # noqa: E402

from intimeui_dpo.dataset import dataset_stats, load_dataset, split_records, write_jsonl  # noqa: E402
from intimeui_dpo.manifest import build_manifest  # noqa: E402

DATA = ROOT / "data" / "seed_pairs.jsonl"


class PrepareTest(unittest.TestCase):
    def test_split_is_deterministic_and_complete(self) -> None:
        records = load_dataset(DATA)
        first = split_records(records, 0.34, "seed-a")
        second = split_records(records, 0.34, "seed-a")
        self.assertEqual([r["id"] for r in first[0]], [r["id"] for r in second[0]])
        self.assertEqual([r["id"] for r in first[1]], [r["id"] for r in second[1]])
        combined = {r["id"] for r in first[0]} | {r["id"] for r in first[1]}
        self.assertEqual(combined, {r["id"] for r in records})
        self.assertEqual(len(first[1]), 1)

    def test_zero_eval_ratio(self) -> None:
        records = load_dataset(DATA)
        train, evaluation = split_records(records, 0.0)
        self.assertEqual(len(train), 3)
        self.assertEqual(evaluation, [])

    def test_invalid_eval_ratio(self) -> None:
        with self.assertRaises(ValueError):
            split_records(load_dataset(DATA), 1.0)

    def test_write_jsonl_roundtrip(self) -> None:
        records = load_dataset(DATA)
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp) / "out" / "pairs.jsonl"
            count = write_jsonl(records, target)
            self.assertEqual(count, 3)
            self.assertEqual([r["id"] for r in load_dataset(target)], [r["id"] for r in records])

    def test_dataset_stats(self) -> None:
        stats = dataset_stats(load_dataset(DATA))
        self.assertEqual(stats["record_count"], 3)
        self.assertEqual(stats["unique_user_count"], 3)
        self.assertAlmostEqual(stats["average_quality"], 0.9, places=4)

    def test_prepare_main_writes_splits_and_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            code = prepare_dataset.main(
                ["--input", str(DATA), "--output-dir", tmp, "--eval-ratio", "0.34"]
            )
            self.assertEqual(code, 0)
            self.assertTrue((Path(tmp) / "train.jsonl").is_file())
            self.assertTrue((Path(tmp) / "eval.jsonl").is_file())
            summary = json.loads((Path(tmp) / "dataset_manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(summary["train_count"], 2)
            self.assertEqual(summary["eval_count"], 1)
            self.assertEqual(summary["stats"]["record_count"], 3)

    def test_prepare_main_rejects_bad_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest = build_manifest(DATA)
            manifest["sha256"] = "0" * 64
            manifest_path = Path(tmp) / "manifest.json"
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            code = prepare_dataset.main(
                [
                    "--input", str(DATA),
                    "--manifest", str(manifest_path),
                    "--output-dir", str(Path(tmp) / "out"),
                ]
            )
            self.assertEqual(code, 2)


if __name__ == "__main__":
    unittest.main()
