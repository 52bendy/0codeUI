"""DPO JSONL 数据契约校验。

导出格式（`dpo-jsonl-v1`）每行一条记录：

```json
{
  "id": "PREF_...",
  "prompt": "...",
  "chosen": "<!DOCTYPE html>...",
  "rejected": "<!DOCTYPE html>...",
  "metadata": {
    "source": "EXPLICIT_SURVEY",
    "scene_code": "ORDER_DETAIL_PAGE",
    "user_hash": "u_...",
    "quality_score": 0.95
  }
}
```

`metadata` 中其余字段（模板版本、模型版本、偏好强度、时间等）为可选溯源信息。
本模块只依赖标准库，便于在无 GPU 的数据准备环境运行。
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Iterable

REQUIRED_TOP_LEVEL = ("id", "prompt", "chosen", "rejected", "metadata")
REQUIRED_METADATA = ("source", "scene_code", "user_hash", "quality_score")
ALLOWED_SOURCES = {"EXPLICIT_SURVEY", "IMPLICIT_SIGNAL", "AB_TEST", "MANUAL"}


class SchemaError(ValueError):
    """数据不符合 DPO JSONL 契约。"""


def validate_record(record: Any, index: int = 0) -> list[str]:
    """返回单条记录的全部错误；空列表表示合法。"""
    prefix = f"record[{index}]"
    if not isinstance(record, dict):
        return [f"{prefix}: must be a JSON object"]

    errors: list[str] = []
    for key in REQUIRED_TOP_LEVEL:
        if key not in record:
            errors.append(f"{prefix}: missing `{key}`")
    if errors:
        return errors

    for key in ("id", "prompt", "chosen", "rejected"):
        value = record[key]
        if not isinstance(value, str) or not value.strip():
            errors.append(f"{prefix}.{key}: must be a non-empty string")

    if isinstance(record["chosen"], str) and isinstance(record["rejected"], str):
        if record["chosen"].strip() == record["rejected"].strip():
            errors.append(f"{prefix}: chosen and rejected must differ")
        if not record["chosen"].lstrip().lower().startswith("<!doctype html"):
            errors.append(f"{prefix}.chosen: must be a complete HTML document")

    metadata = record["metadata"]
    if not isinstance(metadata, dict):
        errors.append(f"{prefix}.metadata: must be a JSON object")
        return errors
    for key in REQUIRED_METADATA:
        if key not in metadata:
            errors.append(f"{prefix}.metadata: missing `{key}`")
    if metadata.get("source") not in ALLOWED_SOURCES:
        errors.append(f"{prefix}.metadata.source: must be one of {sorted(ALLOWED_SOURCES)}")
    if not isinstance(metadata.get("scene_code"), str) or not metadata.get("scene_code", "").strip():
        errors.append(f"{prefix}.metadata.scene_code: must be a non-empty string")
    user_hash = metadata.get("user_hash")
    if not isinstance(user_hash, str) or not user_hash.startswith("u_"):
        errors.append(f"{prefix}.metadata.user_hash: must start with `u_`")
    quality = metadata.get("quality_score")
    if not isinstance(quality, (int, float)) or not 0.0 <= float(quality) <= 1.0:
        errors.append(f"{prefix}.metadata.quality_score: must be a number in [0, 1]")

    return errors


def validate_records(records: Iterable[Any]) -> list[str]:
    """校验多条记录并汇总错误。"""
    errors: list[str] = []
    for index, record in enumerate(records):
        errors.extend(validate_record(record, index))
    return errors


def assert_valid_record(record: Any, index: int = 0) -> None:
    """校验单条记录，非法时抛出 :class:`SchemaError`。"""
    errors = validate_record(record, index)
    if errors:
        raise SchemaError("; ".join(errors))


def load_jsonl(path: str | Path) -> list[dict[str, Any]]:
    """读取 JSONL 并校验每一行；空行忽略。"""
    records: list[dict[str, Any]] = []
    with Path(path).open("r", encoding="utf-8") as handle:
        for line_number, line in enumerate(handle, start=1):
            line = line.strip()
            if not line:
                continue
            try:
                record = json.loads(line)
            except json.JSONDecodeError as error:
                raise SchemaError(f"{path}:{line_number}: invalid JSON: {error}") from error
            assert_valid_record(record, len(records))
            records.append(record)
    if not records:
        raise SchemaError(f"{path}: dataset is empty")
    return records
