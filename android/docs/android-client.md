# Android 客户端联调说明（M7 / REQ-202610-1189）

本文说明 InTimeUI Android 容器的运行时结构、桥接协议、意图确认链路、埋点与防重放对齐方式，以及在没有完整 Android 工具链时的验证边界。

## 1. 目标与边界

本模块解决四件事：

1. **渲染容器**：接收服务端下发的单文件 HTML，在 WebView 中加载，处理加载中、空白、超时、渲染进程崩溃与重试。
2. **JS-Native 桥接**：页面通过 `window.IntimeUI` 发起意图、上报埋点信号、请求主题切换；原生通过 `@JavascriptInterface` 返回 ACK/ERROR 与业务回调。
3. **埋点 SDK**：采集 `dwell_time`、点击率、误触率、滚动深度，按 06 契约批量上报，失败可恢复。
4. **防重放客户端**：按 18 契约对三条写接口签名，处理时钟校准与 Nonce 重试。

不在本模块范围：服务端生成算法、账号登录、推送、离线包管理、多页面路由。

## 2. 模块分层

```
MainActivity
  ├─ AppHeader                 品牌 / 场景 / 右上角明暗切换
  ├─ WebViewContainer          WebView、进度、空白检测、失败兜底
  │    └─ WebViewBridge        @JavascriptInterface + 消息路由
  ├─ ConfirmationSheet         原生确认面板 + 执行回执
  ├─ IntentRepository          意图 execute / confirm / reject
  └─ WebViewTelemetryTracker   原生埋点聚合 + 定时 flush
       └─ TelemetryQueue       持久化有界队列
            └─ TelemetryUploader 批量上报

core（纯 JVM，可单测）
  ├─ ApiClient                 统一请求层 + 签名 + 重试
  ├─ ReplaySigner              INTIMEUI-REPLAY-V1
  ├─ ServerClock               服务器 Date 头校准
  ├─ BridgeRouter              消息校验与分发
  ├─ IntentRequest             UI_INTERACTION 事件树
  └─ TelemetrySession          四项指标聚合
```

`core` 不依赖 Android，因此可以在普通 JDK 上编译与测试；`app` 只负责 Android 视图、WebView 生命周期与依赖装配。

## 3. 渲染生命周期

```
render(html)
  ├─ bridge.setAllowed(true)
  ├─ progress 可见；12s 超时计时开始
  ├─ loadDataWithBaseURL("https://intimeui.local/", html, "text/html", "UTF-8")
  ├─ onPageStarted  → injectSdk()
  ├─ onPageFinished → injectSdk() + 450ms 后空白检测
  ├─ RENDER_READY   → markReady()（取消超时、隐藏进度）
  └─ 空白/超时/主 frame 错误/渲染进程退出 → showFailure(title, detail, retry)
```

失败兜底页包含 `RENDER / FAILED` 标签、标题、说明与「重新加载」按钮。重试会重新执行 `loadContent()`：演示模式重读 asset，远程模式重新请求 `/ui/generate`。

WebView 安全基线：

- JavaScript、DOM Storage 开启；
- `allowFileAccess=false`、`allowContentAccess=false`；
- 禁止 mixed content；
- 禁止缩放与媒体自动播放；
- 外部 URL 导航一律拦截，只允许 `https://intimeui.local/` 与 `about:blank`；
- release 关闭 WebView 调试；debug 才允许明文 HTTP（`src/debug/AndroidManifest.xml`）。

## 4. 桥接协议

### 4.1 消息信封

```json
{
  "v": 1,
  "id": "m_12_lx8f3a",
  "type": "INTENT_TRIGGER",
  "payload": {}
}
```

- `v` 必须为 `1`；
- `id` 匹配 `[A-Za-z0-9._:-]{1,64}`；
- `type` 匹配 `[A-Z_]{1,32}`；
- UTF-8 编码后不超过 64 KiB；
- `payload` 必须是对象，否则按空对象处理。

原生返回统一信封：

```json
{"v":1,"id":"m_12_lx8f3a","type":"ACK","payload":{"accepted":true}}
```

错误：

```json
{"v":1,"id":"m_12_lx8f3a","type":"ERROR","payload":{"code":"UNKNOWN_TYPE","message":"unsupported message type"}}
```

`BridgeRouter` 捕获所有异常并统一返回 `HANDLER_ERROR`，不会把堆栈或内部异常文本透给页面。

### 4.2 JS → Native

| 类型 | payload | 说明 |
| --- | --- | --- |
| `RENDER_READY` | `page`, `scrollDepth` | DOM 就绪；原生启动埋点会话并标记渲染完成 |
| `TELEMETRY_SIGNAL` | `kind`, `interactive` / `depth` | `TAP` 或 `SCROLL` 原始信号 |
| `PAGE_HIDE` | `depth` | 页面隐藏；记录深度并 flush |
| `SET_THEME` | `theme` | `light` / `dark` |
| `RENDER_ERROR` | `message?` | 页面主动上报渲染失败 |
| `INTENT_TRIGGER` | `action`, `data`, `target`, `page`, `entity` | 用户点击可交互元素 |

`intimeui-bridge.js` 通过事件委托采集点击与滚动，不直接读取页面正文：

- 点击：判断目标是否在 `a,button,[role=button],[data-intimeui-action],input,select,textarea,label` 内，发送 `TAP` 与 `interactive`；
- 滚动：被动监听，计算 `scrollTop / (scrollHeight - innerHeight)`，发送 `SCROLL` 与 `depth`；
- 页面隐藏：`visibilitychange` 与 `pagehide` 各发送一次 `PAGE_HIDE`，原生按需去重/合并 flush。

### 4.3 Native → JS

| 类型 | payload | 说明 |
| --- | --- | --- |
| `CONFIRMATION_CARD` | 卡片字段（无 token） | 原生确认面板展示的同时通知页面 |
| `INTENT_RESULT` | `IntentResult.toJson()` | 无需确认的执行结果 |
| `EXECUTION_RECEIPT` | `ConfirmationDecision.toReceiptJson()` | 确认/拒绝后的回执 |
| `INTENT_ERROR` | `code`, `message` | 意图执行失败 |
| `THEME` | `theme` | 原生主题同步到页面 |

`ConfirmationCard.toDisplayJson()` 会移除 `confirmation_token`；令牌只在原生内存中传给确认/拒绝请求，不进入 WebView、不写日志。

## 5. 意图链路

### 5.1 事件树

`IntentRequest.uiInteraction(...)` 生成 09 契约要求的 `UI_INTERACTION`：

```json
{
  "user_id": "usr_9527",
  "session_id": "sess_...",
  "input_type": "UI_INTERACTION",
  "raw_input": {
    "event": {"type": "TAP", "target_id": "btn_save_address"},
    "path": [
      {"id": "ORDER_DETAIL_PAGE", "type": "page"},
      {"id": "btn_save_address", "type": "button", "action": "update_order_address"}
    ],
    "data": {"order_id": "ORD_20261014_001", "new_address": "..."},
    "page": "ORDER_DETAIL_PAGE",
    "entity_id": "ORD_20261014_001"
  },
  "current_context": {
    "active_page": "ORDER_DETAIL_PAGE",
    "entity_id": "ORD_20261014_001"
  }
}
```

### 5.2 执行与确认

```
用户点击
  → INTENT_TRIGGER
  → POST /api/v1/intent/execute（签名）
  → action_type = EXECUTE             → INTENT_RESULT，页面直接展示结果
  → action_type = REQUIRE_CONFIRMATION → ConfirmationSheet.show(card)
       → 用户确认 → POST /api/v1/intent/confirm（Bearer）
       → 用户取消 → POST /api/v1/intent/reject（Bearer）
       → EXECUTION_RECEIPT + 原生回执面板
```

确认面板展示：风险等级（WRITE/DELETE）、意图摘要、确认提示、最多 6 条 `proposed_changes`、有效期倒计时、取消/确认按钮。请求进行中按钮禁用，失败时在面板内显示错误并恢复操作。

## 6. 埋点聚合

`TelemetrySession` 以页面可见为起点，按聚合窗口统计：

| 指标 | 计算 |
| --- | --- |
| `dwell_time` | `now - startedAt`，上限 86,400,000 ms |
| `element_click_rate` | `interactiveTaps / totalTaps`，无点击为 0 |
| `misclick_rate` | `misclicks / totalTaps`，无点击为 0 |
| `scroll_depth` | 窗口内最大滚动比例 |

所有比率 clamp 到 `[0,1]`。`WebViewTelemetryTracker` 每 15 秒 flush 一次，`PAGE_HIDE` 也触发 flush；每次 flush 生成稳定 `event_id` 并重置窗口。

`TelemetryQueue`：

- 有界 FIFO，最大 500 条；
- `SharedPreferences` 持久化，进程重启恢复；
- 失败批次 `requeue` 到队首，复用原 `event_id`；
- 本地数据损坏时清空并重写空队列，不阻塞启动。

`TelemetryUploader` 每批最多 20 条，串行 flush，避免并发重复上报。

## 7. 防重放对齐

### 7.1 协议

`INTIMEUI-REPLAY-V1` 的 canonical string 为 7 行：

```
INTIMEUI-REPLAY-V1
<METHOD>
<path?query>
<timestamp>
<nonce>
<user_id>
<sha256(body)>
```

派生：

```
dynamic_token = hex(HMAC-SHA256(secret, "intimeui.replay.dynamic-token.v1\n" + timestamp + "\n" + nonce))
signature     = hex(HMAC-SHA256(dynamic_token, canonical))
```

请求头：

| Header | 值 |
| --- | --- |
| `x-timestamp` | Unix 秒 |
| `x-nonce` | `n_` + 32 hex，SecureRandom |
| `x-dynamic-token` | 上述派生值 |
| `x-signature` | 上述签名 |

### 7.2 客户端行为

- 先 `Json.write(body)` 得到最终 UTF-8 字节，签名与发送复用同一 `byte[]`；
- `ServerClock` 从任意响应 `Date` 头学习偏移，远程模式启动时先 `GET /healthz` 校准；
- 服务端返回 `40900`（Nonce 重放）或 `40000`（时钟窗口）时，换新 Nonce 重试一次，body 不变；
- 重试仍失败则抛出 `ApiException`，由 UI 映射为「请求被判定为重放 / 设备时间偏差」提示；
- 确认/拒绝接口只带 `Authorization: Bearer`，不带四个防重放头。

### 7.3 密钥注入

`REPLAY_SECRET` 只通过 Gradle `-Pintimeui.replaySecret=...` 或 CI Secret 注入 `BuildConfig`，仓库与 APK 源码中不存在真实密钥。生产环境建议由服务端在登录后下发短期密钥，再替换 `ReplaySecretProvider` 的静态配置；当前最小实现使用构建期共享密钥，便于联调。

## 8. 错误处理

`ApiClient` 将传输层与业务层错误统一为 `ApiException`：

| code | 含义 | UI 文案 |
| --- | --- | --- |
| `-1` | 网络错误 | 网络请求失败，请检查连接后重试 |
| `-2` | 响应格式非法 | 操作没有完成，请稍后重试 |
| `40900` | Nonce 重放 | 请求被判定为重放，已自动重试仍失败 |
| `40000` | 时钟窗口拒绝 | 设备时间与服务器偏差过大 |
| `40100/40300` | 未认证/无权限 | 登录状态无效或权限不足 |

所有 UI 文案走 `res/values/strings.xml`；演示页文案走内嵌 `I18N` 字典，便于后续替换为多语言资源。

## 9. 测试与验证

### 已执行

```bash
# 纯 JVM 核心单测
gradle :core:test
# BUILD SUCCESSFUL — 23 tests passed

# Android debug 包
gradle :app:assembleDebug
# BUILD SUCCESSFUL — app/build/outputs/apk/debug/app-debug.apk

# Android lint
gradle :app:lintDebug
# BUILD SUCCESSFUL — No issues found
```

覆盖：

- `ReplaySignerTest`：固定向量、动态令牌/签名、精确 body 字节、非法输入；
- `NonceGeneratorTest`：格式、长度、唯一性；
- `ServerClockTest`：偏移应用、RFC1123 解析；
- `TelemetrySessionTest`：比率、滚动、dwell clamp；
- `TelemetryQueueTest`：持久化恢复、有界丢弃、requeue 顺序、损坏数据；
- `TelemetryUploaderTest`：批量 drain、失败回队、event_id 稳定；
- `BridgeRouterTest`：分发、未知类型、非法 JSON、超限、异常不泄露；
- `IntentRequestTest`：事件树字段、可选字段省略；
- `ApiClientTest`：本地 HttpServer 验证 40900 重试新 Nonce、body 不变、签名可复算、确认接口无防重放头。

### 构建验证

本机使用临时 Android SDK Platform 35 / Build-Tools 35 实际执行了 `:core:test`、`:app:assembleDebug` 与 `:app:lintDebug`：23 个核心测试全部通过，debug APK 成功生成，lint 无告警。Gradle 构建覆盖 Java 类型、Android API 签名、资源引用、Manifest 合并与打包链路；仍不等同于真机运行。

### 待真机/模拟器执行

仪器测试建议覆盖：

1. 正常渲染 + `RENDER_READY`；
2. 空 HTML / 主 frame 404 / 12s 超时 → 失败页与重试；
3. 点击 WRITE → 确认面板 → 确认 → 回执；
4. 点击 DELETE → 取消 → `REJECTED`；
5. 断网 → 网络错误提示 → 恢复后队列继续；
6. 明暗切换在 Header、WebView、确认面板三处一致；
7. 远程模式抓包核对四个签名头与 canonical 复算。

## 10. 已知限制与后续

- 当前环境没有真机/模拟器，未执行仪器测试；Gradle 构建与 lint 已在本机临时 SDK 35 环境跑通，合并到 CI 后应补齐设备测试。
- 确认面板只支持 `WRITE`/`DELETE` 两种风险文案，其他风险等级按 WRITE 展示。
- 埋点只覆盖点击、滚动、停留与误触，未采集输入焦点、表单完成率等扩展指标。
- 单页容器，未实现多页面栈、返回键路由与页面级缓存。
- `ReplaySecretProvider` 目前读取 BuildConfig 静态密钥；生产建议改为登录后短期下发，并增加密钥轮换。
- WebView 的外部导航白名单固定为 `https://intimeui.local/`；接入 CDN 或远程图片时需要同步调整 CSP 与导航策略。
