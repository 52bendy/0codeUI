"""dpo-jsonl-v1 契约校验测试。"""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from intimeui_dpo.schema import SchemaError, load_jsonl, validate_record

DATA = Path(__file__).resolve().parents[1] / "data" / "seed_pairs.jsonl"


def sample_record() -> dict:
    return json.loads(DATA.read_text(encoding="utf-8").splitlines()[0])


class SchemaTest(unittest.TestCase):
    def test_seed_dataset_is_valid(self) -> None:
        records = load_jsonl(DATA)
        self.assertEqual(len(records), 3)
        self.assertTrue(all(record["id"].startswith("PREF_SEED_") for record in records))

    def test_missing_metadata_field_is_reported(self) -> None:
        record = sample_record()
        del record["metadata"]["scene_code"]
        errors = validate_record(record, 4)
        self.assertTrue(any("record[4].metadata: missing `scene_code`" in error for error in errors))

    def test_identical_outputs_are_rejected(self) -> None:
        record = sample_record()
        record["rejected"] = record["chosen"]
        self.assertTrue(any("must differ" in error for error in validate_record(record)))

    def test_chosen_must_be_complete_document(self) -> None:
        record = sample_record()
        record["chosen"] = "<div>fragment</div>"
        self.assertTrue(any("complete HTML document" in error for error in validate_record(record)))

    def test_unknown_source_is_rejected(self) -> None:
        record = sample_record()
        record["metadata"]["source"] = "CRAWLER"
        self.assertTrue(any("metadata.source" in error for error in validate_record(record)))

    def test_user_hash_prefix_is_enforced(self) -> None:
        record = sample_record()
        record["metadata"]["user_hash"] = "raw-user-1"
        self.assertTrue(any("user_hash" in error for error in validate_record(record)))

    def test_quality_score_range(self) -> None:
        record = sample_record()
        record["metadata"]["quality_score"] = 1.5
        self.assertTrue(any("quality_score" in error for error in validate_record(record)))

    def test_empty_dataset_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "empty.jsonl"
            path.write_text("\n", encoding="utf-8")
            with self.assertRaises(SchemaError):
                load_jsonl(path)

    def test_invalid_json_reports_line_number(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "bad.jsonl"
            path.write_text("{not json}\n", encoding="utf-8")
            with self.assertRaises(SchemaError) as context:
                load_jsonl(path)
            self.assertIn(":1:", str(context.exception))


if __name__ == "__main__":
    unittest.main()
