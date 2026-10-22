#!/usr/bin/env python3
"""校验导出数据并切分为训练集/评测集。"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from intimeui_dpo.dataset import dataset_stats, load_dataset, split_records, write_jsonl
from intimeui_dpo.manifest import verify_manifest


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Prepare DPO train/eval splits")
    parser.add_argument("--input", required=True, help="exported dpo-jsonl-v1 file")
    parser.add_argument("--manifest", help="export manifest json for integrity check")
    parser.add_argument("--output-dir", required=True)
    parser.add_argument("--eval-ratio", type=float, default=0.1)
    parser.add_argument("--seed", default="intimeui-dpo")
    return parser.parse_args(argv)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    source = Path(args.input)
    if args.manifest:
        manifest = json.loads(Path(args.manifest).read_text(encoding="utf-8"))
        if not verify_manifest(source, manifest):
            print("manifest verification failed", file=sys.stderr)
            return 2

    records = load_dataset(source)
    train, evaluation = split_records(records, args.eval_ratio, args.seed)
    output_dir = Path(args.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    write_jsonl(train, output_dir / "train.jsonl")
    write_jsonl(evaluation, output_dir / "eval.jsonl")

    summary = {
        "source": str(source),
        "source_sha256": sha256_file(source),
        "seed": args.seed,
        "eval_ratio": args.eval_ratio,
        "train_count": len(train),
        "eval_count": len(evaluation),
        "stats": dataset_stats(records),
    }
    (output_dir / "dataset_manifest.json").write_text(
        json.dumps(summary, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
