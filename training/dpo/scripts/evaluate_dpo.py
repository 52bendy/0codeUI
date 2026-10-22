#!/usr/bin/env python3
"""DPO 离线评测入口：指标计算 + 上线门禁判定。"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from intimeui_dpo.dataset import load_dataset
from intimeui_dpo.evaluate import (
    DEFAULT_GATES,
    check_gates,
    evaluate_records,
    gates_passed,
    heuristic_score,
    render_markdown,
)


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Evaluate DPO outputs against gates")
    parser.add_argument("--dataset", required=True)
    parser.add_argument("--predictions", help="jsonl with id/chosen_score/rejected_score")
    parser.add_argument("--latency-increase-ratio", type=float)
    parser.add_argument("--output-dir", required=True)
    return parser.parse_args(argv)


def load_predictions(path: Path) -> dict[str, tuple[float, float]]:
    scores: dict[str, tuple[float, float]] = {}
    with path.open("r", encoding="utf-8") as handle:
        for line in handle:
            if not line.strip():
                continue
            item = json.loads(line)
            scores[str(item["id"])] = (float(item["chosen_score"]), float(item["rejected_score"]))
    return scores


def build_score_map(
    records: list[dict[str, Any]],
    predictions: dict[str, tuple[float, float]],
) -> dict[tuple[str, str], float]:
    """以 (prompt, html) 为键展开打分，避免依赖调用顺序。"""
    scores: dict[tuple[str, str], float] = {}
    for record in records:
        chosen_score, rejected_score = predictions[record["id"]]
        scores[(record["prompt"], record["chosen"])] = chosen_score
        scores[(record["prompt"], record["rejected"])] = rejected_score
    return scores


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    records = load_dataset(args.dataset)
    scorer_name = "heuristic-placeholder"
    scorer = heuristic_score
    if args.predictions:
        predictions = load_predictions(Path(args.predictions))
        missing = [record["id"] for record in records if record["id"] not in predictions]
        if missing:
            print(f"predictions missing {len(missing)} ids, e.g. {missing[:3]}", file=sys.stderr)
            return 2
        score_map = build_score_map(records, predictions)
        scorer = lambda prompt, html: score_map[(prompt, html)]  # noqa: E731
        scorer_name = "predictions"

    metrics = evaluate_records(records, scorer, args.latency_increase_ratio)
    gates = check_gates(metrics, DEFAULT_GATES)
    report: dict[str, Any] = {
        "dataset": args.dataset,
        "scorer": scorer_name,
        "metrics": metrics,
        "gates": gates,
        "passed": gates_passed(gates),
    }
    output_dir = Path(args.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    (output_dir / "eval_report.json").write_text(
        json.dumps(report, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    (output_dir / "eval_report.md").write_text(render_markdown(report), encoding="utf-8")
    print(json.dumps(report["metrics"], ensure_ascii=False))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
