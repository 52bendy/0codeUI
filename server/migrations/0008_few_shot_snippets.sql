-- Few-Shot 组件片段库（REQ-202610-1183）
--
-- 片段来源：历史生成结果（GENERATED）、人工策展（CURATED）、内置模板（TEMPLATE）。
-- 满意度 satisfaction 由 06/07/08 的显式反馈与隐式指标共同维护；
-- implicit_score 由使用次数与正负反馈实时重算。
--
-- 幂等性：sqlx 迁移器记录版本，本文件同时使用 IF NOT EXISTS / INSERT OR IGNORE。

CREATE TABLE IF NOT EXISTS ui_few_shot_snippets (
    id                 TEXT PRIMARY KEY NOT NULL,
    source_type        TEXT NOT NULL
                       CHECK (source_type IN ('GENERATED', 'CURATED', 'TEMPLATE')),
    source_ref         TEXT,
    scene_code         TEXT NOT NULL,
    title              TEXT NOT NULL,
    snippet_html       TEXT NOT NULL,
    snippet_text       TEXT NOT NULL,
    tags_json          TEXT NOT NULL DEFAULT '[]',
    theme              TEXT NOT NULL DEFAULT 'any'
                       CHECK (theme IN ('any', 'light', 'dark')),
    font_scale         REAL
                       CHECK (font_scale IS NULL OR font_scale BETWEEN 0.8 AND 1.4),
    density            REAL
                       CHECK (density IS NULL OR density BETWEEN 0.0 AND 1.0),
    radius             REAL
                       CHECK (radius IS NULL OR radius BETWEEN 0.0 AND 24.0),
    spacing            REAL
                       CHECK (spacing IS NULL OR spacing BETWEEN 0.75 AND 1.5),
    satisfaction       REAL NOT NULL DEFAULT 0.5
                       CHECK (satisfaction BETWEEN 0.0 AND 1.0),
    implicit_score     REAL NOT NULL DEFAULT 0.5
                       CHECK (implicit_score BETWEEN 0.0 AND 1.0),
    usage_count        INTEGER NOT NULL DEFAULT 0 CHECK (usage_count >= 0),
    positive_feedback  INTEGER NOT NULL DEFAULT 0 CHECK (positive_feedback >= 0),
    negative_feedback  INTEGER NOT NULL DEFAULT 0 CHECK (negative_feedback >= 0),
    enabled            INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    created_at         TEXT NOT NULL,
    updated_at         TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_few_shot_scene_enabled
    ON ui_few_shot_snippets(scene_code, enabled, satisfaction DESC);

CREATE INDEX IF NOT EXISTS idx_few_shot_updated_at
    ON ui_few_shot_snippets(updated_at);

-- 同一来源（生成任务 / 模板版本）只保留一份片段，供生成管线幂等入库。
CREATE UNIQUE INDEX IF NOT EXISTS idx_few_shot_source_ref
    ON ui_few_shot_snippets(source_type, source_ref)
    WHERE source_ref IS NOT NULL;

-- 冷启动基线片段：空库时也能提供少量高质量参考；后续由生成结果持续补充。
INSERT OR IGNORE INTO ui_few_shot_snippets
    (id, source_type, source_ref, scene_code, title, snippet_html, snippet_text,
     tags_json, theme, font_scale, density, radius, spacing,
     satisfaction, implicit_score, created_at, updated_at)
VALUES
    ('FSP_SEED_ORDER_DETAIL_LIGHT',
     'CURATED', 'seed:order-detail-light:v1', 'ORDER_DETAIL_PAGE',
     '订单状态徽章与商品清单卡片',
     '<section class="order-card">
  <header class="order-card__head">
    <h2>订单详情</h2>
    <span class="badge badge--paid">已支付</span>
  </header>
  <ul class="order-items">
    <li class="order-item"><span class="order-item__name">商品名称</span><span class="order-item__qty">×1</span><span class="order-item__price">¥299.00</span></li>
  </ul>
  <footer class="order-card__total"><span>合计</span><strong>¥299.00</strong></footer>
</section>',
     '订单详情卡片 标题 状态徽章 已支付 商品清单 名称 数量 单价 合计 金额 卡片 圆角 浅色',
     '["order","detail","status","list","订单","详情","卡片"]',
     'light', 1.0, 0.4, 12.0, 1.0, 0.90, 0.80, '2026-10-22T00:00:00Z', '2026-10-22T00:00:00Z'),

    ('FSP_SEED_ORDER_DETAIL_DARK',
     'CURATED', 'seed:order-detail-dark:v1', 'ORDER_DETAIL_PAGE',
     '深色紧凑订单摘要头',
     '<header class="summary summary--dark">
  <div class="summary__row">
    <span class="summary__label">订单号</span>
    <span class="summary__value">ORD-0001</span>
  </div>
  <div class="summary__row">
    <span class="summary__label">应付金额</span>
    <strong class="summary__amount">¥299.00</strong>
  </div>
</header>',
     '深色 紧凑 订单摘要 头部 订单号 应付金额 信息密度 高对比 圆角 小间距',
     '["order","detail","dark","compact","summary","深色","紧凑"]',
     'dark', 1.0, 0.2, 8.0, 0.9, 0.85, 0.75, '2026-10-22T00:00:00Z', '2026-10-22T00:00:00Z'),

    ('FSP_SEED_PRODUCT_DETAIL',
     'CURATED', 'seed:product-detail:v1', 'PRODUCT_DETAIL_PAGE',
     '商品价格与库存信息块',
     '<section class="product-info">
  <h1 class="product-info__name">商品名称</h1>
  <p class="product-info__price">¥299.00</p>
  <p class="product-info__stock">现货 · 48 小时内发货</p>
  <button type="button" class="btn btn--primary">立即购买</button>
</section>',
     '商品详情 名称 价格 库存 现货 发货 购买按钮 主操作 圆角 宽松间距',
     '["product","detail","price","stock","商品","价格","库存"]',
     'light', 1.0, 0.5, 16.0, 1.1, 0.88, 0.78, '2026-10-22T00:00:00Z', '2026-10-22T00:00:00Z');
