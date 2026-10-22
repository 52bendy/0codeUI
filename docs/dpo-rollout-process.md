# DPO 训练与灰度上线流程

适用对象：InTimeUI M6 模型训练线（REQ-202610-1188）。
目标：把偏好数据 → DPO 训练 → 离线评测 → 金丝雀发布串成可重复、可回滚的固定节奏，
任何一步不达标都不得进入下一步。

> 文档状态：**待项目负责人评审**。评审通过前，本文只作为流程草案，不得据此执行
> 生产灰度。

## 1. 原则

1. **数据先行**：没有通过清单校验与抽样复核的数据集，不启动训练。
2. **离线可复现**：每次训练必须绑定 `dataset_id`、数据 SHA-256、配置与代码 commit。
3. **门禁硬约束**：离线指标不达标直接终止，不做“先上线再观察”。
4. **小流量验证**：金丝雀按 5% → 25% → 100% 三阶段放量，每阶段独立观察窗口。
5. **一键回滚**：任何阶段触发红线，先回滚再排查，不带着问题调参。

## 2. 角色与节奏

| 角色 | 职责 |
| --- | --- |
| 数据负责人 | 导出、抽样复核、清单归档 |
| 训练负责人 | 训练执行、产物与指标归档 |
| 项目负责人 | 评审评测报告、批准进入金丝雀与放量 |
| 值班/运维 | 灰度观察、回滚执行、线上指标核对 |

固定节奏（建议）：

| 时间 | 动作 |
| --- | --- |
| 每周一 03:20 | 导出上周偏好数据（离线任务） |
| 周一上午 | 数据抽样复核（≥30 条/场景） |
| 周一下午 | DPO 训练（GPU 空闲窗口） |
| 周二 | 离线评测 + 报告评审 |
| 周三 | 金丝雀 5% |
| 周四 | 25%（5% 阶段达标后） |
| 周五 | 100%（25% 阶段达标后）或回滚 |

非每周强制：数据量不足 200 条或新增场景样本 < 30 条时，跳过本周训练并在周报
注明原因。

## 3. 阶段 0：数据准备

1. 执行导出（默认 dry-run 预览）：

   ```bash
   intimeui-preference export \
     --output data/dpo/export-$(date +%Y%m%d).jsonl \
     --limit 5000 --min-quality 0.5 \
     --max-per-user 50 --max-per-scene 1000
   ```

2. 校验清单：`record_count`、`sha256`、`source_counts`、`unique_user_count`
   与数据库统计一致；清单随数据集一起归档。
3. 抽样复核：`intimeui-preference sample --limit 30 --seed <日期>`，
   人工检查 chosen 确实优于 rejected；抽样通过率 < 90% 时停止训练，回到清洗规则。
4. 数据量门槛：训练集 ≥ 200 条、评测集 ≥ 50 条；单一场景占比不得超过 60%。

## 4. 阶段 1：训练

```bash
# 1) 校验并切分
python3 training/dpo/scripts/prepare_dataset.py \
  --input data/dpo/export-20261026.jsonl \
  --manifest data/dpo/export-20261026.manifest.json \
  --output-dir data/dpo/prepared-20261026 --eval-ratio 0.1

# 2) 计划检查（无 GPU）
python3 training/dpo/scripts/train_dpo.py \
  --config training/dpo/configs/dpo_qwen2_5_coder_7b.json \
  --dataset data/dpo/prepared-20261026/train.jsonl \
  --eval-dataset data/dpo/prepared-20261026/eval.jsonl \
  --output-dir outputs/dpo-20261026 --dry-run

# 3) 真实训练
python3 training/dpo/scripts/train_dpo.py \
  --config training/dpo/configs/dpo_qwen2_5_coder_7b.json \
  --dataset data/dpo/prepared-20261026/train.jsonl \
  --eval-dataset data/dpo/prepared-20261026/eval.jsonl \
  --output-dir outputs/dpo-20261026
```

产物必须包含：`training_plan.json`、adapter 权重、tokenizer、训练日志、配置快照、
数据清单与 SHA-256。产物目录按 `dpo-<日期>-<dataset_id>` 命名，禁止覆盖历史版本。

## 5. 阶段 2：离线评测

```bash
python3 training/dpo/scripts/evaluate_dpo.py \
  --dataset data/dpo/prepared-20261026/eval.jsonl \
  --predictions outputs/dpo-20261026/predictions.jsonl \
  --latency-increase-ratio 0.04 \
  --output-dir outputs/dpo-20261026/eval
```

`predictions.jsonl` 由评测推理生成，每条含 `id`、`chosen_score`、`rejected_score`。
未提供时脚本使用占位启发式打分，报告会标记 `scorer=heuristic-placeholder`，
**不得用于上线决策**。

### 5.1 离线门禁

| 指标 | 阈值 | 说明 |
| --- | --- | --- |
| `pair_accuracy` | ≥ 0.55 | chosen 得分高于 rejected 的比例 |
| `html_valid_rate` | ≥ 0.98 | chosen 完整 HTML 文档比例 |
| `latency_increase_ratio` | ≤ 0.10 | P95 延迟相对基线的增幅 |
| 场景覆盖 | 每个上线场景 ≥ 20 条评测样本 | 不足则该场景不参与灰度 |

任一不通过 → 终止，输出报告归档，不进入金丝雀。

### 5.2 评审记录

| 项目 | 内容 |
| --- | --- |
| 数据集 | `dataset_id` / SHA-256 / 条数 |
| 配置 | 配置文件路径 + 代码 commit |
| 离线指标 | pair_accuracy / html_valid_rate / latency |
| 抽样复核 | 抽样 seed、条数、通过率 |
| 结论 | 通过 / 不通过 |
| 评审人 | 待填写 |
| 日期 | 待填写 |

> 当前状态：流程与模板已就绪，**评审人签字待项目负责人完成**。

## 6. 阶段 3：金丝雀发布

### 6.1 放量节奏

| 阶段 | 流量 | 最短观察窗口 | 进入下一阶段条件 |
| --- | --- | --- | --- |
| Canary-1 | 5% | 24 小时 | 全部在线门禁达标 |
| Canary-2 | 25% | 48 小时 | 全部在线门禁达标，且无 P1/P2 事故 |
| Full | 100% | — | Canary-2 通过 |

放量通过模型网关的灰度能力（`MODEL_CANARY_MODEL` / `MODEL_CANARY_PERCENT`）执行，
按用户 ID 哈希稳定分流，同一用户在同一阶段内不来回切换。

### 6.2 在线门禁

| 指标 | 阈值 | 数据来源 |
| --- | --- | --- |
| 用户采纳率 / 偏好胜率 | ≥ 55%，且不低于基线 | 埋点 + 调研 |
| 非法 HTML 率 | ≤ 基线，且绝对值 ≤ 0.5% | 生成管线校验日志 |
| P95 生成延迟 | ≤ 基线 × 1.10 | 模型网关指标 |
| 5xx / 超时率 | ≤ 基线，且 ≤ 0.1% | 服务指标 |
| 单请求成本 | ≤ 基线 × 1.05 | token 计费 |

### 6.3 回滚红线

出现任一情况立即回滚到上一稳定模型，再排查：

- 非法 HTML 率超过基线 2 倍，或连续 10 分钟 > 1%；
- P95 延迟超过基线 30%，持续 10 分钟；
- 5xx / 超时率超过基线 2 倍，持续 5 分钟；
- 用户采纳率下降超过 5 个百分点（置信度足够时）；
- 出现数据泄露、越权、提示注入导致的安全事件。

回滚操作：把 `MODEL_CANARY_PERCENT` 置 0 并恢复主模型配置，保留现场日志与
请求样本；30 分钟内完成事故记录，24 小时内给出根因与改进项。

## 7. 归档与复盘

每次发布归档：

- 数据集与清单（含 SHA-256）
- 训练配置、代码 commit、随机种子
- 离线报告（JSON + Markdown）
- 金丝雀各阶段指标快照
- 评审记录与回滚记录（如有）

每月复盘一次：数据分布漂移、门禁阈值是否仍合理、回滚次数与原因。

## 8. 检查清单（发布前逐项勾选）

- [ ] 数据集清单校验通过，抽样复核 ≥ 90%
- [ ] 训练产物与配置、commit 绑定
- [ ] 离线三项门禁全部通过
- [ ] 评测报告已由项目负责人评审签字
- [ ] 灰度开关与回滚脚本已验证
- [ ] 监控看板包含 5.2 全部指标
- [ ] 值班人已知晓观察窗口与回滚红线

> 说明：以上为流程文档，当前代码仓库不包含真实 GPU 训练与线上灰度开关的
> 自动化执行；金丝雀阶段需由运维按模型网关现有灰度能力操作。
