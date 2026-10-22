#!/usr/bin/env python3
"""无 GPU 的端到端冒烟：校验 → 切分 → 训练计划 → 离线评测。"""

from __future__ import annotations

import argparse
import json
import sys
import tempfile
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR.parent))

import evaluate_dpo
import prepare_dataset
import train_dpo


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Smoke-test the offline DPO pipeline")
    parser.add_argument("--data", default=str(SCRIPT_DIR.parent / "data" / "seed_pairs.jsonl"))
    parser.add_argument("--config", default=str(SCRIPT_DIR.parent / "configs" / "dpo_qwen2_5_coder_7b.json"))
    parser.add_argument("--workdir", help="keep artifacts in this directory")
    return parser.parse_args(argv)


def run(data: str, config: str, workdir: Path) -> dict:
    dataset_dir = workdir / "dataset"
    run_dir = workdir / "run"
    eval_dir = workdir / "eval"

    code = prepare_dataset.main(
        ["--input", data, "--output-dir", str(dataset_dir), "--eval-ratio", "0.34"]
    )
    if code != 0:
        raise SystemExit(f"prepare_dataset failed: {code}")

    code = train_dpo.main(
        [
            "--config", config,
            "--dataset", str(dataset_dir / "train.jsonl"),
            "--eval-dataset", str(dataset_dir / "eval.jsonl"),
            "--output-dir", str(run_dir),
            "--dry-run",
        ]
    )
    if code != 0:
        raise SystemExit(f"train_dpo dry-run failed: {code}")

    code = evaluate_dpo.main(
        [
            "--dataset", str(dataset_dir / "eval.jsonl"),
            "--output-dir", str(eval_dir),
            # 冒烟不测真实延迟，用 0 占位；真实评测必须传入实测值。
            "--latency-increase-ratio", "0.0",
        ]
    )
    report = json.loads((eval_dir / "eval_report.json").read_text(encoding="utf-8"))
    plan = json.loads((run_dir / "training_plan.json").read_text(encoding="utf-8"))
    summary = {
        "train_count": plan["train_count"],
        "eval_count": plan["eval_count"],
        "planned_steps": plan["planned_steps"],
        "pair_accuracy": report["metrics"]["pair_accuracy"],
        "html_valid_rate": report["metrics"]["html_valid_rate"],
        "gates_passed": report["passed"],
    }
    if code not in (0, 1):
        raise SystemExit(f"evaluate_dpo failed: {code}")
    return summary


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    if args.workdir:
        summary = run(args.data, args.config, Path(args.workdir))
    else:
        with tempfile.TemporaryDirectory(prefix="intimeui-dpo-smoke-") as tmp:
            summary = run(args.data, args.config, Path(tmp))
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    return 0 if summary["gates_passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
