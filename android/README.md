# InTimeUI Android 客户端（M7）

Android WebView 渲染容器与 JS-Native 桥接的联调最小实现。目标是让服务端下发的单文件 HTML 在原生容器中可靠渲染，并把用户交互收敛为统一的意图事件、确认卡片与埋点数据。

## 目录

```
android/
├── core/                         # 纯 JVM 核心（无 Android 依赖，可直接跑 JUnit）
│   └── src/main/java/.../core/
│       ├── api/                  # 统一请求层、ApiClient、HTTP 传输
│       ├── bridge/               # 桥接协议、消息校验、路由
│       ├── intent/               # UI_INTERACTION 事件树、确认卡片、执行回执
│       ├── replay/               # INTIMEUI-REPLAY-V1 签名、Nonce、服务器时钟
│       ├── telemetry/            # 埋点聚合、持久化队列、批量上报
│       └── json/                 # Gson 封装
├── app/                          # Android 容器
│   └── src/main/
│       ├── java/.../app/
│       │   ├── ui/               # Header、WebViewContainer、确认面板、错误兜底
│       │   ├── web/              # @JavascriptInterface 桥接与 SDK 注入
│       │   ├── telemetry/        # SharedPreferences 持久化与原生聚合器
│       │   └── MainActivity.java
│       ├── assets/
│       │   ├── intimeui-bridge.js
│       │   └── demo/order-detail.html
│       └── res/                  # 黑白灰 light/dark token 与中文文案
└── docs/android-client.md        # 架构、协议、联调与验收说明
```

## 环境

- JDK 17
- Android SDK Platform 35（compileSdk / targetSdk 35）
- Gradle 8.7（AGP 8.6.1）
- minSdk 26

## 构建与测试

```bash
cd android

# 纯 JVM 核心单测（23 个）
gradle :core:test

# 演示包：默认 demoMode=true，不访问网络
gradle :app:assembleDebug

# 远程联调包：所有参数在构建时注入，不写入仓库
gradle :app:assembleDebug \
  -Pintimeui.demoMode=false \
  -Pintimeui.apiBaseUrl=https://your-api.example \
  -Pintimeui.authToken='<bearer-token>' \
  -Pintimeui.userId='usr_9527' \
  -Pintimeui.replaySecret='<32-bytes-min-shared-secret>'
```

构建参数：

| Gradle 属性 | BuildConfig | 默认值 | 说明 |
| --- | --- | --- | --- |
| `intimeui.apiBaseUrl` | `API_BASE_URL` | `https://api.intimeui.example` | 后端地址，不含结尾 `/` |
| `intimeui.authToken` | `AUTH_TOKEN` | `dev:usr_9527` | 仅开发默认值；生产必须注入 |
| `intimeui.userId` | `AUTH_USER_ID` | `usr_9527` | 与令牌对应的用户 |
| `intimeui.replaySecret` | `REPLAY_SECRET` | 空 | 防重放共享密钥，远程模式必填 |
| `intimeui.demoMode` | `DEMO_MODE` | `true` | 为 `true` 时加载本地演示页 |
| `intimeui.sceneCode` | `SCENE_CODE` | `ORDER_DETAIL_PAGE` | 场景编码 |
| `intimeui.demoAsset` | `DEMO_ASSET` | `demo/order-detail.html` | 演示页 asset 路径 |

> 仓库不包含任何真实令牌或密钥。`dev:usr_9527` 只用于本地演示，生产构建必须通过 CI Secret 或本机 `-P` 参数注入。

## 运行方式

### 演示模式（默认）

1. 安装 debug 包并启动。
2. 容器加载 `assets/demo/order-detail.html`，右上角可切换明暗主题。
3. 点击「修改收货地址」「查询账户余额」「删除订单」，分别走 WRITE、READ、DELETE 三类意图。
4. 浏览器中直接打开该 HTML 时，页面内置 mock 桥接，可在没有 Android 设备的情况下预览交互与视觉。

### 远程模式

1. 用远程参数构建，确保 `REPLAY_SECRET` 非空。
2. 启动后调用 `POST /api/v1/ui/generate` 获取单文件 HTML，再注入 WebView。
3. 点击带 `data-intimeui-action` 的元素会走 `POST /api/v1/intent/execute`；WRITE/DELETE 返回确认卡片时弹出原生底部面板，确认后调用 `/api/v1/intent/confirm`，拒绝调用 `/api/v1/intent/reject`。
4. 埋点由原生聚合，每 15 秒或页面隐藏时通过 `POST /api/v1/telemetry/event` 批量上报。

## 桥接协议（摘要）

注入脚本暴露 `window.IntimeUI`，原生通过 `window.IntimeUINative.postMessage(json)` 接收消息。消息统一为：

```json
{"v":1,"id":"m_1_...","type":"INTENT_TRIGGER","payload":{}}
```

| 方向 | 类型 | 说明 |
| --- | --- | --- |
| JS → Native | `RENDER_READY` | 页面 DOM 就绪，携带 `page`、`scrollDepth` |
| JS → Native | `INTENT_TRIGGER` | 用户触发意图，携带 `action`、`data`、`target`、`page`、`entity` |
| JS → Native | `TELEMETRY_SIGNAL` | `TAP` / `SCROLL` 原始信号，由原生聚合 |
| JS → Native | `PAGE_HIDE` | 页面隐藏，触发一次 flush |
| JS → Native | `SET_THEME` | 页面请求切换主题 |
| JS → Native | `RENDER_ERROR` | 页面主动上报渲染错误 |
| Native → JS | `ACK` / `ERROR` | 每条消息的确定性响应 |
| Native → JS | `CONFIRMATION_CARD` | 脱敏确认卡片（不含 `confirmation_token`） |
| Native → JS | `INTENT_RESULT` | 无需确认的执行结果 |
| Native → JS | `EXECUTION_RECEIPT` | 确认/拒绝后的执行回执 |
| Native → JS | `INTENT_ERROR` | 意图执行失败 |
| Native → JS | `THEME` | 原生主题同步 |

消息大小上限 64 KiB；`type` 只允许大写字母与下划线，`id` 只允许 `[A-Za-z0-9._:-]{1,64}`。未知类型、非法 JSON、超限消息统一返回 `ERROR`，不会把内部异常文本透给页面。

## 埋点字段

与服务端 `TelemetryEventInput` 对齐，只发送以下字段：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `event_id` | string | `evt_` + UUID，重试保持不变，供服务端去重 |
| `page_id` | string | 页面标识，来自 `data-intimeui-page` |
| `dwell_time` | int | 本次聚合窗口的停留毫秒数，上限 24h |
| `element_click_rate` | number | 可交互元素点击 / 全部点击，0–1 |
| `misclick_rate` | number | 非交互区域点击 / 全部点击，0–1 |
| `scroll_depth` | number | 最深滚动比例，0–1 |
| `timestamp` | string | RFC3339 |

不采集页面正文、URL、输入内容或用户身份信息。队列持久化在 `SharedPreferences`，最多 500 条，失败批次回到队首并复用原 `event_id`。

## 防重放

`core/replay` 实现 `INTIMEUI-REPLAY-V1`，对以下三条写接口签名：

- `POST /api/v1/ui/generate`
- `POST /api/v1/intent/execute`
- `POST /api/v1/telemetry/event`

请求头：`x-timestamp`、`x-nonce`、`x-dynamic-token`、`x-signature`。签名覆盖方法、路径与查询、时间戳、Nonce、用户 ID 和原始 body 字节；客户端先序列化最终 body，再对同一份 `byte[]` 签名与发送。服务端返回 `40900`（Nonce 重放）或 `40000`（时钟窗口）时，客户端用新 Nonce 重试一次，body 不变。

确认/拒绝接口按 REQ-202610-1179 契约只使用 Bearer 鉴权，不附加防重放头。

## 验收步骤

1. `gradle :core:test`：23 个核心单测通过。
2. `gradle :app:assembleDebug`：生成 debug APK。
3. 安装到 API 26+ 设备/模拟器，确认演示页正常渲染，右上角明暗切换生效。
4. 点击「修改收货地址」：出现确认面板；确认后出现执行回执；取消后状态为 `REJECTED`。
5. 点击「查询账户余额」：无需确认，直接返回 `EXECUTION_RESULT` / 回执。
6. 点击「删除订单」：风险标识为 DELETE，确认后执行。
7. 断网后点击操作：出现网络错误提示，不崩溃；恢复网络后埋点队列继续上报。
8. 远程模式：抓包确认三条写接口带四个防重放头，确认/拒绝接口不带。

## 已知限制

- 本机已用临时 Android SDK 35 实际执行 `:core:test`（23 个测试通过）、`:app:assembleDebug`（生成 debug APK）与 `:app:lintDebug`（No issues found）；尚未在真机/模拟器执行仪器测试。
- 未实现离线 HTML 包版本管理、A/B 实验和多场景路由；当前只承载单场景单页。
- 未接入 push、深链、账号体系；鉴权令牌由构建参数注入。
- WebView 只允许加载 `https://intimeui.local/` 基址下的内容，外部导航一律拦截；生产如接入 CDN 需同步调整白名单。

更完整的架构、协议字段与联调说明见 [`docs/android-client.md`](docs/android-client.md)。
