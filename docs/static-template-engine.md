# 静态模板引擎与兜底渲染（REQ-202610-1172）

> 模块：M1 动态 UI 生成 · 第一阶段「模板填充」
> 依赖：REQ-202610-1171 UI 生成接口与场景注册
> 下游：REQ-202610-1185 AI 生成管线（本模块是其降级目标）

## 1. 目标

1. 提供**模板库**：首批内置订单详情模板（`ORDER_DETAIL_PAGE`），结构可承载 PRD §5.1
   `business_data` 示例；
2. 提供 **Data Payload 填充引擎**：业务 JSON → 模板占位符，默认 HTML 转义；
3. 控件交互**硬编码绑定**：页面内 `button.onclick` 内联 `fetch` 调用后端业务接口；
4. 渲染延时 **P95 ≤ 200ms**（PRD §6.1）；
5. 作为全系统**兜底输出通道**：动态管线（REQ-202610-1185）超时 / 校验失败时调用
   `TemplateEngine::render_fallback`，始终返回静态模板页面。

## 2. 代码结构

| 路径 | 说明 |
| --- | --- |
| `server/src/ui_template/engine.rs` | 与业务无关的占位符引擎：解析、上下文栈、HTML 转义 |
| `server/src/ui_template/catalog.rs` | 模板库注册表（场景编码 → 版本 / 模板 / 视图构建函数） |
| `server/src/ui_template/order_detail.rs` | 订单详情业务数据校验与视图模型构建 |
| `server/src/ui_template/mod.rs` | `TemplateEngine` 门面、`RenderedPage`、`TemplateError` |
| `server/templates/order_detail.html` | 订单详情单文件 HTML 模板（编译期 `include_str!` 内嵌） |
| `server/src/routes/ui_render.rs` | `POST /api/v1/ui/render` 静态渲染接口 |
| `server/tests/template_engine.rs` | 引擎行为、订单详情、转义、兜底与 P95 测试 |
| `server/tests/ui_render_api.rs` | HTTP 端到端、内联 fetch 目标可达性与 P95 测试 |

模板库在编译期内嵌，运行时不读取文件、不依赖对象存储，避免冷启动 IO。

## 3. 模板语法

| 语法 | 含义 |
| --- | --- |
| `{{path}}` | 输出标量值，**始终 HTML 转义**（`& < > " '`） |
| `{{order.id}}` | 点路径取值；当前上下文找不到时向父级上下文回溯 |
| `{{#items}}...{{/items}}` | 数组迭代；对象 / 真值渲染一次；空数组跳过 |
| `{{^items}}...{{/items}}` | 反向区块：数组为空 / 值缺失 / 假值时渲染 |
| `{{@index}}` | 循环下标（从 0 开始） |
| `{{index}}` | 循环序号（从 1 开始；元素自带 `index` 字段时优先用元素字段） |
| `{{.}}` | 当前上下文本身 |
| `{{! 注释 }}` | 忽略 |

错误策略：缺失变量、null 插值、未闭合 / 不匹配区块、非法标签名均返回
`TemplateError::Render`，不静默产出半成品页面。

## 4. 场景契约：ORDER_DETAIL_PAGE

请求中的 `business_data` 与 PRD §5.1 完全同构：

```json
{
  "order_id": "ORD_20261014_001",
  "status": "PAID",
  "items": [
    { "name": "智能降噪耳机", "price": 299.00, "qty": 1 }
  ]
}
```

校验规则：

- `order_id`：1–64 字符，仅允许 ASCII 字母、数字、`_`、`-`（同时保证可安全放入 URL / 内联 JS 字符串）；
- `status`：`PENDING | PAID | SHIPPED | COMPLETED | CANCELLED`；
- `items`：1–100 项；`name` 1–200 字符；`price` 0–1,000,000,000；`qty` 1–9999；
- 金额使用 `rust_decimal::Decimal`，输出固定两位小数，不使用浮点。

视图模型额外提供：`status_label` / `status_class`（状态语义色）、`item_count`、
`total_qty`、`total`（按 `price × qty` 精确求和）、`currency_symbol`、`refresh_url`、
`device_class`（平台 + 宽度分档）。

## 5. 接口

### `POST /api/v1/ui/render`

鉴权：`Authorization: Bearer <token>`（与现有业务接口一致）。

请求：

```json
{
  "scene_code": "ORDER_DETAIL_PAGE",
  "business_data": {
    "order_id": "ORD_20261014_001",
    "status": "PAID",
    "items": [{ "name": "智能降噪耳机", "price": 299.00, "qty": 1 }]
  },
  "device_info": { "screen_width": 390, "screen_height": 844, "platform": "android" }
}
```

`device_info` 可省略，默认 `390×844 / web`。

响应：

```json
{
  "code": 0,
  "msg": "ok",
  "data": {
    "html": "<!DOCTYPE html>...",
    "scene_code": "ORDER_DETAIL_PAGE",
    "strategy": "template",
    "template_version": "v1",
    "render_ms": 0.284
  }
}
```

错误码：

| 场景 | HTTP | code |
| --- | --- | --- |
| 未认证 | 401 | 40100 |
| 参数校验失败 / 业务数据不符合契约 | 400 | 40000 |
| 场景未注册 | 404 | 40410 |
| 模板内部错误 | 500 | 50000 |

### 与 03 / 17 的集成

- **03（REQ-202610-1171，已接线）**：03 的场景注册表将 `ORDER_DETAIL_PAGE` 的
  renderer 指向 `ui_template::render_order_detail_scene`，因此
  `POST /api/v1/ui/generate` 与 `POST /api/v1/ui/render` 共用同一份模板、
  同一套业务数据校验与转义规则；`generate` 返回的 `html_content` 即本引擎产出的
  单文件 HTML，`version` 仍为 `template:ORDER_DETAIL_PAGE:v1`。
  其余场景（订单列表 / 商品详情）暂由 03 自带渲染器提供，后续可平移至本模板库。
- **17（REQ-202610-1185）**：动态生成超时、模型输出未通过校验器时，调用
  `TemplateEngine::render_fallback(...)`。该入口语义上只走模板策略，返回
  `strategy = "template"`，不抛动态生成相关错误。

## 6. 内联交互绑定

订单详情模板中的「刷新订单状态」按钮使用原生 `onclick` 内联 `fetch`：

```html
<button type="button"
  onclick="fetch('/api/v1/orders/{{order_id}}/business-data',{headers:{'Accept':'application/json','Authorization':'Bearer '+IntimeUI.token()}}).then(...)">刷新订单状态</button>
```

- 目标地址来自 `refresh_url`，与后端 `GET /api/v1/orders/{order_id}/business-data` 一致；
- `IntimeUI.token()` 依次读取 `window.__INTIMEUI_TOKEN__`、`localStorage.intimeui_token`，
  由客户端容器（WebView 桥）注入，模板本身不落任何凭据；
- `order_id` 经白名单字符校验，不会破坏 URL / JS 字符串；
- 集成测试 `ui_render_inline_fetch_target_is_reachable` 会从渲染结果中提取该 URL，
  再以鉴权请求实际调用，断言 200 且返回同一订单。

## 7. 性能测量方法（P95 ≤ 200ms）

### 7.1 引擎级（纯渲染）

```bash
cargo test -p intimeui-server --test template_engine \
  order_detail_render_p95_is_under_200ms -- --nocapture
```

方法：预热 20 次后连续采样 200 次 `TemplateEngine::render`，按升序取
`ceil(0.95 × N) - 1` 位作为 P95，断言 `< 200ms`；同时打印 P50 / P95 / P99。

本机实测（2026-10-22，8 核 VPS，debug 构建）：

```
order_detail render latency: p50=261.796µs p95=389.276µs p99=542.731µs
```

### 7.2 HTTP 端到端

```bash
cargo test -p intimeui-server --test ui_render_api \
  ui_render_end_to_end_p95_is_under_200ms -- --nocapture
```

方法：预热 10 次后，通过 `tower::ServiceExt::oneshot` 走完整路由 + 鉴权 + 限流中间件，
连续采样 100 次，同样取 P95；本机实测 `p95=3.329225ms`。

> 结论：静态模板路径距离 200ms 门槛有两个数量级余量；生产环境的主要变量是网络与
> 网关，而非模板渲染本身。响应体中的 `render_ms` 可用于线上持续观测。

## 8. 安全

- **输出转义**：引擎对全部插值执行 HTML 实体编码；模板语法不提供「原样输出」开关；
- **输入校验**：订单号白名单、状态枚举、数组 / 字符串 / 金额 / 数量边界；
- **单文件约束**：模板不含任何外部脚本、样式、字体、图片引用；
- **无凭据**：模板不嵌入 token；交互脚本从容器注入的存储读取；
- **错误不泄露**：业务数据错误返回字段级 400；模板内部错误仅记录日志，响应为统一 500。
