# InTimeUI DPO 离线训练骨架

REQ-202610-1188「偏好对齐数据管道与 DPO 离线训练准备」的 Python 侧实现。
覆盖数据契约校验、数据集切分、训练计划生成、离线评测与门禁判定，以及可选的
LoRA/QLoRA 训练入口。

## 目录

```
training/dpo/
├── configs/dpo_qwen2_5_coder_7b.json   # 训练超参
├── data/seed_pairs.jsonl               # 3 条冒烟样本（非真实用户数据）
├── intimeui_dpo/
│   ├── config.py                       # 配置加载/类型校验
│   ├── dataset.py                      # 统计、稳定切分、写出
│   ├── evaluate.py                     # 指标、门禁、Markdown 报告
│   ├── manifest.py                     # 数据集哈希清单
│   └── schema.py                       # dpo-jsonl-v1 契约
├── scripts/
│   ├── prepare_dataset.py              # 校验导出 → train/eval
│   ├── train_dpo.py                    # 训练计划 / 真实训练
│   ├── evaluate_dpo.py                 # 离线评测
│   └── smoke_pipeline.py               # 无 GPU 端到端冒烟
└── tests/                              # unittest，仅标准库
```

## 数据契约（dpo-jsonl-v1）

服务端 `POST /api/v1/preference/export` 与 CLI `intimeui-preference export`
产出同名 JSONL 与 `.manifest.json`。每行：

```json
{
  "id": "PREF_...",
  "prompt": "用户需求",
  "chosen": "<!DOCTYPE html>...",
  "rejected": "<!DOCTYPE html>...",
  "metadata": {
    "source": "EXPLICIT_SURVEY",
    "scene_code": "ORDER_DETAIL_PAGE",
    "user_hash": "u_<sha256 前 16 位>",
    "quality_score": 0.95,
    "prompt_template_version": "order-detail-v3",
    "chosen_model": "qwen2.5-coder-7b",
    "rejected_model": "qwen2.5-coder-7b-lora-v0",
    "preference_strength": 0.9,
    "pipeline_version": "ai-pipeline-v1",
    "occurred_at": "2026-10-20T09:30:00Z"
  }
}
```

约束：`chosen` 必须是完整 HTML 文档（`<!DOCTYPE html>` 开头），且与
`rejected` 不同；`user_hash` 必须是 `u_` 前缀的加盐哈希；`quality_score`
在 `[0,1]`。校验失败会给出 `record[i].field` 级别的错误。

## 快速开始（无需 GPU）

```bash
cd training/dpo
python3 scripts/prepare_dataset.py \
  --input data/seed_pairs.jsonl \
  --output-dir /tmp/dpo-dataset \
  --eval-ratio 0.34

python3 scripts/train_dpo.py \
  --config configs/dpo_qwen2_5_coder_7b.json \
  --dataset /tmp/dpo-dataset/train.jsonl \
  --eval-dataset /tmp/dpo-dataset/eval.jsonl \
  --output-dir /tmp/dpo-run --dry-run

python3 scripts/evaluate_dpo.py \
  --dataset /tmp/dpo-dataset/eval.jsonl \
  --output-dir /tmp/dpo-eval

python3 scripts/smoke_pipeline.py
```

`smoke_pipeline.py` 串起上述三步并断言门禁通过，适合 CI 与定时巡检。

## 真实训练

```bash
python3 -m venv .venv && . .venv/bin/activate
pip install -r requirements.txt
python3 scripts/train_dpo.py \
  --config configs/dpo_qwen2_5_coder_7b.json \
  --dataset /path/to/train.jsonl \
  --eval-dataset /path/to/eval.jsonl \
  --output-dir outputs/dpo-run
```

- 4bit QLoRA，默认只训练注意力与 MLP 投影层。
- `--max-steps` 可限制步数用于小样本验证。
- 无 CUDA 时脚本会拒绝真实训练；请改用 `--dry-run`。
- 训练前建议先 `prepare_dataset.py --manifest <export>.manifest.json` 校验
  导出文件未被篡改。

## 评测与门禁

`evaluate_dpo.py` 默认使用**占位启发式打分器**，仅用于流水线冒烟；接入真实
模型时通过 `--predictions` 传入每条记录的 `chosen_score` / `rejected_score`。

默认门禁（`intimeui_dpo/evaluate.py`）：

| 指标 | 要求 |
| --- | --- |
| `pair_accuracy` | ≥ 0.55 |
| `html_valid_rate` | ≥ 0.98 |
| `latency_increase_ratio` | ≤ 0.10 |

未提供延迟数据时该项视为不通过，报告会明确标注，避免误判为可上线。

## 定期执行

建议每周离线跑一次，训练与评测分离、产物带哈希：

```cron
# 每周一 03:20（示例，需按实际路径调整）
20 3 * * 1 cd /srv/intimeui/training/dpo && ./run_weekly.sh >> logs/dpo-weekly.log 2>&1
```

训练产物、评测报告与导出清单一起归档；具体节奏与金丝雀流程见
`docs/dpo-rollout-process.md`。

## 隐私

- 导出阶段已完成 PII 脱敏与用户 ID 加盐哈希，训练侧不得再引入原始用户标识。
- `data/seed_pairs.jsonl` 为手工构造的演示数据，不含真实用户内容。
- 数据集与清单按内部数据管理，禁止提交到公开仓库或粘贴到外部服务。
