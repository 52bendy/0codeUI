# 显式偏好调研接口契约（REQ-202610-1175）

> 模块：M2 埋点与画像 ｜ 依赖：REQ-202610-1169（工程基座）、REQ-202610-1174（埋点采集）
> 来源：PRD §3.2、§4.2

## 1. 目标与范围

实现 A/B 偏好反馈采集：客户端上报一次「选择 A 样式、拒绝 B 样式」的调研结果，
服务端完成字段校验、幂等落库，并提供明细查询与聚合能力，供：

- 用户画像引擎（REQ-202610-1176）计算 EWMA 偏好；
- 偏好对齐数据管道（REQ-202610-1188）挖掘 chosen / rejected 样本对。

本需求不实现画像计算与 DPO 训练，只提供可消费的样本与统计。

## 2. 接口

### 2.1 提交调研

`POST /api/v1/telemetry/survey`（需鉴权）

```json
{
  "user_id": "u100",
  "session_id": "sess_001",
  "scene_code": "ORDER_DETAIL_PAGE",
  "selected_style_id": "style_modern_dark",
  "rejected_style_id": "style_classic_light",
  "survey_id": "survey_001",
  "timestamp": "2026-10-22T06:00:00Z"
}
```

| 字段 | 必填 | 约束 |
| --- | --- | --- |
| `user_id` | 否 | 1~64 字符；缺省取鉴权用户，显式提供时必须与鉴权用户一致，否则 403 |
| `session_id` | 否 | 1~128 字符，仅允许 `[A-Za-z0-9._:-]` |
| `scene_code` | 是 | 1~128 字符，同上字符集；与埋点事件的 `page_id` 使用同一场景编码 |
| `selected_style_id` | 是 | 1~128 字符，同上字符集 |
| `rejected_style_id` | 是 | 1~128 字符，同上字符集；必须与 `selected_style_id` 不同 |
| `survey_id` | 否 | 1~64 字符，同上字符集；客户端幂等键，缺省由服务端生成 |
| `timestamp` | 否 | RFC3339；缺省为服务端接收时间；允许「90 天前 ~ 未来 5 分钟」 |

请求体启用 `serde(deny_unknown_fields)`，任何未声明字段（例如页面 HTML、自由文本）
都会被拒绝，保证埋点数据最小化。

成功响应（HTTP 200）：

```json
{
  "code": 0,
  "msg": "ok",
  "data": {
    "accepted": true,
    "duplicate": false,
    "survey": {
      "id": "SURV_0123456789abcdef0123456789abcdef",
      "survey_id": "survey_001",
      "user_id": "u100",
      "session_id": "sess_001",
      "scene_code": "ORDER_DETAIL_PAGE",
      "selected_style_id": "style_modern_dark",
      "rejected_style_id": "style_classic_light",
      "timestamp": "2026-10-22T06:00:00Z",
      "received_at": "2026-10-22T06:00:01Z"
    }
  }
}
```

幂等：同一用户重复提交相同 `survey_id` 时返回 `accepted=false, duplicate=true`，
`survey` 为首次落库记录，不会新增行。

### 2.2 查询明细

`GET /api/v1/telemetry/surveys`（需鉴权）

| 查询参数 | 说明 |
| --- | --- |
| `user_id` | 可选；缺省为当前鉴权用户，显式指定他人返回 403 |
| `session_id` | 可选；按会话过滤 |
| `scene_code` | 可选；按场景过滤 |
| `style_id` | 可选；匹配 `selected_style_id` 或 `rejected_style_id` |
| `from` / `to` | 可选；RFC3339，按 `occurred_at` 闭区间过滤，`from > to` 返回 400 |
| `limit` | 可选；默认 50，范围 1~200 |
| `offset` | 可选；默认 0，必须 ≥ 0 |

响应：

```json
{
  "code": 0,
  "msg": "ok",
  "data": {
    "items": [
      {
        "id": "SURV_...",
        "survey_id": "survey_001",
        "user_id": "u100",
        "session_id": "sess_001",
        "scene_code": "ORDER_DETAIL_PAGE",
        "selected_style_id": "style_modern_dark",
        "rejected_style_id": "style_classic_light",
        "timestamp": "2026-10-22T06:00:00Z",
        "received_at": "2026-10-22T06:00:01Z"
      }
    ],
    "total": 1,
    "limit": 50,
    "offset": 0
  }
}
```

排序：`occurred_at DESC, id DESC`。

### 2.3 聚合样本

`GET /api/v1/telemetry/surveys/aggregate`（需鉴权）

支持 `user_id` / `scene_code` / `from` / `to` 过滤（`style_id` 对聚合无意义，忽略）。

```json
{
  "code": 0,
  "msg": "ok",
  "data": {
    "user_id": "u100",
    "from": null,
    "to": null,
    "total_surveys": 3,
    "styles": [
      {
        "style_id": "style_a",
        "selected_count": 2,
        "rejected_count": 1,
        "net_preference": 1
      }
    ],
    "scenes": [
      { "scene_code": "SCENE_X", "survey_count": 2 }
    ],
    "pairs": [
      {
        "scene_code": "SCENE_X",
        "selected_style_id": "style_a",
        "rejected_style_id": "style_b",
        "count": 2
      }
    ]
  }
}
```

- `styles` 按 `net_preference DESC, selected_count DESC, style_id ASC` 排序；
- `scenes` 按 `survey_count DESC, scene_code ASC` 排序；
- `pairs` 按 `count DESC` 排序，最多返回 100 条。

## 3. 错误码

| code | HTTP | 场景 |
| --- | --- | --- |
| `40000` | 400 | 字段缺失 / 长度或字符集非法 / selected 与 rejected 相同 / 时间越界 / 分页参数非法 |
| `40100` | 401 | 缺少或无效凭据 |
| `40300` | 403 | `user_id` 与鉴权用户不一致，或查询他人数据 |
| `42900` | 429 | 触发限流 |
| `50000` | 500 | 内部错误 |

## 4. 存储与关联

迁移：`server/migrations/0003_telemetry_surveys.sql`

| 列 | 类型 | 说明 |
| --- | --- | --- |
| `id` | TEXT PK | 服务端主键 `SURV_<uuid>` |
| `survey_id` | TEXT | 客户端幂等键 |
| `user_id` | TEXT | 用户标识 |
| `session_id` | TEXT NULL | 会话标识 |
| `scene_code` | TEXT | 场景编码 |
| `selected_style_id` | TEXT | 选择的样式 |
| `rejected_style_id` | TEXT | 拒绝的样式 |
| `occurred_at` | TEXT | 调研发生时间 |
| `received_at` | TEXT | 服务端接收时间 |

约束：`UNIQUE (user_id, survey_id)`、`CHECK (selected_style_id <> rejected_style_id)`。

与埋点事件（`telemetry_events`，REQ-202610-1174）的关联维度：

- `user_id` ↔ `telemetry_events.user_id`；
- `scene_code` ↔ `telemetry_events.page_id`（同一场景编码）；
- `occurred_at` ↔ `telemetry_events.occurred_at`（同一时间轴，可按时间窗口对齐）；
- `session_id` 用于同一会话内的调研归集；事件侧后续补充会话字段后可直接按会话 join。

## 5. 测试与验证

- 单元测试（`server/src/telemetry_survey.rs`）：标识符字符集、时间窗、未知字段、
  必填字段、序列化字段名；
- 集成测试（`server/tests/telemetry_survey.rs`）：提交落库与查询、未认证 401、
  用户不一致 403、selected == rejected 400、非法字符 400、时间越界 400、
  `survey_id` 幂等、用户隔离、过滤 / 分页、聚合统计；
- 静态检查：`cargo fmt --all -- --check`、
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`；
- 全量测试：`cargo test --workspace --all-features --locked`。

## 6. 合并说明

- 需求分支：`feat/req-202610-1175-telemetry-survey`；
- 基于 `feat/req-202610-1170-data-model`；
- 迁移编号 0003 预留 1174 的 `0002_telemetry_events.sql`，合并顺序：1174 → 1175；
- 若 1174 未合并，本分支可独立运行（0001 + 0003）。
