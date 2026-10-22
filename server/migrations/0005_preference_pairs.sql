-- 偏好对齐数据管道（REQ-202610-1188，PRD §4.2）
--
-- 一条记录 = 同一 prompt 下的 <chosen_html, rejected_html> 偏好对。
-- 溯源字段：user_id / scene_code / session_id / request_id / survey_id /
-- prompt_template_version / chosen_model / rejected_model / pipeline_version。
-- 清洗结果：quality_score（0–1）与 quality_flags（JSON 数组）。
--
-- 幂等：pair_key = sha256(user_id | source | scene | content_hash) 唯一；
-- content_hash = sha256(prompt | chosen | rejected)，用于跨用户内容去重。
--
-- 版本号说明：0002 预留给 REQ-202610-1174 的 telemetry_events 迁移；
-- 本迁移使用 0005（0003 调研、0004 画像之后）。

CREATE TABLE IF NOT EXISTS preference_pairs (
    id                       TEXT PRIMARY KEY NOT NULL,
    pair_key                 TEXT NOT NULL UNIQUE,
    content_hash             TEXT NOT NULL,
    prompt                   TEXT NOT NULL,
    chosen_html              TEXT NOT NULL,
    rejected_html            TEXT NOT NULL,
    source                   TEXT NOT NULL
                             CHECK (source IN ('EXPLICIT_SURVEY', 'IMPLICIT_SIGNAL', 'AB_TEST', 'MANUAL')),
    scene_code               TEXT NOT NULL,
    user_id                  TEXT NOT NULL,
    session_id               TEXT,
    request_id               TEXT,
    survey_id                TEXT,
    prompt_template_version  TEXT NOT NULL DEFAULT 'unknown',
    chosen_model             TEXT,
    rejected_model           TEXT,
    chosen_style_id          TEXT,
    rejected_style_id        TEXT,
    pipeline_version         TEXT,
    preference_strength      REAL NOT NULL DEFAULT 1.0
                             CHECK (preference_strength BETWEEN 0.0 AND 1.0),
    quality_score            REAL NOT NULL DEFAULT 0.0
                             CHECK (quality_score BETWEEN 0.0 AND 1.0),
    quality_flags            TEXT NOT NULL DEFAULT '[]',
    metadata                 TEXT NOT NULL DEFAULT '{}',
    status                   TEXT NOT NULL DEFAULT 'PENDING'
                             CHECK (status IN ('PENDING', 'ACCEPTED', 'REJECTED', 'EXPORTED')),
    review_note              TEXT,
    reviewed_by              TEXT,
    reviewed_at              TEXT,
    exported_at              TEXT,
    occurred_at              TEXT NOT NULL,
    created_at               TEXT NOT NULL,
    updated_at               TEXT NOT NULL,
    CHECK (chosen_html <> rejected_html)
);

CREATE INDEX IF NOT EXISTS idx_preference_pairs_status_time
    ON preference_pairs(status, created_at);
CREATE INDEX IF NOT EXISTS idx_preference_pairs_scene_time
    ON preference_pairs(scene_code, created_at);
CREATE INDEX IF NOT EXISTS idx_preference_pairs_source_time
    ON preference_pairs(source, created_at);
CREATE INDEX IF NOT EXISTS idx_preference_pairs_user_time
    ON preference_pairs(user_id, created_at);
CREATE INDEX IF NOT EXISTS idx_preference_pairs_content
    ON preference_pairs(content_hash);

-- 导出批次：每次导出记录数据集 ID、JSONL 哈希、过滤条件与完整清单，
-- 保证训练数据可复现、可追溯。
CREATE TABLE IF NOT EXISTS preference_exports (
    id             TEXT PRIMARY KEY NOT NULL,
    dataset_id     TEXT NOT NULL UNIQUE,
    format         TEXT NOT NULL,
    record_count   INTEGER NOT NULL CHECK (record_count >= 0),
    sha256         TEXT NOT NULL,
    filters_json   TEXT NOT NULL,
    manifest_json  TEXT NOT NULL,
    created_at     TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_preference_exports_created
    ON preference_exports(created_at);
