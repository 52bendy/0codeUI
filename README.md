# InTimeUI · 实时UI系统（0codeUI）

实时动态 UI 生成与 AI 意图驱动业务架构。本仓库承载后端服务与后续各模块（M0–M7）。

> 项目看板：`/dashboard/workspaces/dev/projects?code=InTimeUI`

## 仓库结构

```
.
├── Cargo.toml                  # Rust workspace
├── server/                     # 后端服务（intimeui-server，M0 服务基座）
│   ├── migrations/             # SQLite schema 迁移（sqlx，版本化且幂等）
│   ├── src/
│   │   ├── main.rs             # 服务入口（启动 / 迁移 / 优雅停机）
│   │   ├── lib.rs              # 应用组装（路由 + 中间件），供测试复用
│   │   ├── config.rs           # 环境变量配置（含密钥注入）
│   │   ├── db/                 # SQLite 连接池、迁移执行、种子数据
│   │   ├── models.rs           # 核心业务领域模型（Decimal 金额）
│   │   ├── intent/             # 意图执行编排、会话上下文与规则规划器（M3 / #09）
│   │   ├── intent_agent/       # 意图理解与 Tool Call Agent（M3 / #1180）
│   │   ├── repository/         # 数据访问层 + 原子能力（create_order 等）
│   │   ├── error.rs            # 全局错误码规范 + AppError
│   │   ├── response.rs         # 统一响应结构 {code, msg, data}
│   │   ├── state.rs            # 应用共享状态
│   │   ├── telemetry.rs        # 结构化日志
│   │   ├── telemetry_events.rs # 埋点事件模型与校验（REQ-202610-1174）
│   │   ├── validation.rs       # 请求参数校验提取器
│   │   ├── middleware/         # 鉴权 / 限流 / 超时 / 埋点配额 / 防重放接入点
│   │   ├── routes/             # 路由注册与端点
│   │   └── bin/intimeui-seed.rs # 种子数据一键加载 CLI
│   └── tests/                  # 集成测试（api / data_model / intent_execute / intent_agent / 联调）
├── .github/workflows/ci.yml    # CI：fmt + clippy + build + test
├── deploy/                     # 部署参考（systemd 单元）
├── Dockerfile
└── .env.example                # 环境变量样例（不含任何真实密钥）
```

## 后端服务（intimeui-server）

- **技术栈**：Rust（stable）· Axum 0.8 · Tokio · tower-http · tracing · validator · SQLx / SQLite · rust_decimal
- **能力（M0 服务基座）**：
  - 统一响应结构 `{code, msg, data}` 与全局错误码规范；
  - API 网关基础：路由注册、请求参数校验、简单鉴权（user_id 体系）、基础限流；
  - 配置与密钥：全部经环境变量注入（本地可用 `.env`，不入仓）；
  - 结构化日志（dev pretty / prod json）与请求 ID 贯穿；
  - 健康检查：`/healthz`（存活）、`/readyz`（含数据库探测）；
  - 数据底座：用户 / 商品 / 订单 / 订单项 / 操作记录五张表，版本化迁移 + 幂等种子数据；
  - 原子能力数据层：`create_order` / `query_user_balance` / `update_item_status` / `delete_record`（含审计记录）；
  - 埋点采集：单条 / 批量事件上报、字段校验、按用户防刷配额、用户维度明细与聚合查询（REQ-202610-1174）。
  - 意图理解与 Tool Call Agent（M3）：自然语言 → JSON Schema 决策树 → 函数调用序列，支持上下文指代消歧、多意图排序、非法输出拦截与澄清追问（详见 [`docs/intent-tool-call-agent.md`](docs/intent-tool-call-agent.md)）。

### 快速开始（本地）

```bash
# 1. 安装 Rust（已安装可跳过）
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 2. 准备配置（默认 dev 配置即可直接跑）
cp .env.example .env

# 3. 加载示例种子数据（自动执行幂等迁移；重复执行无副作用）
cargo run -p intimeui-server --bin intimeui-seed

# 4. 启动服务（默认监听 0.0.0.0:8080；启动时自动执行迁移）
cargo run -p intimeui-server

# 5. 健康检查
curl -s http://127.0.0.1:8080/healthz
# {"code":0,"msg":"ok","data":{"status":"ok","service":"intimeui-server",...}}
curl -s http://127.0.0.1:8080/readyz

# 6. 示例订单查询（PRD §5.1 business_data 结构）
curl -s http://127.0.0.1:8080/api/v1/orders/ORD_20261014_001/business-data \
  -H 'Authorization: Bearer dev:9527'
# {"code":0,"msg":"ok","data":{"order_id":"ORD_20261014_001","status":"PAID",
#  "items":[{"name":"智能降噪耳机","price":299.00,"qty":1}]}}

# 7. 示例接口（鉴权 + 参数校验）
curl -s -X POST http://127.0.0.1:8080/api/v1/echo \
  -H 'Authorization: Bearer dev:1001' \
  -H 'Content-Type: application/json' \
  -d '{"message":"hello","priority":5}'

# 8. 意图执行（PRD §5.2；写操作返回确认卡片，不直接执行）
curl -s -X POST http://127.0.0.1:8080/api/v1/intent/execute \
  -H 'Authorization: Bearer dev:usr_9527' \
  -H 'Content-Type: application/json' \
  -d '{
    "user_id": "usr_9527",
    "input_type": "NATURAL_LANGUAGE",
    "raw_input": "把这个订单的收货地址修改为北京市海淀区中关村南大街1号",
    "current_context": {
      "active_page": "ORDER_DETAIL_PAGE",
      "entity_id": "ORD_20261014_001"
    }
  }'
```

### 环境变量

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `APP_ENV` | `dev` | 运行环境：`dev` / `prod` |
| `APP_HOST` | `0.0.0.0` | 监听地址 |
| `APP_PORT` | `8080` | 监听端口 |
| `LOG_LEVEL` | `info` | 日志级别（亦兼容 `RUST_LOG`） |
| `LOG_FORMAT` | dev=`pretty` / prod=`json` | 日志格式 |
| `AUTH_DEV_MODE` | dev=`true` / prod=`false` | 开发鉴权（接受 `Bearer dev:<user_id>`），prod 禁止开启 |
| `AUTH_STATIC_TOKENS` | 空 | 静态令牌表，`token:user_id` 逗号分隔 |
| `RATE_LIMIT_ENABLED` | `true` | 是否启用限流 |
| `RATE_LIMIT_RPS` | `50` | 每秒补充令牌数 |
| `RATE_LIMIT_BURST` | `100` | 令牌桶容量（突发额度） |
| `REQUEST_TIMEOUT_SECS` | `30` | 请求处理超时 |
| `TELEMETRY_QUOTA_ENABLED` | `true` | 是否启用埋点防刷配额 |
| `TELEMETRY_QUOTA_PER_MINUTE` | `600` | 每用户每分钟允许上报的事件条数（需 ≥ 单批上限） |
| `TELEMETRY_MAX_BATCH_SIZE` | `100` | 单次上报最大事件条数（1~1000） |
| `TELEMETRY_REPLAY_ENABLED` | `false` | 是否校验防重放头（完整签名由 REQ-202610-1186 实现） |
| `INTENT_AGENT_MAX_ATTEMPTS` | `2` | 意图 Agent 模型输出最大尝试次数（1~3） |
| `INTENT_SESSION_TTL_SECS` | `1800` | 意图会话上下文 TTL（秒，最小 1） |
| `DATABASE_URL` | `sqlite://data/intimeui.db` | SQLite 连接串（文件自动创建；测试可用 `sqlite::memory:`） |

> **密钥管理**：仅通过环境变量 / Secrets 注入，严禁写入仓库或数据库。生产环境请用部署平台（systemd `EnvironmentFile`、Docker secrets、K8s Secret 等）下发。

### 统一响应与错误码

所有接口返回 `{ "code": number, "msg": string, "data": any | null }`；`code = 0` 为成功。

| code | HTTP | 含义 |
| --- | --- | --- |
| `0` | 200 | 成功 |
| `40000` | 400 | 请求参数校验失败 |
| `40100` | 401 | 未认证 / 凭据无效 |
| `40300` | 403 | 无权限 |
| `40400` | 404 | 资源不存在 |
| `40900` | 409 | 资源冲突（唯一键重复） |
| `42900` | 429 | 触发限流（附 `Retry-After`） |
| `50000` | 500 | 服务内部错误 |
| `50300` | 503 | 服务暂不可用 |
| `50400` | 504 | 请求处理超时 |

### 接口一览

| 方法 | 路径 | 鉴权 | 说明 |
| --- | --- | --- | --- |
| GET | `/` | 否 | 服务自述 |
| GET | `/healthz`、`/readyz` | 否 | 健康检查（免限流） |
| GET | `/api/v1/ping` | 否 | 探活示例 |
| GET | `/api/v1/me` | 是 | 返回当前用户标识（user_id） |
| POST | `/api/v1/echo` | 是 | 参数校验示例（`message` 1~500 字；`priority` 1~9） |
| GET | `/api/v1/orders/{order_id}/business-data` | 是 | 订单业务数据（PRD §5.1 `business_data` 同构） |
| GET | `/api/v1/users/{user_id}/balance` | 是 | 用户余额（精确十进制金额） |
| POST | `/api/v1/telemetry/event` | 是 | 埋点事件上报（单条或 `{events:[...]}` 批量） |
| GET | `/api/v1/telemetry/events` | 是 | 埋点明细（按用户 / 页面 / 时间过滤，分页） |
| GET | `/api/v1/telemetry/aggregate` | 是 | 埋点聚合（总体均值 + 分页面均值） |
| POST | `/api/v1/intent/execute` | 是 | 意图执行（PRD §5.2；读类直接执行，写 / 删类返回确认卡片） |
| POST | `/api/v1/intent/understand` | 是 | 自然语言 → 结构化决策树 / 澄清追问 |
| GET | `/api/v1/intent/tools` | 是 | 意图 Agent 可用能力清单（OpenAI function 定义） |

鉴权方式：请求头 `Authorization: Bearer <token>`；开发模式额外接受 `Bearer dev:<user_id>`。
意图执行契约详见 [`docs/intent-execute-contract.md`](docs/intent-execute-contract.md)。

### 数据模型与种子数据（M0）

**表结构**（`server/migrations/0001_core_schema.sql`）：

| 表 | 说明 |
| --- | --- |
| `users` | 用户（`usr_9527` 示例用户，余额 1000.00 CNY） |
| `products` | 商品（`智能降噪耳机` 299.00、`机械键盘` 499.00） |
| `orders` | 订单（`ORD_20261014_001`，状态 `PAID`，合计 299.00） |
| `order_items` | 订单项（商品名称/单价为下单时快照） |
| `operation_records` | 原子能力操作审计（create_order / query_user_balance / update_item_status / delete_record） |
| `intent_sessions` | 意图会话状态（上下文、最近意图、最近动作；按 `session_id` 唯一、按用户隔离） |

**金额精度**：SQLite 没有原生 `DECIMAL`，`NUMERIC` 亲和性会退化为 IEEE-754 浮点。
因此数据库以「分」为单位的整数（`*_cents`）精确存储，Rust 领域层统一使用
`rust_decimal::Decimal` 计算与序列化（JSON 数字），全链路不使用 `f64`。
`decimal_to_cents` 会拒绝超过两位小数的金额，避免静默舍入。

**迁移**：`server/migrations/*.sql` 由 sqlx 版本化管理（`_sqlx_migrations`），
服务启动时自动执行；重复执行幂等。新增表结构请追加迁移文件，不要修改已发布迁移。

**种子数据一键加载**：

```bash
# 使用 DATABASE_URL（默认 sqlite://data/intimeui.db）
cargo run -p intimeui-server --bin intimeui-seed

# 或显式指定数据库
cargo run -p intimeui-server --bin intimeui-seed -- --database-url sqlite://data/intimeui.db
```

种子加载使用 `INSERT OR IGNORE`，重复执行不会产生重复记录；返回本次实际插入行数。

### 埋点事件采集（M2，REQ-202610-1174）

**表结构**：`server/migrations/0002_telemetry_events.sql`（`telemetry_events`）。
按用户 / 页面 / 时间建立索引；`UNIQUE(user_id, event_id)` 保证客户端重试幂等。

**数据最小化**：只落库交互指标（`dwell_time` / `element_click_rate` / `misclick_rate` /
`scroll_depth`）与定位字段（`user_id` / `page_id` / 时间戳）。事件结构启用了
`deny_unknown_fields`，任何额外字段（页面 HTML、URL、文本内容等）都会被 400 拒绝。

**字段规则**：

| 字段 | 类型 | 规则 |
| --- | --- | --- |
| `page_id` | string | 1~128 字符，仅 `[A-Za-z0-9._:-]` |
| `dwell_time` | integer | 毫秒，`0..=86400000`（24h） |
| `element_click_rate` | number | `0.0..=1.0` |
| `misclick_rate` | number | `0.0..=1.0` |
| `scroll_depth` | number | `0.0..=1.0` |
| `user_id` | string | 可选；缺省取鉴权用户，显式提供时必须与鉴权用户一致 |
| `timestamp` | string | 可选 RFC3339；缺省为接收时间，允许 90 天前 ~ 未来 5 分钟 |
| `event_id` | string | 可选幂等键，1~64 字符；同用户重复上报会被跳过 |

**上报示例（单条）**：

```bash
curl -s -X POST http://127.0.0.1:8080/api/v1/telemetry/event \
  -H 'Authorization: Bearer dev:u100' \
  -H 'Content-Type: application/json' \
  -d '{"user_id":"u100","page_id":"scene:flash_sale","dwell_time":1200,
       "element_click_rate":0.35,"misclick_rate":0.02,"scroll_depth":0.8,
       "timestamp":"2026-10-22T06:00:00Z"}'
# {"code":0,"msg":"ok","data":{"accepted":1,"duplicates":0,"event_ids":["TELE_..."]}}
```

**批量上报**：`{"events":[...]}`，单批上限由 `TELEMETRY_MAX_BATCH_SIZE` 控制（默认 100）；
超出配额返回 `429`（`Retry-After` 为建议等待秒数）。

**查询明细**：

```bash
curl -s 'http://127.0.0.1:8080/api/v1/telemetry/events?user_id=u100&page_id=scene:flash_sale&limit=50&offset=0' \
  -H 'Authorization: Bearer dev:u100'
```

**聚合查询**（总体均值 + 分页面均值）：

```bash
curl -s 'http://127.0.0.1:8080/api/v1/telemetry/aggregate?user_id=u100' \
  -H 'Authorization: Bearer dev:u100'
```

用户只能读写自己的埋点数据：`user_id` 与鉴权用户不一致返回 `403`。

**防重放接入点**：上报入口统一经过 `middleware::replay::ReplayGuard`；
`TELEMETRY_REPLAY_ENABLED=true` 时先校验 `x-timestamp` / `x-nonce` / `x-signature`
三个头与 ±300s 时间窗，完整 HMAC 签名与 nonce 去重由 REQ-202610-1186 接入。

### 测试与检查

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo build --release
```

### CI

`.github/workflows/ci.yml`：push / PR 触发，执行 fmt 检查、clippy（`-D warnings`）、构建与测试。

### 部署

**方式一：systemd（裸机 / VPS）**

```bash
cargo build --release
install -m 755 target/release/intimeui-server /usr/local/bin/intimeui-server
install -m 755 target/release/intimeui-seed /usr/local/bin/intimeui-seed
install -m 644 deploy/intimeui-server.service /etc/systemd/system/
install -d -m 750 /etc/intimeui
install -d -o intimeui -g intimeui -m 750 /opt/intimeui/data   # SQLite 数据目录
install -m 600 .env.example /etc/intimeui/intimeui.env   # 按需填写，含密钥请收紧权限
sudo -u intimeui DATABASE_URL=sqlite:///opt/intimeui/data/intimeui.db intimeui-seed
systemctl daemon-reload && systemctl enable --now intimeui-server
curl -s http://127.0.0.1:8080/healthz
curl -s http://127.0.0.1:8080/readyz
```

**方式二：Docker**

```bash
docker build -t intimeui-server:latest .
docker volume create intimeui-data
docker run -d --name intimeui-server -p 8080:8080 \
  -v intimeui-data:/data \
  -e APP_ENV=prod -e AUTH_STATIC_TOKENS=... intimeui-server:latest
# 首次加载种子数据（容器内已内置 intimeui-seed）
docker exec intimeui-server intimeui-seed
```

> 生产环境建议置于网关（Nginx / Cloudflare）之后，由其终止 TLS 并透传 `X-Forwarded-For`。

## 路线图（对应 REQ-202610-1168 拆解）

- **M0 基础工程**：REQ-202610-1169 服务基座（已交付）、REQ-202610-1170 数据模型与种子数据（已交付）
- **M2 埋点画像**：REQ-202610-1174 埋点事件采集（已交付）；REQ-202610-1175 / 1176 待排期
- **M3 意图引擎**：REQ-202610-1180 意图理解与 Tool Call Agent（已交付）
- M1 模板兜底链路 / M4 动态生成 / M5 安全 / M6 训练线 / M7 客户端与验收
