"""离线评测指标与上线门禁（骨架，不依赖 GPU）。"""

from __future__ import annotations

from typing import Any, Callable, Sequence

Scorer = Callable[[str, str], float]

DEFAULT_GATES: dict[str, float] = {
    "min_pair_accuracy": 0.55,
    "min_html_valid_rate": 0.98,
    "max_latency_increase_ratio": 0.10,
}

_STRUCTURAL_TAGS = ("<html", "<head", "<body", "<style", "<main", "<section")


def is_complete_html(document: str) -> bool:
    """轻量结构校验，仅用于离线数据质量统计。"""
    lowered = document.lstrip().lower()
    return (
        lowered.startswith("<!doctype html")
        and "<html" in lowered
        and "</html>" in lowered
        and "<body" in lowered
    )


def heuristic_score(prompt: str, html: str) -> float:
    """占位打分器：仅用于流水线冒烟，不代表真实模型质量。"""
    if not is_complete_html(html):
        return 0.0
    tag_bonus = sum(1 for tag in _STRUCTURAL_TAGS if tag in html.lower())
    length_bonus = min(len(html), 20000) / 20000
    prompt_bonus = min(len(prompt), 2000) / 20000
    return round(0.5 + 0.05 * tag_bonus + 0.2 * length_bonus + prompt_bonus, 6)


def evaluate_records(
    records: Sequence[dict[str, Any]],
    scorer: Scorer = heuristic_score,
    latency_increase_ratio: float | None = None,
) -> dict[str, Any]:
    """计算偏好准确率、HTML 合法率与打分边际。"""
    if not records:
        raise ValueError("records must not be empty")
    correct = 0
    margins: list[float] = []
    valid = 0
    for record in records:
        chosen_score = float(scorer(record["prompt"], record["chosen"]))
        rejected_score = float(scorer(record["prompt"], record["rejected"]))
        if chosen_score > rejected_score:
            correct += 1
        margins.append(chosen_score - rejected_score)
        if is_complete_html(record["chosen"]):
            valid += 1
    metrics: dict[str, Any] = {
        "record_count": len(records),
        "pair_accuracy": round(correct / len(records), 4),
        "mean_margin": round(sum(margins) / len(margins), 6),
        "html_valid_rate": round(valid / len(records), 4),
    }
    if latency_increase_ratio is not None:
        metrics["latency_increase_ratio"] = round(float(latency_increase_ratio), 4)
    return metrics


def check_gates(
    metrics: dict[str, Any],
    gates: dict[str, float] | None = None,
) -> list[dict[str, Any]]:
    """逐项检查门禁，返回每项结果。缺失指标视为不通过。"""
    active = dict(DEFAULT_GATES)
    if gates:
        active.update(gates)
    results: list[dict[str, Any]] = []
    for name, threshold in active.items():
        value = metrics.get(name.removeprefix("min_").removeprefix("max_"))
        if name.startswith("min_"):
            passed = value is not None and float(value) >= threshold
            operator = ">="
        else:
            passed = value is not None and float(value) <= threshold
            operator = "<="
        results.append(
            {
                "gate": name,
                "value": value,
                "operator": operator,
                "threshold": threshold,
                "passed": passed,
            }
        )
    return results


def gates_passed(results: Sequence[dict[str, Any]]) -> bool:
    """全部门禁通过才允许进入金丝雀。"""
    return all(bool(item["passed"]) for item in results)


def render_markdown(report: dict[str, Any]) -> str:
    """将评测报告渲染为 Markdown。"""
    metrics = report["metrics"]
    lines = [
        "# DPO 离线评测报告",
        "",
        f"- 数据集：`{report.get('dataset', 'unknown')}`",
        f"- 记录数：{metrics['record_count']}",
        f"- 偏好准确率：{metrics['pair_accuracy']:.2%}",
        f"- 平均打分边际：{metrics['mean_margin']}",
        f"- chosen HTML 合法率：{metrics['html_valid_rate']:.2%}",
        "",
        "| 门禁 | 实际值 | 要求 | 结果 |",
        "| --- | --- | --- | --- |",
    ]
    for item in report["gates"]:
        outcome = "通过" if item["passed"] else "未通过"
        lines.append(
            f"| {item['gate']} | {item['value']} | {item['operator']} {item['threshold']} | {outcome} |"
        )
    lines.append("")
    lines.append(f"**总体结论：{'通过' if report['passed'] else '未通过'}**")
    return "\n".join(lines) + "\n"
