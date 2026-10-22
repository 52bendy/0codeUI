-- InTimeUI 核心业务数据模型（REQ-202610-1170）
--
-- 金额规范：
--   SQLite 没有原生 DECIMAL，NUMERIC 亲和性会把小数转成 IEEE-754 浮点，
--   因此金额统一以「最小货币单位（分）」的整数存储（*_cents），
--   在 Rust 领域层通过 rust_decimal::Decimal 精确计算与输出，全程不使用 f64。
--   1 元 = 100 分，例如 299.00 元存储为 29900。
--
-- 幂等性：
--   sqlx 迁移器通过 _sqlx_migrations 记录版本；本文件同时使用 IF NOT EXISTS，
--   重复执行不会报错、不会重复建表。

-- 用户
CREATE TABLE IF NOT EXISTS users (
    id            TEXT PRIMARY KEY NOT NULL,
    display_name  TEXT NOT NULL,
    balance_cents INTEGER NOT NULL DEFAULT 0 CHECK (balance_cents >= 0),
    currency      TEXT NOT NULL DEFAULT 'CNY',
    status        TEXT NOT NULL DEFAULT 'ACTIVE'
                  CHECK (status IN ('ACTIVE', 'DISABLED')),
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

-- 商品
CREATE TABLE IF NOT EXISTS products (
    id          TEXT PRIMARY KEY NOT NULL,
    name        TEXT NOT NULL,
    price_cents INTEGER NOT NULL CHECK (price_cents >= 0),
    currency    TEXT NOT NULL DEFAULT 'CNY',
    status      TEXT NOT NULL DEFAULT 'ON_SALE'
                CHECK (status IN ('ON_SALE', 'OFF_SALE')),
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

-- 订单
CREATE TABLE IF NOT EXISTS orders (
    id               TEXT PRIMARY KEY NOT NULL,
    user_id          TEXT NOT NULL REFERENCES users(id),
    status           TEXT NOT NULL
                     CHECK (status IN ('PENDING', 'PAID', 'SHIPPED', 'COMPLETED', 'CANCELLED')),
    total_cents      INTEGER NOT NULL CHECK (total_cents >= 0),
    currency         TEXT NOT NULL DEFAULT 'CNY',
    shipping_address TEXT,
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL
);

-- 订单项（商品名称/单价为下单时快照，避免商品后续变更影响历史订单）
CREATE TABLE IF NOT EXISTS order_items (
    id               TEXT PRIMARY KEY NOT NULL,
    order_id         TEXT NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
    product_id       TEXT REFERENCES products(id) ON DELETE SET NULL,
    name             TEXT NOT NULL,
    unit_price_cents INTEGER NOT NULL CHECK (unit_price_cents >= 0),
    qty              INTEGER NOT NULL CHECK (qty > 0),
    amount_cents     INTEGER NOT NULL CHECK (amount_cents >= 0),
    status           TEXT NOT NULL DEFAULT 'NORMAL'
                     CHECK (status IN ('NORMAL', 'SHIPPED', 'REFUNDED', 'CANCELLED')),
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL
);

-- 操作记录（原子能力审计：create_order / query_user_balance / update_item_status / delete_record）
CREATE TABLE IF NOT EXISTS operation_records (
    id             TEXT PRIMARY KEY NOT NULL,
    actor_user_id  TEXT,
    capability     TEXT NOT NULL,
    entity_type    TEXT NOT NULL,
    entity_id      TEXT NOT NULL,
    request_payload TEXT NOT NULL DEFAULT '{}',
    result_payload TEXT,
    status         TEXT NOT NULL
                   CHECK (status IN ('SUCCESS', 'FAILED', 'PENDING_CONFIRMATION')),
    created_at     TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_orders_user_id ON orders(user_id);
CREATE INDEX IF NOT EXISTS idx_orders_status ON orders(status);
CREATE INDEX IF NOT EXISTS idx_order_items_order_id ON order_items(order_id);
CREATE INDEX IF NOT EXISTS idx_order_items_product_id ON order_items(product_id);
CREATE INDEX IF NOT EXISTS idx_operation_records_entity ON operation_records(entity_type, entity_id);
CREATE INDEX IF NOT EXISTS idx_operation_records_created_at ON operation_records(created_at);
