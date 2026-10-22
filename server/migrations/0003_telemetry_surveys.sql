-- 显式偏好调研（REQ-202610-1175，PRD §3.2）
--
-- 一次 A/B 偏好反馈：用户在一个场景下选择 selected_style_id、拒绝 rejected_style_id。
-- 数据最小化：只存样式标识、场景、用户 / 会话与时间，不接收自由文本或页面内容。
--
-- 关联埋点事件（REQ-202610-1174）：
--   user_id      ↔ telemetry_events.user_id
--   scene_code   ↔ telemetry_events.page_id（同一场景编码）
--   occurred_at  ↔ telemetry_events.occurred_at（同一时间轴）
--   若客户端同时上报 session_id，可按会话维度进一步归集。
--
-- 幂等性：sqlx 迁移器通过 _sqlx_migrations 记录版本；本文件同时使用 IF NOT EXISTS，
-- 重复执行不会报错、不会重复建表。
--
-- 版本号说明：0002 预留给 REQ-202610-1174 的 telemetry_events 迁移，
-- 本迁移使用 0003，合并顺序为 1174 → 1175。

CREATE TABLE IF NOT EXISTS telemetry_surveys (
    id                 TEXT PRIMARY KEY NOT NULL,
    survey_id          TEXT NOT NULL,
    user_id            TEXT NOT NULL,
    session_id         TEXT,
    scene_code         TEXT NOT NULL,
    selected_style_id  TEXT NOT NULL,
    rejected_style_id  TEXT NOT NULL,
    occurred_at        TEXT NOT NULL,
    received_at        TEXT NOT NULL,
    CHECK (selected_style_id <> rejected_style_id),
    UNIQUE (user_id, survey_id)
);

-- 查询维度：按用户 / 会话 / 场景 / 时间；风格过滤走 selected/rejected 两列。
CREATE INDEX IF NOT EXISTS idx_telemetry_surveys_user_time
    ON telemetry_surveys(user_id, occurred_at);
CREATE INDEX IF NOT EXISTS idx_telemetry_surveys_user_session
    ON telemetry_surveys(user_id, session_id);
CREATE INDEX IF NOT EXISTS idx_telemetry_surveys_scene_time
    ON telemetry_surveys(scene_code, occurred_at);
CREATE INDEX IF NOT EXISTS idx_telemetry_surveys_selected
    ON telemetry_surveys(selected_style_id);
CREATE INDEX IF NOT EXISTS idx_telemetry_surveys_rejected
    ON telemetry_surveys(rejected_style_id);
