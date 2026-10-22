# 偏好对齐数据管道与 DPO 离线训练准备（REQ-202610-1188）

本文描述 M6 模型训练线的数据侧实现：把显式调研、隐式反馈与 A/B 对比统一为
`<Prompt, Chosen_HTML, Rejected_HTML>` 偏好对，完成清洗、去偏、溯源与导出，
并给出 DPO 离线训练的脚本骨架与验收方式。

- 代码位置：`server/src/preference/`、`server/src/bin/intimeui-preference.rs`、
  `training/dpo/`
- 迁移：`server/migrations/0005_preference_pairs.sql`
- 上游依赖：REQ-202610-1175（偏好调研）、REQ-202610-1181（模型网关）、
  REQ-202610-1185（AI 生成管线）、REQ-202610-1176（用户画像）

## 1. 总体架构

```
┌──────────────────────┐   ┌──────────────────────┐   ┌──────────────────────┐
│ 显式调研（07）        │   │ 隐式信号              │   │ A/B 对比              │
│ POST .../from-survey │   │ POST .../pairs       │   │ POST .../pairs       │
└──────────┬───────────┘   └──────────┬───────────┘   └──────────┬───────────┘
           └──────────────────────────┼──────────────────────────┘
                                      ▼
                        ┌──────────────────────────┐
                        │ 清洗 / 脱敏 / 质量评分     │  cleaning.rs
                        │ 去重（pair_key/content）  │
                        └────────────┬─────────────┘
                                     ▼
                        ┌──────────────────────────┐
                        │ preference_pairs（SQLite）│  待审 / 通过 / 拒绝 / 已导出
                        └────────────┬─────────────┘
                    人工复核（review）│  抽样（sample，按场景分层）
                                     ▼
                        ┌──────────────────────────┐
                        │ 导出 dpo-jsonl-v1 + 清单   │  export / CLI
                        └────────────┬─────────────┘
                                     ▼
                        ┌──────────────────────────┐
                        │ DPO 训练骨架（离线周期）    │  training/dpo/
                        └──────────────────────────┘
```

## 2. 数据来源与配对规则

| 来源 | `source` | 产生方式 | 说明 |
| --- | --- | --- | --- |
| 显式偏好调研 | `EXPLICIT_SURVEY` | `POST /api/v1/preference/pairs/from-survey` | 读取 REQ-202610-1175 的调研记录，自动继承场景、会话、风格与时间 |
| 隐式反馈 | `IMPLICIT_SIGNAL` | `POST /api/v1/preference/pairs` | 停留、重生成、编辑、采纳等信号由上层判定 chosen/rejected 后提交 |
| A/B 对比 | `AB_TEST` | 同上 | 同 Prompt 下两个模型/两个模板版本的输出对比 |
| 人工构造 | `MANUAL` | 同上 | 冷启动或评测集补充 |

配对约束：

- `chosen_html` 与 `rejected_html` 必须不同（数据库 CHECK + 服务层双重校验）；
- `chosen_html` 必须通过单文件 HTML 校验（完整文档、无外链依赖、安全基线）；
- `rejected_html` 允许不合法，但会打 `rejected_html_invalid` 标记并进入人工复核，
  避免把“坏样本”直接当成负例；
- 同一 `pair_key` 幂等：重复提交返回已有记录，不重复入库。

`from-survey` 只接受当前用户名下、且已存在的调研记录；调研 ID 不存在返回
`40400`，避免跨用户伪造溯源。

## 3. 存储模型

`preference_pairs` 关键字段：

| 字段 | 说明 |
| --- | --- |
| `id` / `pair_key` / `content_hash` | 主键、幂等键（来源+场景+内容哈希）、内容去重哈希 |
| `prompt` / `chosen_html` / `rejected_html` | 训练三元组 |
| `source` / `scene_code` | 来源与场景 |
| `user_id` / `session_id` / `request_id` / `survey_id` | 溯源链 |
| `prompt_template_version` / `chosen_model` / `rejected_model` | 模板与模型版本 |
| `chosen_style_id` / `rejected_style_id` / `pipeline_version` | 风格与管线版本 |
| `preference_strength` / `quality_score` / `quality_flags` | 偏好强度、质量分、清洗标记（JSON 数组） |
| `metadata` | 扩展溯源信息（JSON，已脱敏） |
| `status` / `review_note` / `reviewed_by` / `reviewed_at` / `exported_at` | 审核与导出状态 |

状态机：`PENDING → ACCEPTED / REJECTED`，导出后 `ACCEPTED → EXPORTED`。
`REJECTED` 为终态，不参与导出。

`preference_exports` 记录每次导出的 `dataset_id`、格式、条数、SHA-256、过滤条件
与完整清单，便于复现与审计。

## 4. API 契约

鉴权：`Authorization: Bearer <token>`。管理端接口额外要求
`X-Preference-Token: <PREFERENCE_ADMIN_TOKEN>`；未配置令牌时仅 dev 环境放行。

| 方法 | 路径 | 鉴权 | 说明 |
| --- | --- | --- | --- |
| POST | `/api/v1/preference/pairs` | 用户 | 提交偏好对（幂等） |
| POST | `/api/v1/preference/pairs/from-survey` | 用户 | 从调研记录生成偏好对 |
| GET | `/api/v1/preference/pairs` | 管理端 | 分页查询（状态/来源/场景/用户/质量/时间） |
| GET | `/api/v1/preference/pairs/{id}` | 管理端 | 查看单条 |
| POST | `/api/v1/preference/pairs/{id}/review` | 管理端 | 人工通过 / 拒绝 |
| GET | `/api/v1/preference/stats` | 管理端 | 总量、状态、来源、场景、平均质量 |
| GET | `/api/v1/preference/sample` | 管理端 | 确定性分层抽样（seed 可复现） |
| POST | `/api/v1/preference/export` | 管理端 | 导出 JSONL + 清单（默认 dry-run） |

### 4.1 提交偏好对

```json
{
  "prompt": "生成订单详情页，移动端优先",
  "chosen_html": "<!DOCTYPE html>...",
  "rejected_html": "<!DOCTYPE html>...",
  "source": "AB_TEST",
  "scene_code": "ORDER_DETAIL_PAGE",
  "session_id": "sess_001",
  "request_id": "req_001",
  "prompt_template_version": "order-detail-v3",
  "chosen_model": "qwen2.5-coder-7b",
  "rejected_model": "qwen2.5-coder-7b-lora-v0",
  "chosen_style_id": "minimal",
  "rejected_style_id": "dense",
  "pipeline_version": "ai-pipeline-v1",
  "preference_strength": 0.8,
  "metadata": {"experiment": "ab-2026-10"},
  "occurred_at": "2026-10-20T09:30:00Z"
}
```

响应 `data`：

```json
{
  "accepted": true,
  "duplicate": false,
  "pair": {"id": "PREF_...", "status": "ACCEPTED", "quality_score": 0.92, "quality_flags": []}
}
```

### 4.2 从调研生成

```json
{
  "survey_id": "SRV_...",
  "prompt": "生成商品列表页",
  "chosen_html": "<!DOCTYPE html>...",
  "rejected_html": "<!DOCTYPE html>...",
  "preference_strength": 0.9
}
```

场景、会话、风格与时间从调研记录继承；`source` 固定为 `EXPLICIT_SURVEY`。

### 4.3 导出

```json
{
  "limit": 5000,
  "min_quality": 0.5,
  "max_per_user": 50,
  "max_per_scene": 1000,
  "scene_code": null,
  "source": "EXPLICIT_SURVEY",
  "include_pending": false,
  "dry_run": true
}
```

导出只读取 `ACCEPTED`（`include_pending=true` 时含 `PENDING`），并按
`content_hash` 去重、按用户与场景限额；默认 `dry_run=true`，不会把记录标记为
`EXPORTED`。响应包含 `records`、`manifest` 与 `skipped` 统计。

## 5. 清洗与去偏规则

实现在 `server/src/preference/cleaning.rs`，全部为纯函数，便于单测与复现。

1. **规范化**：统一 CRLF、去除 NUL、首尾空白；Prompt 超过 8000 字符拒绝，
   HTML 超过 256 KiB 拒绝，metadata 超过 8 KiB 拒绝。
2. **PII 脱敏**：Prompt 与 metadata 递归替换邮箱、中国大陆手机号为占位符，
   打 `pii_redacted` 标记；`user_id` 在导出时加盐哈希，不进入训练数据。
3. **HTML 校验**：chosen 必须通过 `validate_single_file_html`；rejected 不合法时
   打 `rejected_html_invalid`，不直接淘汰，交由人工判断。
4. **质量评分**（0–1）：
   - 0.40 chosen 合法
   - 0.20 rejected 合法
   - 0.15 Prompt 长度 ≥ 12
   - 0.15 溯源完整（场景 + 模板版本 + chosen/rejected 模型）
   - 0.10 × `preference_strength`
5. **状态决策**：
   - `quality < PREFERENCE_MIN_QUALITY` → `REJECTED`（`review_note` 以
     `auto-rejected:` 开头，便于统计）；
   - `quality ≥ PREFERENCE_AUTO_ACCEPT_QUALITY` 且无阻断标记
     （`rejected_html_invalid` / `missing_provenance`）→ `ACCEPTED`；
   - 其余 → `PENDING` 人工复核。
6. **去重**：`pair_key` 幂等入库；导出阶段再按 `content_hash` 去重，防止同一
   内容换来源重复计数。
7. **配额去偏**：单用户最多 `PREFERENCE_EXPORT_MAX_PER_USER` 条、单场景最多
   `PREFERENCE_EXPORT_MAX_PER_SCENE` 条，避免高频用户/热门场景主导训练集。
8. **分层抽样**：`GET /sample` 按 `sha256(seed:id)` 排序后轮转各场景，保证
   小场景不被大场景淹没，seed 相同结果可复现。

## 6. 导出格式与清单

格式 `dpo-jsonl-v1`，每行一条：

```json
{"id":"PREF_...","prompt":"...","chosen":"<!DOCTYPE html>...","rejected":"<!DOCTYPE html>...","metadata":{"source":"EXPLICIT_SURVEY","scene_code":"ORDER_DETAIL_PAGE","user_hash":"u_5f2c9a1b7d3e4c60","quality_score":0.95,"prompt_template_version":"order-detail-v3","chosen_model":"...","rejected_model":"...","preference_strength":0.9,"pipeline_version":"...","occurred_at":"..."}}
```

清单（`.manifest.json`）字段：`dataset_id`、`format`、`created_at`、
`record_count`、`source_counts`、`scene_counts`、`unique_user_count`、
`sha256`、`user_hash_fingerprint`、`filters`、`skipped`。

`user_hash` 由 `sha256(salt + ":" + user_id)` 取前 16 位并加 `u_` 前缀；
`session_hash` 同理。盐值只存在于服务端环境变量，导出文件无法反查用户。

### 6.1 CLI

```bash
# 统计
intimeui-preference stats --database-url sqlite://data/intimeui.db

# 抽样（默认 20 条，可复现）
intimeui-preference sample --limit 20 --seed inspect-2026-10 --scene-code ORDER_DETAIL_PAGE

# 导出（写文件后才标记 EXPORTED；--dry-run 只预览）
intimeui-preference export \
  --output data/dpo/export.jsonl \
  --limit 5000 \
  --min-quality 0.5 \
  --max-per-user 50 \
  --max-per-scene 1000

# 预览
intimeui-preference export --output /tmp/preview.jsonl --dry-run
```

CLI 与服务端共用同一套 `PreferenceService`，导出结果与 API 完全一致。

## 7. 环境变量

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `PREFERENCE_ADMIN_TOKEN` | 空 | 管理端令牌；prod 必须 ≥16 字符 |
| `PREFERENCE_MIN_QUALITY` | `0.5` | 低于该分自动拒绝 |
| `PREFERENCE_AUTO_ACCEPT_QUALITY` | `0.8` | 达到该分且无阻断标记自动通过 |
| `PREFERENCE_MAX_PROMPT_CHARS` | `8000` | Prompt 长度上限 |
| `PREFERENCE_MAX_HTML_BYTES` | `262144` | 单侧 HTML 字节上限 |
| `PREFERENCE_MAX_METADATA_BYTES` | `8192` | metadata 字节上限 |
| `PREFERENCE_EXPORT_MAX_RECORDS` | `5000` | 单次导出上限 |
| `PREFERENCE_EXPORT_MAX_PER_USER` | `50` | 单用户导出上限 |
| `PREFERENCE_EXPORT_MAX_PER_SCENE` | `1000` | 单场景导出上限 |
| `PREFERENCE_USER_HASH_SALT` | dev 默认值 | 用户哈希盐；prod 必须 ≥16 字符 |

## 8. 验收自检

```bash
# 1. 服务端集成测试（17 项：入库、幂等、PII、调研、审核、统计、抽样、导出、鉴权）
cargo test -p intimeui-server --test preference_pipeline

# 2. Python 数据侧测试（21 项：契约、清单、切分、端到端冒烟）
python3 -m unittest discover -s training/dpo/tests -t training/dpo -v

# 3. 无 GPU 冒烟：校验 → 切分 → 训练计划 → 评测门禁
python3 training/dpo/scripts/smoke_pipeline.py
```

验收对照：

- **可产出合规偏好数据集**：导出包含 `metadata` 全链路溯源、用户/会话哈希、
  质量分与清单哈希；抽样接口可按场景分层复核。
- **DPO 脚本小样本跑通**：`train_dpo.py --dry-run` 输出训练计划与数据哈希；
  `smoke_pipeline.py` 在 3 条种子样本上完整跑通并产出评测报告。
- **上线流程文档**：见 [`dpo-rollout-process.md`](dpo-rollout-process.md)。

## 9. 隐私与合规

- 训练数据不包含原始 `user_id`、会话 ID、请求 ID；仅保留加盐哈希与版本信息。
- Prompt 与 metadata 在入库前完成 PII 脱敏，原始内容不落训练库。
- 导出文件按内部数据管理，禁止提交仓库、上传公开网盘或粘贴到外部服务。
- 删除用户数据时，按 `user_id` 删除 `preference_pairs` 对应记录；已导出数据集
  需按清单 `dataset_id` 定位并重新生成。
- 管理端令牌与哈希盐仅通过环境变量/Secret 注入，不写入 `.env.example` 之外的
  任何文件。

## 10. 已知限制

- 隐式信号到 chosen/rejected 的判定由上层业务规则提供，本模块只负责接收与
  质检，不推断用户意图。
- 质量分为规则加权，不是学习得到的奖励模型；后续可用人工复核结果校准。
- `include_pending=true` 仅用于内部预览，正式训练必须使用审核通过的记录。
