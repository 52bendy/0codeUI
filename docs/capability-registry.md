# 原子能力注册中心（REQ-202610-1178）

M3 意图引擎的原子能力底座。目标是把标准 CRUD 能力以**声明式**方式注册，
让意图引擎 / Agent 能查询能力清单、生成 function 定义并安全执行，
新增能力时不需要修改调度核心。

## 1. 架构

```
server/src/capability/
├── mod.rs        # 静态注册表、按名查找、对外清单（descriptors）
├── spec.rs       # CapabilitySpec：名称 / 描述 / 参数 Schema / 返回 / 风险 / 权限
├── schema.rs     # 通用 JSON Schema 子集校验器
├── executor.rs   # 调度核心：权限 → 参数校验 → 执行 → 审计
└── builtin.rs    # 首批四个能力的执行器
```

执行链路固定为：

1. **查找能力**：未注册返回 `404 / 40400`；
2. **权限校验**：`CapabilitySpec.required_permissions` 与 `ExecutionContext` 比对；
3. **参数校验**：按能力自身的 JSON Schema 校验，未知字段直接拒绝；
4. **执行**：调用 `CapabilityExecutor`，业务逻辑复用 `repository` 层；
5. **审计**：成功由仓储层写 `SUCCESS`，失败由调度核心写 `FAILED`。

## 2. 首批能力

| 能力 | 风险 | 所需权限 | 说明 |
| --- | --- | --- | --- |
| `create_order` | `WRITE` | `order:create` | 创建订单，按商品快照计价 |
| `query_user_balance` | `READ` | `balance:read` | 查询用户余额 |
| `update_item_status` | `WRITE` | `order_item:update` | 更新订单项状态 |
| `delete_record` | `DELETE` | `record:delete` | 删除订单 / 订单项 / 商品 / 审计记录 |

风险等级供后续「二次确认」能力（REQ-202610-1179）消费：
`READ` 可自动执行，`WRITE` / `DELETE` 必须确认。

### 数据范围

- `create_order`、`query_user_balance`：`user_id` 缺省为当前登录用户；
  显式传入时必须与登录用户一致，否则返回 `403 / 40300`。
- `update_item_status`、`delete_record`（订单 / 订单项）：校验实体归属，
  不允许操作他人数据。
- `delete_record` 删除 `product` / `operation_record` 属于全局数据，
  需要额外权限 `record:delete:any`；开发模式自动授予，生产需由后续 RBAC 接入。

## 3. HTTP 接口

两个接口都要求 `Authorization: Bearer <token>`（开发模式可用 `Bearer dev:<user_id>`）。

### 3.1 能力清单

```http
GET /api/v1/capabilities
```

响应 `data`：

```json
{
  "capabilities": [
    {
      "name": "create_order",
      "description": "创建订单：按商品快照计价并写入订单、订单项与审计记录。",
      "risk_level": "WRITE",
      "required_permissions": ["order:create"],
      "parameters": { "type": "object", "properties": {}, "required": ["items"] },
      "returns": { "type": "object" },
      "function": {
        "name": "create_order",
        "description": "创建订单：按商品快照计价并写入订单、订单项与审计记录。",
        "parameters": { "type": "object", "properties": {}, "required": ["items"] }
      }
    }
  ],
  "count": 4
}
```

`function` 可直接放入 OpenAI `tools` / function calling 定义，供 Agent 消费。

### 3.2 执行能力

```http
POST /api/v1/capabilities/{name}/execute
Content-Type: application/json

{
  "parameters": {
    "order_id": "ORD_CAP_0001",
    "shipping_address": "北京市海淀区中关村南大街1号",
    "items": [{ "product_id": "PROD_20261014_001", "qty": 2 }]
  }
}
```

成功响应：

```json
{
  "code": 0,
  "msg": "ok",
  "data": {
    "capability": "create_order",
    "risk_level": "WRITE",
    "result": { "id": "ORD_CAP_0001", "status": "PENDING", "total_amount": 598.00 }
  }
}
```

错误映射：

| 场景 | HTTP | code |
| --- | --- | --- |
| 未认证 | 401 | `40100` |
| 能力未注册 | 404 | `40400` |
| 参数不符合 Schema | 400 | `40000` |
| 越权 / 权限不足 | 403 | `40300` |
| 实体不存在 | 404 | `40400` |
| 唯一键冲突 | 409 | `40900` |

## 4. 审计

审计表 `operation_records` 记录：

| 字段 | 说明 |
| --- | --- |
| `actor_user_id` | 操作人（鉴权用户） |
| `capability` | 能力名称 |
| `entity_type` / `entity_id` | 被操作实体 |
| `request_payload` | 参数摘要（JSON，最长 2000 字符） |
| `result_payload` | 结果摘要或错误信息 |
| `status` | `SUCCESS` / `FAILED` |
| `created_at` | 时间（UTC） |

- 成功：由仓储层在业务事务提交后写入 `SUCCESS`；
- 失败：参数校验、权限不足、执行错误都会写入 `FAILED`，`result_payload`
  形如 `{"error":"..."}`；
- 审计写入失败不会覆盖原始业务错误，只记录 `tracing::error!`。

## 5. 新增能力

以新增 `refund_order` 为例，只需三步，**不修改 `executor.rs`**：

1. 在 `spec.rs` 增加 `CapabilitySpec` 常量（名称、描述、参数 / 返回 Schema、
   风险等级、所需权限）；
2. 在 `builtin.rs` 实现 `CapabilityExecutor`（`execute` + `audit_entity`）；
3. 在 `mod.rs` 创建静态执行器实例，并向 `CAPABILITIES` 追加 `CapabilityEntry`。

参数约束全部写在 JSON Schema 中，调度核心使用 `schema.rs` 统一校验。
Schema 支持的关键字：`type`、`properties`、`required`、
`additionalProperties`、`enum`、`minLength` / `maxLength`、`pattern`、
`minimum` / `maximum`、`minItems` / `maxItems`、`items`。

## 6. 测试

```bash
cargo test -p intimeui-server --test capability_registry
```

覆盖：能力清单与 function 定义、四个能力执行、参数校验、权限与数据范围、
成功 / 失败审计、HTTP 契约（401 / 400 / 403 / 404）。
