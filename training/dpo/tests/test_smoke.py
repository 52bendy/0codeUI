"""端到端冒烟测试（无 GPU）。"""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

import smoke_pipeline  # noqa: E402


class SmokePipelineTest(unittest.TestCase):
    def test_smoke_pipeline_passes_gates(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            code = smoke_pipeline.main(["--workdir", tmp])
            self.assertEqual(code, 0)
            plan = json.loads((Path(tmp) / "run" / "training_plan.json").read_text(encoding="utf-8"))
            report = json.loads((Path(tmp) / "eval" / "eval_report.json").read_text(encoding="utf-8"))
            self.assertEqual(plan["train_count"] + plan["eval_count"], 3)
            self.assertGreaterEqual(report["metrics"]["pair_accuracy"], 0.55)
            self.assertTrue(report["passed"])
            self.assertTrue((Path(tmp) / "eval" / "eval_report.md").is_file())


if __name__ == "__main__":
    unittest.main()
