# UI 生成接口契约（REQ-202610-1171 · M1 第一阶段）

> 端点：`POST /api/v1/ui/generate`
> 阶段：M1 动态 UI 生成 · 第一阶段（模板兜底）
> 依赖：REQ-202610-1169 服务基座、REQ-202610-1170 数据模型

## 1. 请求

### 1.1 请求头

| 头 | 必填 | 说明 |
| --- | --- | --- |
| `Authorization` | 是 | `Bearer <token>`；开发模式支持 `Bearer dev:<user_id>` |
| `Content-Type` | 是 | `application/json` |

生产模式下，请求体中的 `user_id` 必须与凭据解析出的用户一致，否则返回 `40300`。
开发模式（`AUTH_DEV_MODE=true`）允许跨用户联调。

### 1.2 请求体

```json
{
  "user_id": "usr_9527",
  "scene_code": "ORDER_DETAIL_PAGE",
  "device_info": {
    "screen_width": 390,
    "screen_height": 844,
    "platform": "android"
  },
  "business_data": {
    "order_id": "ORD_20261014_001",
    "status": "PAID",
    "items": [
      { "name": "智能降噪耳机", "price": 299.00, "qty": 1 }
    ]
  }
}
```

| 字段 | 类型 | 约束 |
| --- | --- | --- |
| `user_id` | string | 1–64 字符，非空 |
| `scene_code` | string | 1–64 字符，必须是注册表中的精确编码（大小写敏感） |
| `device_info.screen_width` | integer | 1–20000（CSS px） |
| `device_info.screen_height` | integer | 1–20000（CSS px） |
| `device_info.platform` | string | `android` / `ios` / `web` / `harmonyos`（大小写不敏感，`harmony` 视为 `harmonyos`） |
| `business_data` | object | JSON 对象；序列化后 ≤ 64 KiB；结构由场景契约决定 |

金额字段使用 JSON 数字传递，服务端按 `rust_decimal::Decimal` 精确解析，
不接受浮点误差、负数或超过两位小数。

## 2. 响应

### 2.1 成功响应

HTTP 200：

```json
{
  "code": 0,
  "msg": "ok",
  "data": {
    "render_type": "SINGLE_FILE_HTML",
    "version": "template:ORDER_DETAIL_PAGE:v1",
    "html_content": "<!DOCTYPE html>..."
  }
}
```

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `code` | integer | `0` 表示成功 |
| `msg` | string | `ok` |
| `data.render_type` | string | 第一阶段固定 `SINGLE_FILE_HTML` |
| `data.version` | string | `<strategy>:<scene_code>:<template_version>` |
| `data.html_content` | string | 完整、可直接加载的单文件 HTML |

`version` 示例：

- `template:ORDER_DETAIL_PAGE:v1`：模板策略 + 订单详情场景 + 模板 v1
- 后续动态场景形如 `dynamic:ORDER_DETAIL_PAGE:<prompt-or-model-version>`

### 2.2 错误响应

错误时 `data` 为 `null`，结构与成功响应一致：

```json
{
  "code": 40410,
  "msg": "unknown scene_code `NOT_A_SCENE`; available scenes: ORDER_DETAIL_PAGE, ORDER_LIST_PAGE, PRODUCT_DETAIL_PAGE",
  "data": null
}
```

| code | HTTP | 场景 |
| --- | --- | --- |
| `40000` | 400 | 请求体非法、字段校验失败、`business_data` 不符合场景契约或超过 64 KiB |
| `40100` | 401 | 缺少 / 无效凭据 |
| `40300` | 403 | 请求 `user_id` 与认证用户不一致 |
| `40410` | 404 | 未知 `scene_code`（响应会列出可用场景） |
| `42900` | 429 | 触发限流（附 `Retry-After`） |
| `50000` | 500 | 渲染内部错误 |
| `50300` | 503 | 场景已注册但当前无可用渲染器（动态场景接入前） |

## 3. 场景注册表

| scene_code | 渲染策略 | 模板版本 | business_data |
| --- | --- | --- | --- |
| `ORDER_DETAIL_PAGE` | `template` | `v1` | `{ order_id, status, items: [{ name, price, qty }] }` |
| `ORDER_LIST_PAGE` | `template` | `v1` | `{ orders: [{ order_id, status, total_amount, currency?, item_count? }] }` |
| `PRODUCT_DETAIL_PAGE` | `template` | `v1` | `{ product_id, name, price, currency?, description?, stock_status? }` |

### 3.1 `ORDER_DETAIL_PAGE`

| 字段 | 约束 |
| --- | --- |
| `order_id` | 非空，≤ 64 字符 |
| `status` | `PENDING` / `PAID` / `SHIPPED` / `COMPLETED` / `CANCELLED` |
| `items` | 1–100 项 |
| `items[].name` | 非空，≤ 200 字符 |
| `items[].price` | ≥ 0，最多两位小数 |
| `items[].qty` | 1–9999 |

订单合计由服务端按 `price × qty` 精确求和，不信任客户端传入的合计值。

### 3.2 `ORDER_LIST_PAGE`

| 字段 | 约束 |
| --- | --- |
| `orders` | 0–50 项（空数组渲染空状态） |
| `orders[].order_id` | 非空，≤ 64 字符 |
| `orders[].status` | 同订单详情状态枚举 |
| `orders[].total_amount` | ≥ 0，最多两位小数 |
| `orders[].currency` | 可选，三位大写货币代码 |
| `orders[].item_count` | 可选，≤ 9999 |

### 3.3 `PRODUCT_DETAIL_PAGE`

| 字段 | 约束 |
| --- | --- |
| `product_id` | 非空，≤ 64 字符 |
| `name` | 非空，≤ 200 字符 |
| `price` | ≥ 0，最多两位小数 |
| `currency` | 可选，三位大写货币代码 |
| `description` | 可选，≤ 2000 字符 |
| `stock_status` | 可选：`IN_STOCK` / `LOW_STOCK` / `OUT_OF_STOCK` |

## 4. 单文件 HTML 输出约束

- 输出以 `<!DOCTYPE html>` 开头、`</html>` 结尾，可直接写入 WebView 或浏览器加载；
- CSS / JS 全部内联，不引用外部脚本、样式、字体、图片；
- 响应头之外通过 `<meta http-equiv="Content-Security-Policy">` 限制资源加载：
  `default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src data:; base-uri 'none'; form-action 'none'`；
- 所有业务字段写入 HTML 前进行 HTML 转义，脚本不拼接业务数据；
- 黑白灰主题、语义化 HTML、响应式布局，右上角提供明暗主题切换；
- 动画仅使用 `transform` / `opacity`，并尊重 `prefers-reduced-motion`。

## 5. 调用示例

```bash
curl -s -X POST http://127.0.0.1:8080/api/v1/ui/generate \
  -H 'Authorization: Bearer dev:usr_9527' \
  -H 'Content-Type: application/json' \
  -d '{
    "user_id": "usr_9527",
    "scene_code": "ORDER_DETAIL_PAGE",
    "device_info": { "screen_width": 390, "screen_height": 844, "platform": "android" },
    "business_data": {
      "order_id": "ORD_20261014_001",
      "status": "PAID",
      "items": [{ "name": "智能降噪耳机", "price": 299.00, "qty": 1 }]
    }
  }'
```

## 6. 契约测试

`server/tests/ui_generate.rs` 覆盖：

- 成功响应结构（`code` / `msg` / `data` 三字段）；
- `render_type` 固定值、`version` 可追溯、`html_content` 完整性；
- 三个首批场景均可渲染；
- 未知场景返回 `40410` 且列出可用场景；
- 未认证返回 `40100`；
- 缺失字段、非法屏幕尺寸、非法平台、非法 `business_data` 返回 `40000`；
- 生产模式用户不一致返回 `40300`。

## 7. 后续演进

- REQ-202610-1172：静态模板引擎性能优化（P95 ≤ 200ms）与更多模板；
- REQ-202610-1173：单文件 HTML 输出校验器（在渲染后增加结构 / 安全校验）；
- REQ-202610-1185：动态生成管线接入，注册表增加 `dynamic` 策略并保留模板兜底；
- 场景注册表新增场景时，必须同时补充场景契约、模板与契约测试。
