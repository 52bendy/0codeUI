//! 静态模板引擎（REQ-202610-1172）行为测试。
//!
//! 覆盖：占位符填充与 HTML 转义、区块迭代 / 反向区块、错误处理、
//! 订单详情模板渲染、动态管线兜底入口与 P95 ≤ 200ms 性能门槛。

use std::time::Instant;

use intimeui_server::ui_template::{DeviceInfo, TemplateEngine, TemplateError};
use serde_json::json;

fn device() -> DeviceInfo {
    DeviceInfo {
        screen_width: 390,
        screen_height: 844,
        platform: "android".to_string(),
    }
}

#[test]
fn renders_scalar_placeholders_with_html_escaping() {
    let html = TemplateEngine::render_str(
        "<p>你好 {{name}}</p>",
        &json!({ "name": "<script>alert('x')</script>" }),
    )
    .expect("template should render");

    assert!(html.contains("&lt;script&gt;alert(&#x27;x&#x27;)&lt;/script&gt;"));
    assert!(!html.contains("<script>alert"));
}

#[test]
fn renders_sections_and_inverted_sections() {
    let template = "{{#items}}[{{index}}:{{name}}]{{/items}}{{^items}}empty{{/items}}";

    let filled = TemplateEngine::render_str(
        template,
        &json!({ "items": [{ "name": "耳机" }, { "name": "键盘" }] }),
    )
    .unwrap();
    assert_eq!(filled, "[1:耳机][2:键盘]");

    let empty = TemplateEngine::render_str(template, &json!({ "items": [] })).unwrap();
    assert_eq!(empty, "empty");
}

#[test]
fn unknown_placeholder_is_rejected() {
    let error = TemplateEngine::render_str("{{missing}}", &json!({})).unwrap_err();
    assert!(matches!(error, TemplateError::Render(_)));
    assert!(error.to_string().contains("missing"));
}

#[test]
fn unclosed_section_is_rejected() {
    let error = TemplateEngine::render_str("{{#items}}x", &json!({})).unwrap_err();
    assert!(matches!(error, TemplateError::Render(_)));
}

#[test]
fn renders_order_detail_page_with_inline_fetch_binding() {
    let data = json!({
        "order_id": "ORD_20261014_001",
        "status": "PAID",
        "items": [
            { "name": "智能降噪耳机", "price": 299.00, "qty": 1 }
        ]
    });

    let page = TemplateEngine::global()
        .render("ORDER_DETAIL_PAGE", &data, &device())
        .expect("order detail should render");

    assert_eq!(page.scene_code, "ORDER_DETAIL_PAGE");
    assert_eq!(page.strategy, "template");
    assert_eq!(page.template_version, "v1");
    assert!(page.html.starts_with("<!DOCTYPE html>"));
    assert!(page.html.contains("ORD_20261014_001"));
    assert!(page.html.contains("智能降噪耳机"));
    assert!(page.html.contains("299.00"));
    assert!(page
        .html
        .contains("onclick=\"fetch('/api/v1/orders/ORD_20261014_001/business-data'"));
    assert!(page.html.contains("刷新订单状态"));
}

#[test]
fn order_detail_escapes_malicious_business_data() {
    let data = json!({
        "order_id": "ORD_XSS_001",
        "status": "PAID",
        "items": [
            { "name": "<img src=x onerror=alert(1)>", "price": 1.00, "qty": 1 }
        ]
    });

    let page = TemplateEngine::global()
        .render("ORDER_DETAIL_PAGE", &data, &device())
        .expect("order detail should render");

    assert!(!page.html.contains("<img src=x"));
    assert!(page.html.contains("&lt;img src=x onerror=alert(1)&gt;"));
}

#[test]
fn order_detail_rejects_invalid_business_data() {
    let invalid_status = json!({
        "order_id": "ORD_1",
        "status": "NOT_A_STATUS",
        "items": [{ "name": "x", "price": 1.00, "qty": 1 }]
    });
    let error = TemplateEngine::global()
        .render("ORDER_DETAIL_PAGE", &invalid_status, &device())
        .unwrap_err();
    assert!(matches!(error, TemplateError::InvalidBusinessData(_)));

    let empty_items = json!({
        "order_id": "ORD_1",
        "status": "PAID",
        "items": []
    });
    let error = TemplateEngine::global()
        .render("ORDER_DETAIL_PAGE", &empty_items, &device())
        .unwrap_err();
    assert!(matches!(error, TemplateError::InvalidBusinessData(_)));
}

#[test]
fn unknown_scene_is_rejected() {
    let error = TemplateEngine::global()
        .render("NOPE_PAGE", &json!({}), &device())
        .unwrap_err();
    assert!(matches!(error, TemplateError::SceneNotFound(_)));
}

#[test]
fn fallback_entry_always_uses_template_strategy() {
    let data = json!({
        "order_id": "ORD_20261014_001",
        "status": "PAID",
        "items": [{ "name": "智能降噪耳机", "price": 299.00, "qty": 1 }]
    });

    let page = TemplateEngine::global()
        .render_fallback("ORDER_DETAIL_PAGE", &data, &device())
        .expect("fallback should render");

    assert_eq!(page.strategy, "template");
    assert!(page.html.contains("ORD_20261014_001"));
}

#[test]
fn order_detail_render_p95_is_under_200ms() {
    let data = json!({
        "order_id": "ORD_20261014_001",
        "status": "PAID",
        "items": [
            { "name": "智能降噪耳机", "price": 299.00, "qty": 1 },
            { "name": "机械键盘", "price": 499.00, "qty": 2 }
        ]
    });
    let engine = TemplateEngine::global();

    for _ in 0..20 {
        engine
            .render("ORDER_DETAIL_PAGE", &data, &device())
            .expect("warm-up render");
    }

    let mut samples = Vec::with_capacity(200);
    for _ in 0..200 {
        let started = Instant::now();
        engine
            .render("ORDER_DETAIL_PAGE", &data, &device())
            .expect("measured render");
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    let percentile = |p: f64| samples[(samples.len() as f64 * p).ceil() as usize - 1];
    let p50 = percentile(0.50);
    let p95 = percentile(0.95);
    let p99 = percentile(0.99);
    println!("order_detail render latency: p50={p50:?} p95={p95:?} p99={p99:?}");

    assert!(
        p95.as_millis() < 200,
        "P95 render latency must be < 200ms, got {p95:?}"
    );
}
