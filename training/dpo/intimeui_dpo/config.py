"""DPO 训练配置加载与校验。"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

DEFAULTS: dict[str, Any] = {
    "model_name": "Qwen/Qwen2.5-Coder-7B-Instruct",
    "dataset_path": "data/dpo/export.jsonl",
    "output_dir": "outputs/dpo-qwen2.5-coder-7b",
    "max_length": 4096,
    "learning_rate": 5e-5,
    "epochs": 1,
    "batch_size": 1,
    "gradient_accumulation_steps": 8,
    "beta": 0.1,
    "lora_r": 16,
    "lora_alpha": 32,
    "lora_dropout": 0.05,
    "seed": 42,
}

REQUIRED_TYPES: dict[str, type | tuple[type, ...]] = {
    "model_name": str,
    "dataset_path": str,
    "output_dir": str,
    "max_length": int,
    "learning_rate": (int, float),
    "epochs": (int, float),
    "batch_size": int,
    "gradient_accumulation_steps": int,
    "beta": (int, float),
    "lora_r": int,
    "lora_alpha": int,
    "lora_dropout": (int, float),
    "seed": int,
}


def load_config(path: str | Path | None = None) -> dict[str, Any]:
    """加载 JSON 配置并与默认值合并；未知键直接拒绝。"""
    config = dict(DEFAULTS)
    if path is not None:
        raw = json.loads(Path(path).read_text(encoding="utf-8"))
        if not isinstance(raw, dict):
            raise ValueError("config must be a JSON object")
        unknown = set(raw) - set(DEFAULTS)
        if unknown:
            raise ValueError(f"unknown config keys: {sorted(unknown)}")
        config.update(raw)

    for key, expected in REQUIRED_TYPES.items():
        value = config.get(key)
        if not isinstance(value, expected) or isinstance(value, bool):
            raise ValueError(f"config `{key}` must be {expected!r}")
    if not 0.0 < float(config["learning_rate"]) <= 1.0:
        raise ValueError("learning_rate must be in (0, 1]")
    if not 0.0 < float(config["beta"]) <= 1.0:
        raise ValueError("beta must be in (0, 1]")
    if not 0.0 <= float(config["lora_dropout"]) < 1.0:
        raise ValueError("lora_dropout must be in [0, 1)")
    return config
