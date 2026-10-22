//! 单文件 HTML 模板渲染（M1 第一阶段）。
//!
//! 约束：
//! - 输出必须是完整、可直接在 WebView 加载的单文件 HTML；
//! - 不引用任何外部脚本 / 样式 / 字体 / 图片；
//! - 所有业务字段在写入 HTML 前做 HTML 转义，避免 XSS；
//! - 交互脚本只操作静态 DOM，不拼接业务数据。

use std::fmt::Write as _;

use rust_decimal::{Decimal, RoundingStrategy};
use serde::Deserialize;
use serde_json::Value;

use crate::models::{OrderBusinessData, OrderBusinessItem};
use crate::ui::context::{SceneContext, SceneError};

const BASE_CSS: &str = include_str!("assets/base.css");
const BASE_JS: &str = include_str!("assets/base.js");

const VALID_ORDER_STATUSES: &[&str] = &["PENDING", "PAID", "SHIPPED", "COMPLETED", "CANCELLED"];
const VALID_STOCK_STATUSES: &[&str] = &["IN_STOCK", "LOW_STOCK", "OUT_OF_STOCK"];

/// 订单列表场景的业务数据。
#[derive(Debug, Deserialize)]
struct OrderListData {
    orders: Vec<OrderSummary>,
}

/// 订单列表中的单笔订单。
#[derive(Debug, Deserialize)]
struct OrderSummary {
    order_id: String,
    status: String,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    total_amount: Decimal,
    #[serde(default)]
    currency: Option<String>,
    #[serde(default)]
    item_count: Option<u32>,
}

/// 商品详情场景的业务数据。
#[derive(Debug, Deserialize)]
struct ProductDetailData {
    product_id: String,
    name: String,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    price: Decimal,
    #[serde(default)]
    currency: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    stock_status: Option<String>,
}

/// 渲染 `ORDER_DETAIL_PAGE`。
pub fn render_order_detail(ctx: &SceneContext<'_>) -> Result<String, SceneError> {
    let data: OrderBusinessData = parse_business_data(ctx.business_data, "ORDER_DETAIL_PAGE")?;
    validate_order_detail(&data)?;

    let total = order_total(&data.items);
    let total_qty: i32 = data.items.iter().map(|item| item.qty).sum();
    let (status_class, status_label) = order_status_view(&data.status);
    let items_html = render_order_items(&data.items);

    let body = format!(
        r#"{header}
<main class="page">
  <section class="hero card" style="--delay: 0ms">
    <div class="hero-top">
      <span class="eyebrow">订单详情</span>
      <span class="status {status_class}"><span class="status-dot" aria-hidden="true"></span>{status_label}</span>
    </div>
    <h1 class="hero-title">订单 <span class="mono order-id" id="order-id">{order_id}</span></h1>
    <p class="hero-sub">共 {total_qty} 件商品 · 合计</p>
    <p class="price-hero"><span class="currency">¥</span>{total}</p>
    <div class="hero-actions">
      <button class="btn btn-primary" type="button" data-copy="order-id">复制订单号</button>
      <button class="btn btn-ghost" type="button" data-demo-action="客服会话已创建（示例）">联系客服</button>
    </div>
  </section>
  <section class="section" style="--delay: 60ms">
    <div class="section-head">
      <h2>商品明细</h2>
      <span class="muted">{total_qty} 件</span>
    </div>
    <ul class="item-list">{items_html}</ul>
  </section>
  <section class="card summary" style="--delay: 120ms">
    <dl class="summary-row"><dt>商品小计</dt><dd>¥{total}</dd></dl>
    <dl class="summary-row summary-total"><dt>应付合计</dt><dd>¥{total}</dd></dl>
  </section>
  {footer}
</main>"#,
        header = page_header(),
        status_class = status_class,
        status_label = status_label,
        order_id = escape_html(&data.order_id),
        total_qty = total_qty,
        total = format_money(total),
        items_html = items_html,
        footer = page_footer(ctx),
    );

    Ok(render_document("订单详情", &body, ctx))
}

/// 渲染 `ORDER_LIST_PAGE`。
pub fn render_order_list(ctx: &SceneContext<'_>) -> Result<String, SceneError> {
    let data: OrderListData = parse_business_data(ctx.business_data, "ORDER_LIST_PAGE")?;
    validate_order_list(&data)?;

    let order_count = data.orders.len();
    let total = data
        .orders
        .iter()
        .map(|order| order.total_amount)
        .sum::<Decimal>();
    let orders_html = render_order_summaries(&data.orders);

    let body = format!(
        r#"{header}
<main class="page">
  <section class="hero card hero-compact" style="--delay: 0ms">
    <span class="eyebrow">订单列表</span>
    <h1 class="hero-title">我的订单</h1>
    <p class="hero-sub">共 {order_count} 笔订单 · 累计 ¥{total}</p>
  </section>
  <section class="section" style="--delay: 60ms">
    <div class="section-head">
      <h2>全部订单</h2>
      <span class="muted">{order_count} 笔</span>
    </div>
    {orders_html}
  </section>
  {footer}
</main>"#,
        header = page_header(),
        order_count = order_count,
        total = format_money(total),
        orders_html = orders_html,
        footer = page_footer(ctx),
    );

    Ok(render_document("订单列表", &body, ctx))
}

/// 渲染 `PRODUCT_DETAIL_PAGE`。
pub fn render_product_detail(ctx: &SceneContext<'_>) -> Result<String, SceneError> {
    let data: ProductDetailData = parse_business_data(ctx.business_data, "PRODUCT_DETAIL_PAGE")?;
    validate_product_detail(&data)?;

    let (stock_class, stock_label) = stock_status_view(data.stock_status.as_deref());
    let out_of_stock = data.stock_status.as_deref() == Some("OUT_OF_STOCK");
    let description_html = data
        .description
        .as_deref()
        .filter(|text| !text.trim().is_empty())
        .map(|text| {
            format!(
                r#"<p class="hero-sub product-desc">{}</p>"#,
                escape_html(text)
            )
        })
        .unwrap_or_default();
    let disabled = if out_of_stock { " disabled" } else { "" };
    let cta_label = if out_of_stock {
        "暂时缺货"
    } else {
        "立即购买"
    };

    let body = format!(
        r#"{header}
<main class="page">
  <section class="hero card product-hero" style="--delay: 0ms">
    <span class="eyebrow">商品详情</span>
    <h1 class="hero-title">{name}</h1>
    <p class="price-hero"><span class="currency">{currency}</span>{price}</p>
    {description_html}
    <div class="badge-row">
      <span class="status {stock_class}"><span class="status-dot" aria-hidden="true"></span>{stock_label}</span>
      <span class="chip">正品保障</span>
      <span class="chip">极速发货</span>
      <span class="chip">7 天无理由</span>
    </div>
    <button class="btn btn-primary btn-block" type="button" data-demo-action="已加入购物车（示例）"{disabled}>{cta_label}</button>
    <p class="product-id muted">商品编号 <span class="mono">{product_id}</span></p>
  </section>
  <section class="section feature-grid" style="--delay: 60ms">
    <article class="card feature"><h2>严选品质</h2><p>平台直供，支持验货与售后保障。</p></article>
    <article class="card feature"><h2>快速履约</h2><p>现货订单优先出库，物流状态实时同步。</p></article>
    <article class="card feature"><h2>安心售后</h2><p>支持 7 天无理由退换，客服全程协助。</p></article>
  </section>
  {footer}
</main>"#,
        header = page_header(),
        name = escape_html(&data.name),
        currency = currency_prefix(data.currency.as_deref()),
        price = format_money(data.price),
        description_html = description_html,
        stock_class = stock_class,
        stock_label = stock_label,
        disabled = disabled,
        cta_label = cta_label,
        product_id = escape_html(&data.product_id),
        footer = page_footer(ctx),
    );

    Ok(render_document("商品详情", &body, ctx))
}

/// 反序列化场景业务数据，失败时给出字段级错误。
fn parse_business_data<T>(value: &Value, scene_code: &str) -> Result<T, SceneError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(value.clone()).map_err(|error| {
        SceneError::invalid(format!("does not match {scene_code} contract: {error}"))
    })
}

/// 校验订单详情业务数据。
fn validate_order_detail(data: &OrderBusinessData) -> Result<(), SceneError> {
    ensure_text(&data.order_id, "order_id", 64)?;
    ensure_order_status(&data.status, "status")?;
    if data.items.is_empty() {
        return Err(SceneError::invalid("items must contain at least 1 item"));
    }
    if data.items.len() > 100 {
        return Err(SceneError::invalid("items must contain at most 100 items"));
    }
    for (index, item) in data.items.iter().enumerate() {
        ensure_text(&item.name, &format!("items[{index}].name"), 200)?;
        if item.qty <= 0 || item.qty > 9999 {
            return Err(SceneError::invalid(format!(
                "items[{index}].qty must be within 1..=9999"
            )));
        }
        ensure_money(item.price, &format!("items[{index}].price"))?;
    }
    Ok(())
}

/// 校验订单列表业务数据。
fn validate_order_list(data: &OrderListData) -> Result<(), SceneError> {
    if data.orders.len() > 50 {
        return Err(SceneError::invalid("orders must contain at most 50 items"));
    }
    for (index, order) in data.orders.iter().enumerate() {
        ensure_text(&order.order_id, &format!("orders[{index}].order_id"), 64)?;
        ensure_order_status(&order.status, &format!("orders[{index}].status"))?;
        ensure_money(order.total_amount, &format!("orders[{index}].total_amount"))?;
        if let Some(currency) = &order.currency {
            ensure_currency(currency, &format!("orders[{index}].currency"))?;
        }
        if let Some(item_count) = order.item_count {
            if item_count > 9999 {
                return Err(SceneError::invalid(format!(
                    "orders[{index}].item_count must be <= 9999"
                )));
            }
        }
    }
    Ok(())
}

/// 校验商品详情业务数据。
fn validate_product_detail(data: &ProductDetailData) -> Result<(), SceneError> {
    ensure_text(&data.product_id, "product_id", 64)?;
    ensure_text(&data.name, "name", 200)?;
    ensure_money(data.price, "price")?;
    if let Some(currency) = &data.currency {
        ensure_currency(currency, "currency")?;
    }
    if let Some(description) = &data.description {
        if description.chars().count() > 2000 {
            return Err(SceneError::invalid(
                "description must be at most 2000 characters",
            ));
        }
    }
    if let Some(stock_status) = &data.stock_status {
        if !VALID_STOCK_STATUSES.contains(&stock_status.as_str()) {
            return Err(SceneError::invalid(format!(
                "stock_status must be one of {}",
                VALID_STOCK_STATUSES.join(", ")
            )));
        }
    }
    Ok(())
}

/// 校验非空文本字段。
fn ensure_text(value: &str, field: &str, max_chars: usize) -> Result<(), SceneError> {
    if value.trim().is_empty() {
        return Err(SceneError::invalid(format!("{field} must not be empty")));
    }
    if value.chars().count() > max_chars {
        return Err(SceneError::invalid(format!(
            "{field} must be at most {max_chars} characters"
        )));
    }
    Ok(())
}

/// 校验订单状态枚举。
fn ensure_order_status(value: &str, field: &str) -> Result<(), SceneError> {
    if !VALID_ORDER_STATUSES.contains(&value) {
        return Err(SceneError::invalid(format!(
            "{field} must be one of {}",
            VALID_ORDER_STATUSES.join(", ")
        )));
    }
    Ok(())
}

/// 校验货币代码（三位大写字母）。
fn ensure_currency(value: &str, field: &str) -> Result<(), SceneError> {
    let bytes = value.as_bytes();
    if bytes.len() != 3 || !bytes.iter().all(u8::is_ascii_uppercase) {
        return Err(SceneError::invalid(format!(
            "{field} must be a 3-letter uppercase currency code"
        )));
    }
    Ok(())
}

/// 校验金额：非负且最多两位小数。
fn ensure_money(value: Decimal, field: &str) -> Result<(), SceneError> {
    if value < Decimal::ZERO {
        return Err(SceneError::invalid(format!("{field} must be >= 0")));
    }
    if value.normalize().scale() > 2 {
        return Err(SceneError::invalid(format!(
            "{field} must have at most 2 decimal places"
        )));
    }
    Ok(())
}

/// 订单总金额。
fn order_total(items: &[OrderBusinessItem]) -> Decimal {
    items
        .iter()
        .map(|item| item.price * Decimal::from(item.qty))
        .sum()
}

/// 渲染订单明细列表。
fn render_order_items(items: &[OrderBusinessItem]) -> String {
    let mut html = String::new();
    for item in items {
        let line_total = item.price * Decimal::from(item.qty);
        write!(
            html,
            r#"<li class="item">
  <div class="item-main">
    <span class="item-name">{name}</span>
    <span class="item-meta">单价 ¥{price} × {qty}</span>
  </div>
  <span class="item-amount">¥{line_total}</span>
</li>"#,
            name = escape_html(&item.name),
            price = format_money(item.price),
            qty = item.qty,
            line_total = format_money(line_total),
        )
        .expect("writing to a String cannot fail");
    }
    html
}

/// 渲染订单列表卡片。
fn render_order_summaries(orders: &[OrderSummary]) -> String {
    if orders.is_empty() {
        return r#"<div class="card empty-state">
  <p class="empty-title">暂无订单</p>
  <p class="muted">下单后可以在这里查看订单进度。</p>
</div>"#
            .to_string();
    }

    let mut html = String::new();
    html.push_str(r#"<div class="order-list">"#);
    for order in orders {
        let (status_class, status_label) = order_status_view(&order.status);
        let count_label = order
            .item_count
            .map(|count| format!("{count} 件商品"))
            .unwrap_or_else(|| "商品数量未提供".to_string());
        write!(
            html,
            r#"<article class="card order-card">
  <div class="order-card-top">
    <span class="mono order-id">{order_id}</span>
    <span class="status {status_class}"><span class="status-dot" aria-hidden="true"></span>{status_label}</span>
  </div>
  <div class="order-card-bottom">
    <span class="muted">{count_label}</span>
    <span class="amount">{currency}{total}</span>
  </div>
</article>"#,
            order_id = escape_html(&order.order_id),
            status_class = status_class,
            status_label = status_label,
            count_label = escape_html(&count_label),
            currency = currency_prefix(order.currency.as_deref()),
            total = format_money(order.total_amount),
        )
        .expect("writing to a String cannot fail");
    }
    html.push_str("</div>");
    html
}

/// 订单状态 → (CSS 类, 中文标签)。
fn order_status_view(status: &str) -> (&'static str, &'static str) {
    match status {
        "PENDING" => ("status-pending", "待支付"),
        "PAID" => ("status-paid", "已支付"),
        "SHIPPED" => ("status-shipped", "已发货"),
        "COMPLETED" => ("status-completed", "已完成"),
        "CANCELLED" => ("status-cancelled", "已取消"),
        _ => ("status-neutral", "未知状态"),
    }
}

/// 库存状态 → (CSS 类, 中文标签)。
fn stock_status_view(status: Option<&str>) -> (&'static str, &'static str) {
    match status {
        Some("IN_STOCK") => ("status-instock", "现货"),
        Some("LOW_STOCK") => ("status-lowstock", "库存紧张"),
        Some("OUT_OF_STOCK") => ("status-outstock", "暂时缺货"),
        _ => ("status-neutral", "库存待确认"),
    }
}

/// 货币符号；未知币种回退为「代码 + 空格」。
fn currency_prefix(currency: Option<&str>) -> String {
    match currency.unwrap_or("CNY") {
        "CNY" => "¥".to_string(),
        "USD" => "$".to_string(),
        "EUR" => "€".to_string(),
        other => format!("{other} "),
    }
}

/// 金额格式化：固定两位小数，千分位不分组（移动端更易读）。
fn format_money(amount: Decimal) -> String {
    let rounded = amount.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero);
    let mut text = rounded.to_string();
    match text.find('.') {
        Some(dot) => {
            let decimals = text.len().saturating_sub(dot + 1);
            if decimals < 2 {
                text.push_str(&"0".repeat(2 - decimals));
            }
        }
        None => text.push_str(".00"),
    }
    text
}

/// HTML 转义：覆盖文本节点与属性值。
fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

/// 页面顶部品牌栏与主题切换按钮。
fn page_header() -> &'static str {
    r#"<header class="topbar">
  <div class="brand">
    <span class="brand-mark" aria-hidden="true"></span>
    <span class="brand-name">InTimeUI</span>
  </div>
  <button class="theme-toggle" id="theme-toggle" type="button" aria-label="切换明暗主题" aria-pressed="false">
    <span class="theme-glyph theme-glyph-sun" aria-hidden="true">☀</span>
    <span class="theme-glyph theme-glyph-moon" aria-hidden="true">☾</span>
  </button>
</header>"#
}

/// 页面底部版本信息。
fn page_footer(ctx: &SceneContext<'_>) -> String {
    format!(
        r#"<footer class="footer">
  <span>由 InTimeUI 实时生成</span>
  <span class="footer-sep" aria-hidden="true"></span>
  <span class="mono">模板 {template_version}</span>
</footer>"#,
        template_version = escape_html(ctx.template_version),
    )
}

/// 组装完整单文件 HTML。
fn render_document(title: &str, body: &str, ctx: &SceneContext<'_>) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN" data-platform="{platform}" data-scene="{scene}" data-version="{version}" style="--screen-width: {width}px; --screen-height: {height}px;">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<meta name="color-scheme" content="light dark">
<meta name="intimeui-template" content="{scene}:{template_version}">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src data:; base-uri 'none'; form-action 'none'">
<title>{title} · InTimeUI</title>
<style>{css}</style>
</head>
<body>
{body}
<div class="toast" id="toast" role="status" aria-live="polite"></div>
<script>{js}</script>
</body>
</html>"#,
        platform = escape_html(ctx.platform),
        scene = escape_html(ctx.scene_code),
        version = escape_html(ctx.version),
        width = ctx.screen_width,
        height = ctx.screen_height,
        template_version = escape_html(ctx.template_version),
        title = escape_html(title),
        css = BASE_CSS,
        body = body,
        js = BASE_JS,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::str::FromStr;

    fn context<'a>(scene_code: &'a str, business_data: &'a Value) -> SceneContext<'a> {
        SceneContext {
            scene_code,
            screen_width: 390,
            screen_height: 844,
            platform: "android",
            business_data,
            version: "template:ORDER_DETAIL_PAGE:v1",
            template_version: "v1",
        }
    }

    #[test]
    fn escape_html_covers_special_characters() {
        assert_eq!(
            escape_html(r#"<script>alert("x&y")</script>"#),
            "&lt;script&gt;alert(&quot;x&amp;y&quot;)&lt;/script&gt;"
        );
        assert_eq!(escape_html("it's"), "it&#39;s");
    }

    #[test]
    fn format_money_pads_two_decimals() {
        assert_eq!(format_money(Decimal::from(299)), "299.00");
        assert_eq!(format_money(Decimal::from_str("299.9").unwrap()), "299.90");
        assert_eq!(format_money(Decimal::ZERO), "0.00");
        assert_eq!(format_money(Decimal::from_str("0.005").unwrap()), "0.01");
    }

    #[test]
    fn order_detail_escapes_item_names() {
        let data = json!({
            "order_id": "ORD_1",
            "status": "PAID",
            "items": [{ "name": "<img src=x onerror=alert(1)>", "price": 10.00, "qty": 1 }]
        });
        let html = render_order_detail(&context("ORDER_DETAIL_PAGE", &data)).unwrap();
        assert!(html.contains("&lt;img src=x onerror=alert(1)&gt;"));
        assert!(!html.contains("<img src=x"));
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("</html>"));
        assert!(html.contains("Content-Security-Policy"));
    }

    #[test]
    fn order_detail_rejects_negative_price() {
        let data = json!({
            "order_id": "ORD_1",
            "status": "PAID",
            "items": [{ "name": "item", "price": -1.00, "qty": 1 }]
        });
        let error = render_order_detail(&context("ORDER_DETAIL_PAGE", &data)).unwrap_err();
        assert!(matches!(error, SceneError::InvalidBusinessData(_)));
    }

    #[test]
    fn order_list_renders_empty_state() {
        let data = json!({ "orders": [] });
        let html = render_order_list(&context("ORDER_LIST_PAGE", &data)).unwrap();
        assert!(html.contains("暂无订单"));
    }

    #[test]
    fn product_detail_marks_out_of_stock() {
        let data = json!({
            "product_id": "PRD_1",
            "name": "商品",
            "price": 99.00,
            "stock_status": "OUT_OF_STOCK"
        });
        let html = render_product_detail(&context("PRODUCT_DETAIL_PAGE", &data)).unwrap();
        assert!(html.contains("暂时缺货"));
        assert!(html.contains("disabled"));
    }
}
