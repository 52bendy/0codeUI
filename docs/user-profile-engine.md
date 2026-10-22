# 用户画像引擎（EWMA 偏好计算与防抖动）

> 需求：REQ-202610-1176 · 项目：实时UI系统（InTimeUI）· 依赖：REQ-202610-1174（埋点采集）、REQ-202610-1175（偏好调研）

## 1. 目标与范围

把用户的行为信号与显式调研统一收敛为一份稳定的偏好画像，供 Prompt 拼装器与动态生成链路直接使用：

- 偏好维度与 PRD §4.1 对齐：`font_scale`、`density`、`theme`、`radius`、`spacing`；
- 使用 EWMA（指数加权移动平均）让画像随行为序列平滑收敛；
- 三层防抖动：单步限幅、冷启动死区、更新频率控制；
- 冷启动默认画像 + 置信度标记；
- 提供带鉴权、带用户归属校验的读写 API；
- `prompt_profile` 为可直接序列化注入 Prompt 的扁平 JSON。

不在本次范围：前端渲染、埋点采集协议本身、调研问卷 UI、Prompt 模板内容。

## 2. 数据模型

迁移文件：`server/migrations/0004_user_profiles.sql`。

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `user_id` | TEXT PK | 用户标识 |
| `font_scale` | REAL | 字号缩放，`0.8 ~ 1.4`，默认 `1.0` |
| `density` | REAL | 密度，`0.0` 紧凑 ~ `1.0` 宽松，默认 `0.5` |
| `theme_score` | REAL | 主题连续分数，`0` 明亮 ~ `1` 暗黑，默认 `0` |
| `radius` | REAL | 圆角 px，`0 ~ 24`，默认 `8` |
| `spacing` | REAL | 间距缩放，`0.75 ~ 1.5`，默认 `1.0` |
| `confidence` | REAL | 置信度 `0 ~ 1` |
| `sample_count` | INTEGER | 累计参与计算的样本数 |
| `version` | INTEGER | 每次有效更新 +1，从 1 开始 |
| `cold_start` | INTEGER | `1` 表示仍是冷启动默认画像 |
| `created_at` / `updated_at` / `last_observed_at` | TEXT | 时间戳（UTC RFC3339） |

主题在库内保存为连续值，而不是 `light/dark` 枚举。对外按 `theme_score >= 0.5` 映射为 `dark`，否则 `light`，因此单次调研不会让主题瞬间翻转。

## 3. EWMA 算法

对每个维度独立计算：

```text
target = Σ(value_i × weight_i) / Σ(weight_i)
alpha  = clamp(base_alpha × (1 + 0.1 × (n - 1)), 0, max_alpha)
next   = current + clamp((target - current) × alpha, -max_step, +max_step)
```

- `base_alpha` 默认 `0.15`，`max_alpha` 默认 `0.6`；
- 来源权重：埋点 `0.75`，调研 `1.25`（显式选择可信度更高）；
- 单次观测的 `weight` 再与来源权重相乘；
- 缺失维度不参与该维度的加权平均，不会把画像拉回默认值。

### 单步限幅（防止 UI 剧烈抖动）

| 维度 | 单次最大步长 |
| --- | --- |
| `font_scale` | `0.05` |
| `density` | `0.08` |
| `theme_score` | `0.10` |
| `radius` | `2.0 px` |
| `spacing` | `0.06` |

即使输入是极端异常值（例如 24 小时停留、100% 误触），一次更新也最多移动上述步长。

### 冷启动死区

`sample_count == 0` 时，若某维度本次变化小于死区（font `0.002`、density `0.003`、theme `0.004`、radius `0.05`、spacing `0.002`），该维度保持默认值；全部维度都未越过死区时返回 `skipped_reason = "no_change"`，不产生版本变更。

### 更新频率控制

- 两次自动更新至少间隔 `PROFILE_MIN_UPDATE_INTERVAL_SECS`（默认 60 秒）；
- 间隔内调用返回 `updated = false`、`skipped_reason = "rate_limited"`；
- `force = true` 跳过频率控制，供离线补算 / 运维重算使用；
- 显式 `PUT` 不受频率控制（用户主动操作应当立即生效）。

## 4. 冷启动与置信度

冷启动画像：

```json
{
  "font_scale": 1.0,
  "density": 0.5,
  "theme": "light",
  "radius": 8.0,
  "spacing": 1.0,
  "confidence": 0.0,
  "cold_start": true,
  "sample_count": 0,
  "version": 1
}
```

置信度公式：`confidence = 1 - exp(-sample_count / 20)`，随样本数饱和到 1。

| 等级 | 条件 |
| --- | --- |
| `cold_start` | `sample_count = 0` |
| `low` | `confidence < 0.3` |
| `medium` | `0.3 <= confidence < 0.7` |
| `high` | `confidence >= 0.7` |

用户主动 `PUT` 视为 20 条等效样本，保证一次明确设置后置信度不低于 `0.5`。

## 5. 信号映射

### 5.1 埋点事件（1174）

输入：`dwell_time`（毫秒）、`element_click_rate`、`misclick_rate`、`scroll_depth`。

- 所有输入先做区间收敛（停留时长 `0 ~ 120000ms`，比率 `0 ~ 1`）；
- `engagement = click_rate - misclick_rate`；
- 字号：参与度越高、滚动越深，字号略增；
- 密度：参与度越高，密度越低（更紧凑）；
- 主题 / 圆角 / 间距：隐式行为无法可靠推断，保持缺失。

### 5.2 偏好调研（1175）

风格 ID 约定为 `style_<theme>_<density>_<font>_<radius>_<spacing>`，例如：

```text
style_dark_compact_large_font_rounded_wide_spacing
```

| token | 维度目标 |
| --- | --- |
| `dark` / `light` | `theme_score = 1.0 / 0.0` |
| `compact` / `spacious` | `density = 0.15 / 0.85` |
| `large_font` / `small_font` | `font_scale = 1.2 / 0.9` |
| `rounded` / `sharp` / `pill` | `radius = 16 / 4 / 24` |
| `wide_spacing` / `tight_spacing` | `spacing = 1.2 / 0.85` |

选中风格未覆盖的维度，用拒绝风格的反向值补齐。单次调研权重固定为 `1.0`；由于 `theme_score` 单步限幅 `0.10`，连续约 5 次暗色调研后主题才会翻转为 `dark`。

## 6. API 契约

统一响应：`{ "code": 0, "msg": "ok", "data": ... }`；错误码见 README。

鉴权：`Authorization: Bearer <token>`；开发模式支持 `Bearer dev:<user_id>`。
三个端点都校验路径 `user_id` 与凭据归属一致，不一致返回 `403 / 40300`。

### 6.1 读取画像

```http
GET /api/v1/users/{user_id}/profile
```

```json
{
  "code": 0,
  "msg": "ok",
  "data": {
    "user_id": "usr_9527",
    "preferences": {
      "font_scale": 1.0,
      "density": 0.5,
      "theme": "light",
      "radius": 8.0,
      "spacing": 1.0
    },
    "confidence": 0.0,
    "confidence_level": "cold_start",
    "cold_start": true,
    "sample_count": 0,
    "version": 1,
    "updated_at": null,
    "prompt_profile": {
      "font_scale": 1.0,
      "density": 0.5,
      "theme": "light",
      "radius": 8.0,
      "spacing": 1.0,
      "confidence": 0.0,
      "cold_start": true
    }
  }
}
```

### 6.2 主动设置偏好

```http
PUT /api/v1/users/{user_id}/profile
Content-Type: application/json

{ "font_scale": 1.1, "theme": "dark" }
```

字段均可选，但至少提供一个。范围校验失败、空更新、非法 JSON 均返回 `400 / 40000`。

### 6.3 刷新画像

```http
POST /api/v1/users/{user_id}/profile/refresh
Content-Type: application/json

{
  "force": true,
  "window_hours": 24,
  "telemetry": [
    {
      "dwell_time": 30000,
      "element_click_rate": 0.8,
      "misclick_rate": 0.02,
      "scroll_depth": 0.9
    }
  ],
  "surveys": [
    {
      "selected_style_id": "style_dark_compact_large_font_rounded",
      "rejected_style_id": "style_light_spacious_small_font_sharp"
    }
  ]
}
```

- `telemetry` / `surveys` 显式提供时直接使用，不再查库；
- 都不提供时，从 1174 / 1175 的 `telemetry_events`、`telemetry_surveys` 表按 `window_hours` 读取；
- 单次最多处理 `PROFILE_MAX_OBSERVATIONS` 条（默认 1000）；
- 响应：

```json
{
  "code": 0,
  "msg": "ok",
  "data": {
    "updated": true,
    "observed_samples": 2,
    "skipped_reason": null,
    "profile": { "...": "同 6.1 data" }
  }
}
```

`skipped_reason`：`rate_limited` / `no_change` / `no_observations`。

## 7. Prompt 注入

`prompt_profile` 是扁平、稳定的 JSON，可直接放入 Prompt 的“用户偏好”段落：

```text
用户界面偏好（JSON）：
{"cold_start":false,"confidence":0.63,"density":0.45,"font_scale":1.03,"radius":9.4,"spacing":1.03,"theme":"dark"}
```

建议 Prompt 拼装器：
- `cold_start = true` 时不描述具体偏好，只说明“使用默认布局”；
- `confidence < 0.3` 时用弱约束措辞（“倾向”）；
- `confidence >= 0.7` 时用强约束措辞（“必须”）。

## 8. 配置

| 环境变量 | 默认值 | 说明 |
| --- | --- | --- |
| `PROFILE_EWMA_ALPHA` | `0.15` | EWMA 基础平滑系数，`(0, 1]` |
| `PROFILE_MAX_ALPHA` | `0.6` | EWMA 平滑系数上限，`[base, 1]` |
| `PROFILE_MIN_UPDATE_INTERVAL_SECS` | `60` | 自动更新最小间隔（秒） |
| `PROFILE_MAX_OBSERVATIONS` | `1000` | 单次刷新最多样本数，`1..=10000` |

## 9. 安全与降级

- 三个端点均挂在 `auth::require_auth` 之后；路径 `user_id` 必须与凭据一致，防止横向越权；
- 所有 SQL 使用绑定参数，无字符串拼接；
- 输入长度 / 范围 / 枚举全部校验，非法 JSON 统一映射为 `400 / 40000`；
- 依赖表（`telemetry_events` / `telemetry_surveys`）尚未建表时，刷新接口记录告警并返回冷启动结果，不返回 500；
- 画像写库使用 `ON CONFLICT(user_id) DO UPDATE`，天然幂等。

## 10. 测试

- `server/tests/profile_engine.rs`：9 个用例，覆盖冷启动、EWMA 收敛、单次异常限幅、频率控制、死区、调研 token 映射、主题翻转、Prompt JSON、显式写入；
- `server/tests/user_profile.rs`：9 个用例，覆盖鉴权、归属校验、冷启动默认值、显式写入、非法值、显式样本刷新、频率限制、无数据、从 1174 / 1175 表读取；
- `server/src/profile/*` 内含 7 个单元测试，覆盖模型边界、置信度与信号映射。

执行：

```bash
cargo test -p intimeui-server --test profile_engine
cargo test -p intimeui-server --test user_profile
cargo test -p intimeui-server --lib profile
```
