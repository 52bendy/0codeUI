-- InTimeUI 高风险意图二次确认（REQ-202610-1179）
--
-- 设计要点：
--   * 写 / 删类意图只生成 PENDING 确认记录，绝不直接执行；
--   * 客户端持有的确认令牌不落库，只保存 SHA-256 摘要；
--   * 记录绑定 user_id + session_id + 动作快照，确认时再次校验归属；
--   * status 由 PENDING 原子迁移到 EXECUTING，保证「仅执行一次」；
--   * intent_confirmation_events 记录创建 / 确认 / 拒绝 / 过期 / 执行 / 失败
--     全生命周期事件，供审计追溯。
-- 迁移编号 0006 预留 1174/1175/1176/1177 的 0002~0005。

CREATE TABLE IF NOT EXISTS intent_confirmations (
    id               TEXT PRIMARY KEY NOT NULL,
    user_id          TEXT NOT NULL,
    session_id       TEXT NOT NULL,
    intent_summary   TEXT NOT NULL,
    risk_level       TEXT NOT NULL,
    proposed_changes TEXT NOT NULL,
    token_hash       TEXT NOT NULL UNIQUE,
    status           TEXT NOT NULL,
    created_at       TEXT NOT NULL,
    expires_at       TEXT NOT NULL,
    decided_at       TEXT,
    executed_at      TEXT,
    result_payload   TEXT,
    failure_reason   TEXT
);

CREATE INDEX IF NOT EXISTS idx_intent_confirmations_user_id
    ON intent_confirmations(user_id);
CREATE INDEX IF NOT EXISTS idx_intent_confirmations_status_expires
    ON intent_confirmations(status, expires_at);

CREATE TABLE IF NOT EXISTS intent_confirmation_events (
    id              TEXT PRIMARY KEY NOT NULL,
    confirmation_id TEXT NOT NULL,
    event_type      TEXT NOT NULL,
    actor_user_id   TEXT,
    detail          TEXT,
    created_at      TEXT NOT NULL,
    FOREIGN KEY (confirmation_id) REFERENCES intent_confirmations(id)
);

CREATE INDEX IF NOT EXISTS idx_intent_confirmation_events_confirmation
    ON intent_confirmation_events(confirmation_id, created_at);
