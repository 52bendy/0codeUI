# AI 生成管线编排与降级兜底（REQ-202610-1185）

> 目标：把「03 接口 → 画像/数据 → 14 Prompt → 13 模型 → 05 校验 → 16 混淆 → 返回」
> 串成一条可观测、可兜底的动态生成链路；端到端预算默认 1500ms，任何一步超时或失败
> 都在秒级切回静态模板，客户端不出现白屏。

## 1. 架构与链路

```
POST /api/v1/ui/generate/dynamic
        │
        ▼
┌─────────────────────────────────────────────────────────────┐
│ Pipeline::generate                                          │
│                                                             │
│ 1. 场景注册表校验（scene_code / device / business_data）      │
│ 2. 预渲染静态兜底页（TemplateEngine::render_fallback）        │
│ 3. 加载用户画像（EWMA，冷启动降级为默认画像）                  │
│ 4. PromptAssembler 拼装 System Prompt（版本化 / 脱敏）        │
│ 5. 缓存查询（命中则跳过 6，直接 7）                            │
│ 6. ModelGateway::generate_ui_with_system（并发 / 超时 / 重试） │
│ 7. validate_single_file_html（结构 + 单文件 + 安全基线）       │
│ 8. ObfuscationEngine 输出混淆（REQ-202610-1184）              │
│ 9. 记录指标 / 结构化日志，返回 PipelineOutcome                 │
└─────────────────────────────────────────────────────────────┘
        │ 任一步失败 / 超时
        ▼
静态模板兜底（strategy=template, degraded=true）
```

代码位置：

| 模块 | 职责 |
| --- | --- |
| `server/src/pipeline/orchestrator.rs` | 编排、时间预算、降级决策、版本串 |
| `server/src/pipeline/config.rs` | `PIPELINE_*` 配置与预算计算 |
| `server/src/pipeline/cache.rs` | TTL + FIFO 响应缓存 |
| `server/src/pipeline/metrics.rs` | 成功率 / 降级原因 / 延迟分位 |
| `server/src/pipeline/obfuscation.rs` | 混淆抽象与 `EngineObfuscator` 适配器 |
| `server/src/routes/pipeline.rs` | 动态生成、SSE 流式、指标与运行信息端点 |

## 2. 时间预算

| 预算项 | 默认值 | 说明 |
| --- | --- | --- |
| `PIPELINE_TIMEOUT_MS` | 1500 | 端到端硬预算（PRD §6.1） |
| `PIPELINE_FALLBACK_RESERVE_MS` | 50 | 预留给静态模板兜底的时间 |
| 动态阶段预算 | 1450 | `timeout - reserve`，模型调用与排队共用 |

模型调用使用 `tokio::time::timeout` 包住「排队 + 推理 + 重试」的完整 future；
超时后不再等待模型，直接返回已预渲染的兜底页。模型返回后还会再检查一次总预算，
避免校验 / 混淆阶段把总耗时推过 1500ms。

## 3. 降级矩阵

| `degradation_reason` | 触发条件 | 结果 |
| --- | --- | --- |
| `pipeline_disabled` | `PIPELINE_ENABLED=false` | 静态模板 |
| `model_timeout` | 动态阶段预算耗尽 | 静态模板 |
| `model_busy` | 网关队列满 / 排队超时 | 静态模板 |
| `model_disabled` | `MODEL_ENABLED=false` | 静态模板 |
| `model_error` | 上游 4xx/5xx、协议错误 | 静态模板 |
| `deadline_exceeded` | 模型返回后总预算已耗尽 | 静态模板 |
| `validation_failed` | 单文件 HTML 校验不通过 | 静态模板 |
| `obfuscation_failed` | 混淆失败或输出复检失败 | 静态模板 |

兜底页在模型调用**之前**完成渲染与业务数据校验，因此降级路径只做内存拷贝，
耗时在毫秒级；即使模型卡死也不会拖过总预算。

## 4. 流式输出（首屏优先）

`POST /api/v1/ui/generate/dynamic/stream` 返回 SSE：

| 事件 | 时机 | 载荷 |
| --- | --- | --- |
| `fallback` | 连接建立后立即 | `{preview: {render_type, strategy:"template", version, html_content, render_ms}}` |
| `result` | 动态生成成功 | `{outcome: PipelineOutcome}`（`degraded=false`） |
| `degraded` | 动态生成失败 | `{outcome: PipelineOutcome}`（`degraded=true`，HTML 为兜底页） |
| `error` | 兜底页已下发后的内部错误 | `{code, message}` |

客户端先渲染 `fallback`，收到 `result` 后用动态页替换；收到 `degraded` 则保留兜底页。
这样首屏感知时间等于模板渲染耗时（本机实测 0.616ms），而不是模型生成时间。
SSE 的 token 级流式能力仍由 `POST /api/v1/ui/model/stream` 提供（REQ-202610-1181）。

## 5. 缓存

- 缓存内容：**已通过校验、尚未混淆**的模型输出；命中后重新混淆，避免固定指纹。
- 缓存键：`scene_code + user_id + device + prompt_hash + business_data + model + profile_version`，
  经 FNV-1a 64 位哈希；`prompt_hash` 由 Prompt 拼装器给出，画像变化会产生新键。
- TTL 默认 300s，容量默认 256，按插入顺序 FIFO 淘汰；`bypass_cache=true` 可强制穿透。
- 命中时 `cache_hit=true`，`attempts=0`，不调用模型。

## 6. 版本策略

| 路径 | 版本串 | 说明 |
| --- | --- | --- |
| 动态成功 | `dynamic:<scene>:v1-<prompt_hash 前 8 位>` | Prompt / 画像 / 业务数据变化都会改变指纹 |
| 静态兜底 | `template:<scene>:<template_version>` | 与 REQ-202610-1172 模板版本一致 |

`version` 随响应返回，并在 `pipeline_outcome` 日志中记录，便于把线上页面回溯到具体生成输入。

## 7. 可观测性

`GET /api/v1/ui/pipeline/metrics`：

```json
{
  "total_requests": 100,
  "dynamic_success": 92,
  "degraded_requests": 8,
  "cache_hits": 31,
  "cache_misses": 69,
  "fallback_failures": 0,
  "degradation_reasons": [{"reason": "model_timeout", "count": 6}],
  "avg_latency_ms": 42.1,
  "p50_latency_ms": 31.2,
  "p95_latency_ms": 118.4,
  "max_latency_ms": 251.6,
  "budget_ms": 1500,
  "within_budget": 100,
  "within_budget_rate": 1.0
}
```

每次请求输出一条 `pipeline_outcome` 结构化日志，字段包括：`scene_code`、`strategy`、
`version`、`degraded`、`degradation_reason`、`degradation_detail`、`cache_hit`、`model`、
`total_ms`、`model_ms`、`validation_ms`、`obfuscation_ms`。

## 8. 验收测量报告

测量环境：本机 Linux，`cargo test` debug 构建，mock provider（注入 20ms 推理延迟），
场景 `ORDER_DETAIL_PAGE`，业务数据为 PRD §5.1 示例订单。

| 场景 | 预算 | 实测 | 结果 |
| --- | --- | --- | --- |
| 正常路径 | 1500ms | **28.778ms**（模型 22.235 / 校验 0.854 / 混淆 2.791） | 通过 |
| 模型超时降级 | 300ms（演练配置） | **251.56ms**，`degradation_reason=model_timeout`，HTML 完整 | 通过 |
| 流式首屏 fallback | 1500ms | **0.616ms** 下发兜底页；动态结果总计 29.868ms | 通过 |

复现命令：

```bash
cargo test -p intimeui-server --test pipeline_api -- --nocapture
```

> 生产环境的模型推理延迟需按 `docs/model-latency-baseline.md` 在目标 GPU 上复测；
> 超过动态预算时管线会按上述矩阵自动降级，端到端仍受 1500ms 硬预算约束。

## 9. 配置

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `PIPELINE_ENABLED` | `true` | 关闭后全部走静态模板 |
| `PIPELINE_TIMEOUT_MS` | `1500` | 端到端预算 |
| `PIPELINE_FALLBACK_RESERVE_MS` | `50` | 兜底预留 |
| `PIPELINE_CACHE_ENABLED` | `true` | 响应缓存开关 |
| `PIPELINE_CACHE_TTL_SECS` | `300` | 缓存 TTL |
| `PIPELINE_CACHE_MAX_ENTRIES` | `256` | 缓存容量 |
| `PIPELINE_OBFUSCATION_LEVEL` | `standard` | 管线内混淆开关与强度（`off` / `basic` / `standard`）；实际强度由 `UI_OBFUSCATION_LEVEL` 决定 |

## 10. 接口示例

```bash
# 非流式
curl -s -X POST http://127.0.0.1:8080/api/v1/ui/generate/dynamic \
  -H 'Authorization: Bearer dev:9527' \
  -H 'Content-Type: application/json' \
  -d '{
    "user_id": "9527",
    "scene_code": "ORDER_DETAIL_PAGE",
    "device_info": {"screen_width": 390, "screen_height": 844, "platform": "android"},
    "business_data": {"order_id": "ORD_20261014_001", "status": "PAID", "items": []}
  }'

# 流式（SSE）
curl -N -X POST http://127.0.0.1:8080/api/v1/ui/generate/dynamic/stream \
  -H 'Authorization: Bearer dev:9527' \
  -H 'Content-Type: application/json' \
  -d '{"user_id":"9527","scene_code":"ORDER_DETAIL_PAGE","device_info":{"screen_width":390,"screen_height":844,"platform":"android"},"business_data":{"order_id":"ORD_20261014_001","status":"PAID","items":[]}}'
```
