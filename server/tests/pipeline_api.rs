//! 集成测试：AI 生成管线编排与降级兜底（REQ-202610-1185）。
//!
//! 覆盖：
//! - 正常路径端到端 ≤ 1.5s（mock provider，输出测量值）；
//! - 模型超时 → 秒级切换静态模板，页面完整可渲染（无白屏）；
//! - 输出校验失败 → 降级；
//! - 缓存命中不重复调用模型；
//! - 降级指标可通过 `/api/v1/ui/pipeline/metrics` 观测；
//! - 混淆阶段失败 → 降级；混淆成功 → 标记 `obfuscated`；
//! - 版本串随输入变化（可追溯）。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use intimeui_server::build_app;
use intimeui_server::config::AppConfig;
use intimeui_server::db::Db;
use intimeui_server::model::{
    ChatRequest, ChatResponse, ModelConfig, ModelError, ModelGateway, ModelProvider, ModelStream,
    StreamChunk, Usage,
};
use intimeui_server::pipeline::{
    HtmlObfuscator, ObfuscationOutput, Pipeline, PipelineConfig, PipelineRequest,
};
use intimeui_server::state::AppState;
use intimeui_server::ui_template::DeviceInfo;
use serde_json::{json, Value};
use tower::ServiceExt;

const VALID_HTML: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>订单详情</title>
  <style>body{margin:0;background:#fff;color:#111}main{padding:24px}</style>
</head>
<body><main><h1>订单详情</h1><p>ORD_20261014_001</p></main></body>
</html>"#;

fn test_config() -> AppConfig {
    AppConfig {
        rate_limit_enabled: false,
        auth_dev_mode: true,
        model: ModelConfig {
            mock_latency_ms: 20,
            mock_stream_delay_ms: 0,
            ..ModelConfig::default()
        },
        ..AppConfig::default()
    }
}

async fn db() -> Db {
    let db = Db::connect_in_memory()
        .await
        .expect("failed to open in-memory database");
    db.migrate().await.expect("failed to run migrations");
    db.seed().await.expect("failed to load seed data");
    db
}

async fn app_with(config: AppConfig) -> axum::Router {
    let db = db().await;
    build_app(AppState::new(config, db))
}

async fn app_with_gateway(config: AppConfig, gateway: Arc<ModelGateway>) -> axum::Router {
    let db = db().await;
    build_app(AppState::with_gateway(config, db, gateway))
}

fn post_json(path: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn get(path: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(path)
        .body(Body::empty())
        .unwrap()
}

fn with_bearer(mut request: Request<Body>) -> Request<Body> {
    request.headers_mut().insert(
        header::AUTHORIZATION,
        "Bearer dev:usr_9527".parse().unwrap(),
    );
    request
}

async fn send_json(app: &axum::Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.expect("request failed");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("failed to read body")
        .to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("response body is not valid JSON")
    };
    (status, value)
}

fn order_body(order_id: &str) -> Value {
    json!({
        "user_id": "usr_9527",
        "scene_code": "ORDER_DETAIL_PAGE",
        "device_info": {"screen_width": 390, "screen_height": 844, "platform": "android"},
        "business_data": {
            "order_id": order_id,
            "status": "PAID",
            "items": [{"name": "智能降噪耳机", "price": 299.00, "qty": 1}]
        }
    })
}

/// 可计数、可注入输出内容的 provider。
struct StubProvider {
    content: String,
    calls: Arc<AtomicUsize>,
    latency: Duration,
}

impl StubProvider {
    fn new(content: impl Into<String>, latency: Duration) -> (Self, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        (
            Self {
                content: content.into(),
                calls: calls.clone(),
                latency,
            },
            calls,
        )
    }
}

#[async_trait]
impl ModelProvider for StubProvider {
    fn name(&self) -> &'static str {
        "stub"
    }

    async fn generate(&self, _request: ChatRequest) -> Result<ChatResponse, ModelError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(self.latency).await;
        Ok(ChatResponse {
            content: self.content.clone(),
            finish_reason: Some("stop".to_string()),
            usage: Usage {
                prompt_tokens: 10,
                completion_tokens: 20,
                total_tokens: 30,
            },
        })
    }

    async fn generate_stream(&self, _request: ChatRequest) -> Result<ModelStream, ModelError> {
        Ok(Box::pin(futures_util::stream::empty::<
            Result<StreamChunk, ModelError>,
        >()))
    }
}

fn gateway_with(provider: Arc<dyn ModelProvider>) -> Arc<ModelGateway> {
    Arc::new(ModelGateway::new(ModelConfig::default(), provider))
}

#[tokio::test]
async fn normal_path_meets_first_screen_budget() {
    let app = app_with(test_config()).await;
    let (status, body) = send_json(
        &app,
        with_bearer(post_json(
            "/api/v1/ui/generate/dynamic",
            order_body("ORD_20261014_001"),
        )),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], 0);
    let data = &body["data"];
    assert_eq!(data["render_type"], "SINGLE_FILE_HTML");
    assert_eq!(data["strategy"], "dynamic");
    assert_eq!(data["degraded"], false);
    assert_eq!(data["cache_hit"], false);
    assert!(data["version"]
        .as_str()
        .unwrap()
        .starts_with("dynamic:ORDER_DETAIL_PAGE:v1-"));
    let html = data["html_content"].as_str().unwrap();
    assert!(html.starts_with("<!DOCTYPE html>"));
    assert!(html.trim_end().ends_with("</html>"));
    let total_ms = data["timings"]["total_ms"].as_f64().unwrap();
    assert!(
        total_ms < 1500.0,
        "normal path exceeded 1.5s budget: {total_ms}ms"
    );
    println!(
        "normal path measured: total={total_ms}ms model={}ms validation={}ms obfuscation={}ms",
        data["timings"]["model_ms"],
        data["timings"]["validation_ms"],
        data["timings"]["obfuscation_ms"]
    );
}

#[tokio::test]
async fn model_timeout_degrades_to_template_without_white_screen() {
    let mut config = test_config();
    config.model.mock_latency_ms = 5_000;
    config.pipeline = PipelineConfig {
        timeout_ms: 300,
        fallback_reserve_ms: 50,
        ..PipelineConfig::default()
    };
    let app = app_with(config).await;
    let (status, body) = send_json(
        &app,
        with_bearer(post_json(
            "/api/v1/ui/generate/dynamic",
            order_body("ORD_20261014_001"),
        )),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let data = &body["data"];
    assert_eq!(data["degraded"], true);
    assert_eq!(data["degradation_reason"], "model_timeout");
    assert_eq!(data["strategy"], "template");
    assert!(data["version"]
        .as_str()
        .unwrap()
        .starts_with("template:ORDER_DETAIL_PAGE:"));
    let html = data["html_content"].as_str().unwrap();
    assert!(html.starts_with("<!DOCTYPE html>"));
    assert!(html.trim_end().ends_with("</html>"));
    assert!(html.contains("ORD_20261014_001"));
    let total_ms = data["timings"]["total_ms"].as_f64().unwrap();
    assert!(
        total_ms < 1_000.0,
        "degradation took too long: {total_ms}ms"
    );
    println!("degradation drill measured: total={total_ms}ms");
}

#[tokio::test]
async fn validation_failure_degrades_to_template() {
    let (provider, _calls) = StubProvider::new(
        "<html><body><p>not a complete single-file page</p></body></html>",
        Duration::from_millis(1),
    );
    let app = app_with_gateway(test_config(), gateway_with(Arc::new(provider))).await;
    let (status, body) = send_json(
        &app,
        with_bearer(post_json(
            "/api/v1/ui/generate/dynamic",
            order_body("ORD_20261014_001"),
        )),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let data = &body["data"];
    assert_eq!(data["degraded"], true);
    assert_eq!(data["degradation_reason"], "validation_failed");
    assert!(data["html_content"]
        .as_str()
        .unwrap()
        .contains("ORD_20261014_001"));
}

#[tokio::test]
async fn cache_hit_skips_model_call() {
    let (provider, calls) = StubProvider::new(VALID_HTML, Duration::from_millis(1));
    let app = app_with_gateway(test_config(), gateway_with(Arc::new(provider))).await;

    let first = send_json(
        &app,
        with_bearer(post_json(
            "/api/v1/ui/generate/dynamic",
            order_body("ORD_CACHE_001"),
        )),
    )
    .await;
    assert_eq!(first.0, StatusCode::OK);
    assert_eq!(first.1["data"]["cache_hit"], false);

    let second = send_json(
        &app,
        with_bearer(post_json(
            "/api/v1/ui/generate/dynamic",
            order_body("ORD_CACHE_001"),
        )),
    )
    .await;
    assert_eq!(second.0, StatusCode::OK);
    assert_eq!(second.1["data"]["cache_hit"], true);
    assert_eq!(calls.load(Ordering::SeqCst), 1, "cache hit must skip model");
}

#[tokio::test]
async fn metrics_expose_degradation_and_budget() {
    let mut config = test_config();
    config.model.mock_latency_ms = 5_000;
    config.pipeline = PipelineConfig {
        timeout_ms: 250,
        fallback_reserve_ms: 50,
        ..PipelineConfig::default()
    };
    let app = app_with(config).await;
    let _ = send_json(
        &app,
        with_bearer(post_json(
            "/api/v1/ui/generate/dynamic",
            order_body("ORD_20261014_001"),
        )),
    )
    .await;

    let (status, body) = send_json(&app, with_bearer(get("/api/v1/ui/pipeline/metrics"))).await;
    assert_eq!(status, StatusCode::OK);
    let data = &body["data"];
    assert_eq!(data["total_requests"], 1);
    assert_eq!(data["degraded_requests"], 1);
    assert_eq!(data["dynamic_success"], 0);
    assert_eq!(data["degradation_reasons"][0]["reason"], "model_timeout");
    assert_eq!(data["budget_ms"], 250);
    assert_eq!(data["within_budget"], 1);
}

#[tokio::test]
async fn version_is_content_addressed() {
    let (provider, _calls) = StubProvider::new(VALID_HTML, Duration::from_millis(1));
    let app = app_with_gateway(test_config(), gateway_with(Arc::new(provider))).await;

    let first = send_json(
        &app,
        with_bearer(post_json(
            "/api/v1/ui/generate/dynamic",
            order_body("ORD_VERSION_A"),
        )),
    )
    .await;
    let second = send_json(
        &app,
        with_bearer(post_json(
            "/api/v1/ui/generate/dynamic",
            order_body("ORD_VERSION_B"),
        )),
    )
    .await;
    assert_ne!(
        first.1["data"]["version"], second.1["data"]["version"],
        "different inputs must produce different versions"
    );
}

struct UpperCaseObfuscator;

impl HtmlObfuscator for UpperCaseObfuscator {
    fn obfuscate(
        &self,
        html: &str,
        _session_id: Option<&str>,
    ) -> Result<ObfuscationOutput, String> {
        Ok(ObfuscationOutput {
            html: format!("<!-- obfuscated -->{html}"),
            applied: true,
        })
    }
}

struct FailingObfuscator;

impl HtmlObfuscator for FailingObfuscator {
    fn obfuscate(
        &self,
        _html: &str,
        _session_id: Option<&str>,
    ) -> Result<ObfuscationOutput, String> {
        Err("synthetic obfuscation failure".to_string())
    }
}

fn pipeline_request(order_id: &str) -> PipelineRequest {
    PipelineRequest {
        user_id: "usr_9527".to_string(),
        scene_code: "ORDER_DETAIL_PAGE".to_string(),
        device: DeviceInfo {
            screen_width: 390,
            screen_height: 844,
            platform: "android".to_string(),
        },
        business_data: json!({
            "order_id": order_id,
            "status": "PAID",
            "items": [{"name": "智能降噪耳机", "price": 299.00, "qty": 1}]
        }),
        prompt: None,
        model: None,
        session_id: Some("session-1".to_string()),
        bypass_cache: true,
    }
}

#[tokio::test]
async fn obfuscation_stage_runs_and_is_marked() {
    let (provider, _calls) = StubProvider::new(VALID_HTML, Duration::from_millis(1));
    let gateway = gateway_with(Arc::new(provider));
    let db = Arc::new(db().await);
    let pipeline = Pipeline::with_obfuscator(
        PipelineConfig::default(),
        gateway,
        db,
        Arc::new(UpperCaseObfuscator),
    );
    let outcome = pipeline
        .generate(pipeline_request("ORD_OBF_001"))
        .await
        .expect("pipeline should succeed");
    assert!(!outcome.degraded);
    assert!(outcome.obfuscated);
    assert!(outcome.html_content.starts_with("<!-- obfuscated -->"));
}

#[tokio::test]
async fn obfuscation_failure_degrades_to_template() {
    let (provider, _calls) = StubProvider::new(VALID_HTML, Duration::from_millis(1));
    let gateway = gateway_with(Arc::new(provider));
    let db = Arc::new(db().await);
    let pipeline = Pipeline::with_obfuscator(
        PipelineConfig::default(),
        gateway,
        db,
        Arc::new(FailingObfuscator),
    );
    let outcome = pipeline
        .generate(pipeline_request("ORD_OBF_002"))
        .await
        .expect("fallback should still produce a page");
    assert!(outcome.degraded);
    assert_eq!(
        outcome.degradation_reason,
        Some(intimeui_server::pipeline::DegradationReason::ObfuscationFailed)
    );
    assert!(outcome.html_content.contains("ORD_OBF_002"));
}
