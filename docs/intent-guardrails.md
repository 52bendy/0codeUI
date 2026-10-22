# 意图安全风控与二次确认流（REQ-202610-1179）

## 1. 目标

写 / 删类意图**不得隐式执行**。`/api/v1/intent/execute` 只负责解析与生成
「确认卡片」，用户在前端确认后，服务端才执行动作；拒绝、超时或令牌校验失败
都不会产生任何业务副作用。

安全目标：

| 威胁 | 防护 |
| --- | --- |
| 写 / 删意图被直接执行 | 风险分级：`WRITE` / `DELETE` 一律返回 `REQUIRE_CONFIRMATION`，执行只发生在 `confirm` |
| 伪造确认请求 | 一次性令牌 `cct_<64 hex>`，服务端仅存 SHA-256 摘要，常量时间比较 |
| 重放确认请求 | `PENDING -> EXECUTING` 原子状态迁移，只有一次成功；终态再次确认返回 409 |
| 跨用户确认 | 确认单绑定 `user_id`，确认时与鉴权用户比对，不一致返回 403 |
| 跨会话确认 | 确认单绑定 `session_id`，请求携带 `session_id` 时必须一致 |
| 过期后仍可确认 | `expires_at` 在原子迁移条件中再次校验，过期单落 `EXPIRED` 并返回 409 |
| 审计缺失 | 确认单全生命周期事件 + 实际执行写入 `operation_records` |

## 2. 风险分级

风险元数据来自能力目录（REQ-202610-1177，后续由 REQ-202610-1178 注册中心接管）：

| 能力 | 风险 | 行为 |
| --- | --- | --- |
| `query_user_balance` | `READ` | 直接执行 |
| `query_order_business_data` | `READ` | 直接执行 |
| `update_order_address` | `WRITE` | 生成确认卡片 |
| `update_item_status` | `WRITE` | 生成确认卡片 |
| `create_order` | `WRITE` | 生成确认卡片 |
| `delete_record` | `DELETE` | 生成确认卡片 |

多动作（`proposed_changes.additional_calls`）按最高风险合并：`DELETE > WRITE > READ`。
未注册能力直接拒绝，避免「未知即放行」。

## 3. 流程

```
前端                    /intent/execute                 /intent/confirm
 │  "把地址改成 X"             │                                │
 ├────────────────────────────>│                                │
 │                             │ 解析 + 归属校验                │
 │                             │ 生成 PENDING 确认单             │
 │<────────────────────────────┤ data.confirmation {id, token}  │
 │                                                              │
 │  用户点击「确认」                                             │
 ├─────────────────────────────────────────────────────────────>│
 │                                                              │ 校验用户 / 令牌 / 会话 / 过期
 │                                                              │ PENDING -> EXECUTING（原子）
 │                                                              │ 执行动作 + 写审计
 │<─────────────────────────────────────────────────────────────┤ status=EXECUTED
```

拒绝 / 取消调用 `POST /api/v1/intent/reject`，确认单进入 `REJECTED`，不执行任何动作。

## 4. 接口契约

### 4.1 `POST /api/v1/intent/execute`

写 / 删类意图的 `data` 中新增 `confirmation` 字段（读类不返回）：

```json
{
  "code": 0,
  "msg": "ok",
  "data": {
    "session_id": "sess_...",
    "action_type": "REQUIRE_CONFIRMATION",
    "intent_summary": "更新订单收货地址",
    "proposed_changes": {
      "target_capability": "update_order_address",
      "parameters": {
        "order_id": "ORD_20261014_001",
        "new_address": "北京市海淀区中关村南大街1号"
      }
    },
    "confirmation_prompt": "确认将订单 ORD_20261014_001 的收货地址修改为【...】吗？",
    "confirmation": {
      "confirmation_id": "CNF_<32 hex>",
      "confirmation_token": "cct_<64 hex>",
      "risk_level": "WRITE",
      "intent_summary": "更新订单收货地址",
      "proposed_changes": { "...": "动作快照" },
      "confirmation_prompt": "确认将订单 ... 吗？",
      "expires_at": "2026-10-22T07:35:00Z",
      "expires_in_secs": 300
    },
    "context": { "active_page": "ORDER_DETAIL_PAGE", "entity_id": "ORD_20261014_001" },
    "execution_results": []
  }
}
```

`confirmation_token` 只在此响应中出现一次，服务端不落明文。

### 4.2 `POST /api/v1/intent/confirm`

请求：

```json
{
  "confirmation_id": "CNF_...",
  "confirmation_token": "cct_...",
  "session_id": "sess_..."
}
```

`session_id` 可选；提供时必须与创建确认单时的会话一致。

成功响应（`status` 为 `EXECUTED`）：

```json
{
  "code": 0,
  "msg": "ok",
  "data": {
    "confirmation_id": "CNF_...",
    "status": "EXECUTED",
    "decided_at": "2026-10-22T07:31:00Z",
    "executed_at": "2026-10-22T07:31:00Z",
    "results": [
      {
        "target_capability": "update_order_address",
        "status": "SUCCESS",
        "data": { "id": "ORD_20261014_001", "shipping_address": "北京市海淀区中关村南大街1号" }
      }
    ]
  }
}
```

### 4.3 `POST /api/v1/intent/reject`

请求：

```json
{
  "confirmation_id": "CNF_...",
  "confirmation_token": "cct_...",
  "reason": "用户取消"
}
```

响应 `status=REJECTED`，包含 `reason`；不会执行任何动作。

### 4.4 错误语义

| HTTP | code | 场景 |
| --- | --- | --- |
| 400 | `40000` | 编号 / 令牌格式非法、令牌不匹配、会话不匹配、参数校验失败 |
| 403 | `40300` | 确认单不属于当前用户 |
| 404 | `40400` | 确认单不存在 |
| 409 | `40900` | 已确认 / 已拒绝 / 已过期 / 正在执行，重复确认或拒绝 |
| 500 | `50000` | 执行失败（确认单落 `FAILED`，不自动重试） |

## 5. 状态机

```
PENDING ──confirm──> EXECUTING ──success──> EXECUTED
   │                     └──────failure───> FAILED
   ├──reject──> REJECTED
   └──expire──> EXPIRED
```

- `PENDING -> EXECUTING` 由单条 `UPDATE ... WHERE status='PENDING' AND expires_at > now`
  完成，并发确认只有一个请求能拿到执行权。
- `EXECUTING -> EXECUTED/FAILED` 只允许一次；失败不自动重试，避免重复副作用。
- 后台任务每 30 秒清理过期 `PENDING`（每次最多 200 条）；确认 / 拒绝路径也会惰性过期。

## 6. 审计

`intent_confirmation_events` 记录：

| 事件 | 触发时机 |
| --- | --- |
| `CREATED` | 确认单创建 |
| `CONFIRMED` | 用户确认、开始执行 |
| `REJECTED` | 用户拒绝 / 取消 |
| `EXPIRED` | 惰性检查或后台清理发现过期 |
| `EXECUTED` | 所有动作执行成功 |
| `FAILED` | 执行失败（附失败原因） |

实际业务写入仍由各原子能力写入 `operation_records`，两层审计可按
`confirmation_id` / `entity_id` 交叉追溯。

## 7. 配置

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `INTENT_CONFIRMATION_TTL_SECS` | `300` | 确认单有效期（秒），最小 1 |

## 8. 测试覆盖

`server/tests/intent_guardrails.rs`：

- 写意图只返回确认卡片，确认前数据与审计均不变；
- 确认后执行一次，重放返回 409；
- 拒绝不产生副作用，且之后不能确认；
- 伪造令牌 400、跨用户 403、跨会话 400；
- 过期确认 409 并落 `EXPIRED`；
- 后台清理过期确认单；
- 删除意图确认后才真正删除；
- 读类意图不受影响且不产生确认单；
- 非法确认请求统一错误包络。
