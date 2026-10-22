"""数据集统计、切分与写出（仅标准库）。"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any, Iterable, Sequence

from .schema import assert_valid_record, load_jsonl


def dataset_stats(records: Sequence[dict[str, Any]]) -> dict[str, Any]:
    """汇总数据集规模、来源、场景与用户分布。"""
    sources: dict[str, int] = {}
    scenes: dict[str, int] = {}
    users: set[str] = set()
    quality_sum = 0.0
    for record in records:
        metadata = record["metadata"]
        source = str(metadata["source"])
        scene = str(metadata["scene_code"])
        sources[source] = sources.get(source, 0) + 1
        scenes[scene] = scenes.get(scene, 0) + 1
        users.add(str(metadata["user_hash"]))
        quality_sum += float(metadata["quality_score"])
    count = len(records)
    return {
        "record_count": count,
        "source_counts": dict(sorted(sources.items())),
        "scene_counts": dict(sorted(scenes.items())),
        "unique_user_count": len(users),
        "average_quality": round(quality_sum / count, 4) if count else 0.0,
    }


def _order_key(seed: str, record: dict[str, Any]) -> str:
    return hashlib.sha256(f"{seed}:{record['id']}".encode("utf-8")).hexdigest()


def split_records(
    records: Sequence[dict[str, Any]],
    eval_ratio: float = 0.1,
    seed: str = "intimeui-dpo",
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    """按哈希稳定切分训练集/评测集，避免随机源不可复现。"""
    if not 0.0 <= eval_ratio < 1.0:
        raise ValueError("eval_ratio must be in [0, 1)")
    ordered = sorted(records, key=lambda record: _order_key(seed, record))
    eval_count = int(round(len(ordered) * eval_ratio))
    if eval_ratio > 0 and len(ordered) > 1:
        eval_count = max(1, eval_count)
    eval_count = min(eval_count, max(0, len(ordered) - 1))
    return ordered[eval_count:], ordered[:eval_count]


def write_jsonl(records: Iterable[dict[str, Any]], path: str | Path) -> int:
    """逐行写出并再次校验，返回记录数。"""
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    count = 0
    with target.open("w", encoding="utf-8") as handle:
        for record in records:
            assert_valid_record(record, count)
            handle.write(json.dumps(record, ensure_ascii=False) + "\n")
            count += 1
    return count


def load_dataset(path: str | Path) -> list[dict[str, Any]]:
    """读取并校验数据集。"""
    return load_jsonl(path)
