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
│   │   ├── profile/            # 用户画像引擎（EWMA / 防抖动 / Prompt 注入）
│   │   ├── few_shot/           # Few-Shot 模版检索（场景过滤 / 画像排序 / token 预算）
│   │   ├── repository/         # 数据访问层 + 原子能力（create_order 等）
│   │   ├── error.rs            # 全局错误码规范 + AppError
│   │   ├── response.rs         # 统一响应结构 {code, msg, data}
│   │   ├── state.rs            # 应用共享状态
│   │   ├── telemetry.rs        # 结构化日志
│   │   ├── validation.rs       # 请求参数校验提取器
│   │   ├── middleware/         # 鉴权 / 限流 / 超时中间件
│   │   ├── routes/             # 路由注册与端点
│   │   └── bin/intimeui-seed.rs # 种子数据一键加载 CLI
│   └── tests/                  # 集成测试（api / data_model / profile_engine / user_profile / few_shot）
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
  - 用户画像引擎：EWMA 偏好计算、单步限幅 / 死区 / 更新频率控制、冷启动置信度、Prompt 可注入 JSON（REQ-202610-1176）；
  - Few-Shot 模版检索（RAG）：场景硬过滤 + 语义 / 画像 / 质量加权排序、token 预算裁剪、
    空库 / 低置信安全降级，可直接把片段段落注入 Prompt（REQ-202610-1183）。

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
| `DATABASE_URL` | `sqlite://data/intimeui.db` | SQLite 连接串（文件自动创建；测试可用 `sqlite::memory:`） |
| `PROFILE_EWMA_ALPHA` | `0.15` | 画像 EWMA 基础平滑系数，`(0, 1]` |
| `PROFILE_MAX_ALPHA` | `0.6` | 画像 EWMA 平滑系数上限，`[base, 1]` |
| `PROFILE_MIN_UPDATE_INTERVAL_SECS` | `60` | 画像自动更新最小间隔（秒） |
| `PROFILE_MAX_OBSERVATIONS` | `1000` | 单次画像刷新最多处理样本数 |
| `FEW_SHOT_ENABLED` | `true` | Few-Shot 检索总开关 |
| `FEW_SHOT_TOP_K` | `3` | 单次返回片段数上限（1~10） |
| `FEW_SHOT_TOKEN_BUDGET` | `1200` | Prompt 片段 token 预算（64~8000） |
| `FEW_SHOT_MIN_SCORE` | `0.15` | 最低相关性阈值（0~1），低于该值安全降级 |
| `FEW_SHOT_MAX_SNIPPET_CHARS` | `4000` | 单片段最大字符数（200~20000） |
| `FEW_SHOT_CANDIDATE_LIMIT` | `200` | 单场景候选集上限（1~1000） |
| `FEW_SHOT_ADMIN_TOKEN` | 空 | 片段写接口管理令牌；设置后需携带 `X-Few-Shot-Token` |

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

### 接口一览（M0 示例）

| 方法 | 路径 | 鉴权 | 说明 |
| --- | --- | --- | --- |
| GET | `/` | 否 | 服务自述 |
| GET | `/healthz`、`/readyz` | 否 | 健康检查（免限流） |
| GET | `/api/v1/ping` | 否 | 探活示例 |
| GET | `/api/v1/me` | 是 | 返回当前用户标识（user_id） |
| POST | `/api/v1/echo` | 是 | 参数校验示例（`message` 1~500 字；`priority` 1~9） |
| GET | `/api/v1/orders/{order_id}/business-data` | 是 | 订单业务数据（PRD §5.1 `business_data` 同构） |
| GET | `/api/v1/users/{user_id}/balance` | 是 | 用户余额（精确十进制金额） |
| GET | `/api/v1/users/{user_id}/profile` | 是 | 读取用户画像（冷启动返回默认值） |
| PUT | `/api/v1/users/{user_id}/profile` | 是 | 主动设置偏好（font_scale / density / theme / radius / spacing） |
| POST | `/api/v1/users/{user_id}/profile/refresh` | 是 | 按埋点 / 调研样本重算画像（EWMA） |
| GET | `/api/v1/few-shot/snippets` | 是 | 分页列出 Few-Shot 片段（可按场景过滤） |
| POST | `/api/v1/few-shot/snippets` | 是 | 登记 / 幂等更新片段（可配管理令牌） |
| POST | `/api/v1/few-shot/snippets/{id}/feedback` | 是 | 记录片段满意度反馈 |
| POST | `/api/v1/few-shot/retrieve` | 是 | 按场景 / 画像 / 语义检索并注入 Prompt |

鉴权方式：请求头 `Authorization: Bearer <token>`；开发模式额外接受 `Bearer dev:<user_id>`。

### 数据模型与种子数据（M0）

**表结构**（`server/migrations/0001_core_schema.sql`）：

| 表 | 说明 |
| --- | --- |
| `users` | 用户（`usr_9527` 示例用户，余额 1000.00 CNY） |
| `products` | 商品（`智能降噪耳机` 299.00、`机械键盘` 499.00） |
| `orders` | 订单（`ORD_20261014_001`，状态 `PAID`，合计 299.00） |
| `order_items` | 订单项（商品名称/单价为下单时快照） |
| `operation_records` | 原子能力操作审计（create_order / query_user_balance / update_item_status / delete_record） |
| `user_profiles` | 用户画像（偏好向量 / 置信度 / 样本数 / 版本 / 冷启动标记） |
| `ui_few_shot_snippets` | Few-Shot 组件片段库（来源 / 场景 / HTML / 风格 / 满意度 / 反馈统计） |

**金额精度**：SQLite 没有原生 `DECIMAL`，`NUMERIC` 亲和性会退化为 IEEE-754 浮点。
因此数据库以「分」为单位的整数（`*_cents`）精确存储，Rust 领域层统一使用
`rust_decimal::Decimal` 计算与序列化（JSON 数字），全链路不使用 `f64`。
`decimal_to_cents` 会拒绝超过两位小数的金额，避免静默舍入。

**迁移**：`server/migrations/*.sql` 由 sqlx 版本化管理（`_sqlx_migrations`），
服务启动时自动执行；重复执行幂等。新增表结构请追加迁移文件，不要修改已发布迁移。

### 用户画像引擎（REQ-202610-1176）

画像引擎把 REQ-202610-1174 的埋点事件与 REQ-202610-1175 的偏好调研统一为
`{font_scale, density, theme, radius, spacing}` 偏好向量，并通过 EWMA 平滑收敛。
防抖动三层：单步限幅（异常停留 / 误触最多移动固定步长）、冷启动死区、更新频率控制。
`GET /api/v1/users/{user_id}/profile` 返回的 `prompt_profile` 是可直接注入 Prompt 的扁平 JSON。

完整算法、API 契约与调参说明见 [`docs/user-profile-engine.md`](docs/user-profile-engine.md)。

### Few-Shot 模版检索（REQ-202610-1183）

`server/src/few_shot/` 实现「场景硬过滤 → 语义 / 画像 / 质量加权排序 → token 预算渲染」的
轻量 RAG 链路：中文单字 + 二元组词法相似度、画像风格亲和度、显式 / 隐式满意度共同打分；
空库、低置信、预算不足时返回 `degraded=true` 的空片段，不影响生成主链路。
片段 HTML 作为不可信数据做危险标记检查，并用长于内容的代码围栏包裹后注入 Prompt。

完整设计、评分公式、降级矩阵、API 示例与人工抽检清单见
[`docs/few-shot-rag.md`](docs/few-shot-rag.md)。

**种子数据一键加载**：

```bash
# 使用 DATABASE_URL（默认 sqlite://data/intimeui.db）
cargo run -p intimeui-server --bin intimeui-seed

# 或显式指定数据库
cargo run -p intimeui-server --bin intimeui-seed -- --database-url sqlite://data/intimeui.db
```

种子加载使用 `INSERT OR IGNORE`，重复执行不会产生重复记录；返回本次实际插入行数。

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

- **M0 基础工程**：REQ-202610-1169 服务基座（已交付）、REQ-202610-1170 数据模型与种子数据（本批交付）
- M1 模板兜底链路 / M2 埋点画像 / M3 意图引擎 / M4 动态生成 / M5 安全 / M6 训练线 / M7 客户端与验收
