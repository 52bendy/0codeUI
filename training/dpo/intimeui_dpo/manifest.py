"""数据集清单：哈希、记录数与可复现校验。"""

from __future__ import annotations

import hashlib
import json
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from .schema import load_jsonl

MANIFEST_FORMAT = "dpo-jsonl-v1"


def sha256_file(path: str | Path) -> str:
    """计算文件 SHA-256。"""
    digest = hashlib.sha256()
    with Path(path).open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def build_manifest(
    dataset_path: str | Path,
    dataset_id: str | None = None,
    filters: dict[str, Any] | None = None,
    record_count: int | None = None,
) -> dict[str, Any]:
    """基于数据集文件生成清单。"""
    path = Path(dataset_path)
    records = load_jsonl(path)
    source_counts: dict[str, int] = {}
    scene_counts: dict[str, int] = {}
    user_hashes: set[str] = set()
    for record in records:
        metadata = record["metadata"]
        source = str(metadata["source"])
        scene = str(metadata["scene_code"])
        source_counts[source] = source_counts.get(source, 0) + 1
        scene_counts[scene] = scene_counts.get(scene, 0) + 1
        user_hashes.add(str(metadata["user_hash"]))

    return {
        "dataset_id": dataset_id or f"DS_{uuid.uuid4().hex}",
        "format": MANIFEST_FORMAT,
        "created_at": datetime.now(timezone.utc).isoformat(),
        "record_count": len(records) if record_count is None else record_count,
        "source_counts": dict(sorted(source_counts.items())),
        "scene_counts": dict(sorted(scene_counts.items())),
        "unique_user_count": len(user_hashes),
        "sha256": sha256_file(path),
        "filters": filters or {},
    }


def write_manifest(
    dataset_path: str | Path,
    manifest_path: str | Path,
    dataset_id: str | None = None,
    filters: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """生成并写入清单 JSON。"""
    manifest = build_manifest(dataset_path, dataset_id=dataset_id, filters=filters)
    Path(manifest_path).write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    return manifest


def verify_manifest(dataset_path: str | Path, manifest: dict[str, Any]) -> bool:
    """校验数据集哈希、格式与记录数是否与清单一致。"""
    if manifest.get("format") != MANIFEST_FORMAT:
        return False
    if not Path(dataset_path).is_file():
        return False
    if sha256_file(dataset_path) != manifest.get("sha256"):
        return False
    records = load_jsonl(dataset_path)
    return len(records) == manifest.get("record_count")
