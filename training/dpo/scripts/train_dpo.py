#!/usr/bin/env python3
"""DPO 离线训练入口（LoRA/QLoRA 骨架）。

默认以 `--dry-run` 运行：只做数据与配置校验并产出训练计划，不需要 GPU。
真实训练需安装 requirements.txt 中的依赖。
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from intimeui_dpo.config import load_config
from intimeui_dpo.dataset import dataset_stats, load_dataset


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Train InTimeUI DPO adapter")
    parser.add_argument("--config", required=True, help="path to dpo config json")
    parser.add_argument("--dataset", required=True, help="prepared train.jsonl")
    parser.add_argument("--eval-dataset", help="prepared eval.jsonl")
    parser.add_argument("--output-dir", help="override output_dir from config")
    parser.add_argument("--max-steps", type=int, default=-1)
    parser.add_argument("--dry-run", action="store_true", help="validate and emit plan only")
    return parser.parse_args(argv)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def build_plan(config: dict, train_path: Path, eval_path: Path | None, max_steps: int) -> dict:
    records = load_dataset(train_path)
    eval_count = len(load_dataset(eval_path)) if eval_path else 0
    effective_batch = int(config["batch_size"]) * int(config["gradient_accumulation_steps"])
    steps_per_epoch = max(1, len(records) // effective_batch)
    planned_steps = max_steps if max_steps > 0 else steps_per_epoch * int(config["epochs"])
    return {
        "created_at": datetime.now(timezone.utc).isoformat(),
        "model_name": config["model_name"],
        "train_path": str(train_path),
        "train_sha256": sha256_file(train_path),
        "eval_path": str(eval_path) if eval_path else None,
        "eval_sha256": sha256_file(eval_path) if eval_path else None,
        "train_count": len(records),
        "eval_count": eval_count,
        "effective_batch_size": effective_batch,
        "planned_steps": planned_steps,
        "stats": dataset_stats(records),
        "config": config,
    }


def run_training(config: dict, train_path: Path, eval_path: Path | None, output_dir: Path, max_steps: int) -> None:
    """真实训练路径；导入失败时给出明确安装提示。"""
    try:
        import torch
        from datasets import load_dataset as hf_load_dataset
        from peft import LoraConfig, get_peft_model, prepare_model_for_kbit_training
        from transformers import AutoModelForCausalLM, AutoTokenizer, BitsAndBytesConfig
        from trl import DPOConfig, DPOTrainer
    except ImportError as error:  # pragma: no cover - 依赖缺失路径
        raise SystemExit(
            f"training dependencies missing: {error}. Install training/dpo/requirements.txt"
        ) from error

    if not torch.cuda.is_available():
        raise SystemExit("CUDA is required for real training; use --dry-run for data checks")

    tokenizer = AutoTokenizer.from_pretrained(config["model_name"], trust_remote_code=True)
    if tokenizer.pad_token is None:
        tokenizer.pad_token = tokenizer.eos_token

    quant_config = BitsAndBytesConfig(
        load_in_4bit=True,
        bnb_4bit_quant_type="nf4",
        bnb_4bit_compute_dtype=torch.bfloat16,
    )
    model = AutoModelForCausalLM.from_pretrained(
        config["model_name"],
        quantization_config=quant_config,
        device_map="auto",
        trust_remote_code=True,
    )
    model = prepare_model_for_kbit_training(model)
    lora = LoraConfig(
        r=int(config["lora_r"]),
        lora_alpha=int(config["lora_alpha"]),
        lora_dropout=float(config["lora_dropout"]),
        bias="none",
        task_type="CAUSAL_LM",
        target_modules=["q_proj", "k_proj", "v_proj", "o_proj", "gate_proj", "up_proj", "down_proj"],
    )
    model = get_peft_model(model, lora)

    data = {"train": str(train_path)}
    if eval_path:
        data["validation"] = str(eval_path)
    dataset = hf_load_dataset("json", data=data)

    training_args = DPOConfig(
        output_dir=str(output_dir),
        per_device_train_batch_size=int(config["batch_size"]),
        gradient_accumulation_steps=int(config["gradient_accumulation_steps"]),
        learning_rate=float(config["learning_rate"]),
        num_train_epochs=float(config["epochs"]),
        max_steps=max_steps,
        beta=float(config["beta"]),
        max_length=int(config["max_length"]),
        logging_steps=10,
        save_strategy="epoch",
        eval_strategy="epoch" if eval_path else "no",
        bf16=True,
        seed=int(config["seed"]),
        report_to=[],
    )
    trainer = DPOTrainer(
        model=model,
        ref_model=None,
        args=training_args,
        train_dataset=dataset["train"],
        eval_dataset=dataset.get("validation"),
        processing_class=tokenizer,
    )
    trainer.train()
    trainer.save_model(str(output_dir))
    tokenizer.save_pretrained(str(output_dir))


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    config = load_config(args.config)
    output_dir = Path(args.output_dir or config["output_dir"])
    train_path = Path(args.dataset)
    eval_path = Path(args.eval_dataset) if args.eval_dataset else None

    plan = build_plan(config, train_path, eval_path, args.max_steps)
    output_dir.mkdir(parents=True, exist_ok=True)
    (output_dir / "training_plan.json").write_text(
        json.dumps(plan, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    print(json.dumps({k: plan[k] for k in ("train_count", "eval_count", "planned_steps")}, ensure_ascii=False))

    if args.dry_run:
        print(f"dry-run ok; plan written to {output_dir / 'training_plan.json'}")
        return 0

    run_training(config, train_path, eval_path, output_dir, args.max_steps)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
