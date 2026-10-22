-- 用户画像引擎（REQ-202610-1176）
--
-- 偏好向量与 PRD §4.1 对齐：font_scale / density / theme / radius / spacing。
-- 主题在库内以连续值 theme_score 保存（0 = 明亮，1 = 暗黑），
-- 对外 API 按 0.5 阈值映射为 light / dark，避免单次调研直接翻转主题。
--
-- 幂等性：sqlx 迁移器通过 _sqlx_migrations 记录版本，本文件同时使用 IF NOT EXISTS。

CREATE TABLE IF NOT EXISTS user_profiles (
    user_id          TEXT PRIMARY KEY NOT NULL,
    font_scale       REAL NOT NULL DEFAULT 1.0
                     CHECK (font_scale BETWEEN 0.8 AND 1.4),
    density          REAL NOT NULL DEFAULT 0.5
                     CHECK (density BETWEEN 0.0 AND 1.0),
    theme_score      REAL NOT NULL DEFAULT 0.0
                     CHECK (theme_score BETWEEN 0.0 AND 1.0),
    radius           REAL NOT NULL DEFAULT 8.0
                     CHECK (radius BETWEEN 0.0 AND 24.0),
    spacing          REAL NOT NULL DEFAULT 1.0
                     CHECK (spacing BETWEEN 0.75 AND 1.5),
    confidence       REAL NOT NULL DEFAULT 0.0
                     CHECK (confidence BETWEEN 0.0 AND 1.0),
    sample_count     INTEGER NOT NULL DEFAULT 0
                     CHECK (sample_count >= 0),
    version          INTEGER NOT NULL DEFAULT 1
                     CHECK (version >= 1),
    cold_start       INTEGER NOT NULL DEFAULT 1
                     CHECK (cold_start IN (0, 1)),
    created_at       TEXT NOT NULL,
    updated_at       TEXT,
    last_observed_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_user_profiles_updated_at
    ON user_profiles(updated_at);
