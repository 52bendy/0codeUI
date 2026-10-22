//! OpenAI Chat Completions 兼容提供者（vLLM / Ollama / DeepSeek 等）。
//!
//! 只依赖 hyper / hyper-util，避免引入额外 TLS 栈；生产部署时由同机或内网
//! HTTP 推理服务提供能力，如需 HTTPS 请在反向代理层终止 TLS。

use std::collections::VecDeque;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use futures_util::stream::{self, BoxStream, StreamExt};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use hyper::{Request, Response, StatusCode, Uri};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use serde::{Deserialize, Serialize};

use super::config::{ModelConfig, SecretString};
use super::error::ModelError;
use super::provider::{ModelProvider, ModelStream};
use super::types::{ChatMessage, ChatRequest, ChatResponse, StreamChunk, Usage};

/// OpenAI 兼容推理提供者。
pub struct OpenAiCompatibleProvider {
    client: Client<HttpConnector, Full<Bytes>>,
    endpoint: String,
    api_key: SecretString,
    request_timeout: Duration,
    stream_timeout: Duration,
}

impl OpenAiCompatibleProvider {
    /// 基于模型配置构造提供者。
    pub fn new(config: &ModelConfig) -> Result<Self, ModelError> {
        if config.base_url.starts_with("https://") {
            return Err(ModelError::InvalidRequest(
                "https upstream requires TLS termination at a reverse proxy; use http://"
                    .to_string(),
            ));
        }
        let mut connector = HttpConnector::new();
        connector.set_connect_timeout(Some(Duration::from_millis(config.connect_timeout_ms)));
        let client = Client::builder(TokioExecutor::new()).build(connector);
        let endpoint = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
        Ok(Self {
            client,
            endpoint,
            api_key: config.api_key.clone(),
            request_timeout: Duration::from_millis(config.request_timeout_ms),
            stream_timeout: Duration::from_millis(config.stream_timeout_ms),
        })
    }

    async fn post(
        &self,
        body: &ChatBody<'_>,
        stream: bool,
        timeout: Duration,
    ) -> Result<Response<Incoming>, ModelError> {
        let uri: Uri = self
            .endpoint
            .parse()
            .map_err(|error: hyper::http::uri::InvalidUri| {
                ModelError::InvalidRequest(error.to_string())
            })?;
        let payload = serde_json::to_vec(body)
            .map_err(|error| ModelError::InvalidRequest(error.to_string()))?;
        let mut builder = Request::builder()
            .method("POST")
            .uri(uri)
            .header(CONTENT_TYPE, "application/json")
            .header(
                ACCEPT,
                if stream {
                    "text/event-stream"
                } else {
                    "application/json"
                },
            );
        if !self.api_key.is_empty() {
            builder = builder.header(AUTHORIZATION, format!("Bearer {}", self.api_key.expose()));
        }
        let request = builder
            .body(Full::new(Bytes::from(payload)))
            .map_err(|error| ModelError::InvalidRequest(error.to_string()))?;

        let response = tokio::time::timeout(timeout, self.client.request(request))
            .await
            .map_err(|_| ModelError::Timeout)?
            .map_err(|error| ModelError::Upstream {
                status: 0,
                message: truncate(&error.to_string(), 300),
                retryable: true,
            })?;

        if response.status().is_success() {
            return Ok(response);
        }
        let status = response.status();
        let message = read_error_body(response).await;
        Err(ModelError::Upstream {
            status: status.as_u16(),
            message,
            retryable: status == StatusCode::TOO_MANY_REQUESTS
                || status == StatusCode::REQUEST_TIMEOUT
                || status.is_server_error(),
        })
    }
}

#[async_trait]
impl ModelProvider for OpenAiCompatibleProvider {
    fn name(&self) -> &'static str {
        "openai_compatible"
    }

    async fn generate(&self, request: ChatRequest) -> Result<ChatResponse, ModelError> {
        let body = ChatBody::new(&request, false);
        let response = self.post(&body, false, self.request_timeout).await?;
        let bytes = tokio::time::timeout(self.request_timeout, response.into_body().collect())
            .await
            .map_err(|_| ModelError::Timeout)?
            .map_err(|error| ModelError::Protocol(error.to_string()))?
            .to_bytes();
        let parsed: ChatCompletionResponse = serde_json::from_slice(&bytes)
            .map_err(|error| ModelError::Protocol(format!("invalid JSON: {error}")))?;
        let choice = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| ModelError::Protocol("missing choices".to_string()))?;
        let content = choice
            .message
            .content
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| ModelError::Protocol("empty completion content".to_string()))?;
        Ok(ChatResponse {
            content,
            finish_reason: choice.finish_reason,
            usage: parsed.usage.unwrap_or_default(),
        })
    }

    async fn generate_stream(&self, request: ChatRequest) -> Result<ModelStream, ModelError> {
        let body = ChatBody::new(&request, true);
        let response = self.post(&body, true, self.stream_timeout).await?;
        let body_stream: BoxStream<'static, Result<Bytes, hyper::Error>> =
            Box::pin(response.into_body().into_data_stream());
        let state = StreamState {
            body: body_stream,
            decoder: SseDecoder::default(),
            pending: VecDeque::new(),
            usage: Usage::default(),
            finish_reason: None,
            done: false,
        };
        let timeout = self.stream_timeout;
        let stream = stream::unfold(state, move |mut state| async move {
            if state.done {
                return None;
            }
            loop {
                if let Some(payload) = state.pending.pop_front() {
                    if payload.trim() == "[DONE]" {
                        state.done = true;
                        return Some((
                            Ok(StreamChunk {
                                delta: None,
                                finish_reason: state.finish_reason.take(),
                                usage: Some(state.usage),
                            }),
                            state,
                        ));
                    }
                    match parse_stream_payload(&payload) {
                        Ok(parsed) => {
                            if let Some(usage) = parsed.usage {
                                state.usage.merge(usage);
                            }
                            let choice = parsed.choices.into_iter().next();
                            if let Some(choice) = choice {
                                if let Some(reason) = choice.finish_reason {
                                    state.finish_reason = Some(reason);
                                }
                                if let Some(delta) = choice.delta.content {
                                    if !delta.is_empty() {
                                        return Some((
                                            Ok(StreamChunk {
                                                delta: Some(delta),
                                                finish_reason: None,
                                                usage: None,
                                            }),
                                            state,
                                        ));
                                    }
                                }
                            }
                        }
                        Err(error) => {
                            state.done = true;
                            return Some((Err(error), state));
                        }
                    }
                    continue;
                }
                match tokio::time::timeout(timeout, state.body.next()).await {
                    Err(_) => {
                        state.done = true;
                        return Some((Err(ModelError::Timeout), state));
                    }
                    Ok(None) => return None,
                    Ok(Some(Err(error))) => {
                        state.done = true;
                        return Some((
                            Err(ModelError::Protocol(format!("stream error: {error}"))),
                            state,
                        ));
                    }
                    Ok(Some(Ok(bytes))) => {
                        let payloads = state.decoder.push(&bytes);
                        state.pending.extend(payloads);
                    }
                }
            }
        });
        Ok(Box::pin(stream))
    }
}

#[derive(Serialize)]
struct ChatBody<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    max_tokens: u32,
    temperature: f32,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptions>,
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

impl<'a> ChatBody<'a> {
    fn new(request: &'a ChatRequest, stream: bool) -> Self {
        Self {
            model: &request.model,
            messages: &request.messages,
            max_tokens: request.max_tokens,
            temperature: request.temperature,
            stream,
            stream_options: stream.then_some(StreamOptions {
                include_usage: true,
            }),
        }
    }
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<CompletionChoice>,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct CompletionChoice {
    message: CompletionMessage,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct CompletionMessage {
    content: Option<String>,
}

#[derive(Deserialize)]
struct StreamResponse {
    #[serde(default)]
    choices: Vec<StreamChoice>,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
    finish_reason: Option<String>,
}

#[derive(Deserialize, Default)]
struct StreamDelta {
    content: Option<String>,
}

fn parse_stream_payload(payload: &str) -> Result<StreamResponse, ModelError> {
    serde_json::from_str(payload)
        .map_err(|error| ModelError::Protocol(format!("invalid stream JSON: {error}")))
}

async fn read_error_body(response: Response<Incoming>) -> String {
    let collected =
        tokio::time::timeout(Duration::from_secs(2), response.into_body().collect()).await;
    match collected {
        Ok(Ok(body)) => {
            let text = String::from_utf8_lossy(&body.to_bytes()).trim().to_string();
            if text.is_empty() {
                "upstream returned an empty error body".to_string()
            } else {
                truncate(&text, 300)
            }
        }
        _ => "upstream error body unavailable".to_string(),
    }
}

fn truncate(value: &str, max: usize) -> String {
    let mut result: String = value.chars().take(max).collect();
    if value.chars().count() > max {
        result.push('…');
    }
    result
}

struct StreamState {
    body: BoxStream<'static, Result<Bytes, hyper::Error>>,
    decoder: SseDecoder,
    pending: VecDeque<String>,
    usage: Usage,
    finish_reason: Option<String>,
    done: bool,
}

/// 跨分片的 SSE 解码器。
#[derive(Default)]
struct SseDecoder {
    buffer: Vec<u8>,
    current: Vec<String>,
}

impl SseDecoder {
    fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some(position) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=position).collect();
            let text = String::from_utf8_lossy(&line[..line.len().saturating_sub(1)])
                .trim_end_matches('\r')
                .to_string();
            if text.is_empty() {
                if !self.current.is_empty() {
                    events.push(self.current.join("\n"));
                    self.current.clear();
                }
            } else if let Some(data) = text.strip_prefix("data:") {
                self.current.push(data.trim_start().to_string());
            }
        }
        events
    }
}

/// 等待流结束（供测试与 CLI 复用）。
pub async fn collect_stream(mut stream: ModelStream) -> Result<(String, Usage), ModelError> {
    let mut text = String::new();
    let mut usage = Usage::default();
    while let Some(item) = stream.next().await {
        let chunk = item?;
        if let Some(delta) = chunk.delta {
            text.push_str(&delta);
        }
        if let Some(value) = chunk.usage {
            usage.merge(value);
        }
    }
    Ok((text, usage))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::config::ProviderKind;
    use axum::http::header;
    use axum::routing::post;
    use axum::{Json, Router};
    use serde_json::{json, Value};

    async fn spawn_upstream(app: Router) -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind test upstream");
        let port = listener.local_addr().expect("missing local address").port();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("test upstream crashed");
        });
        port
    }

    fn provider_config(port: u16) -> ModelConfig {
        ModelConfig {
            provider: ProviderKind::OpenAiCompatible,
            base_url: format!("http://127.0.0.1:{port}/v1"),
            api_key: SecretString::new("test-key"),
            request_timeout_ms: 2000,
            stream_timeout_ms: 2000,
            ..ModelConfig::default()
        }
    }

    fn chat_request() -> ChatRequest {
        ChatRequest {
            model: "test-model".to_string(),
            messages: vec![ChatMessage::user("生成页面")],
            max_tokens: 32,
            temperature: 0.0,
            stream: false,
        }
    }

    async fn completion_handler(Json(body): Json<Value>) -> Json<Value> {
        assert_eq!(body["model"], "test-model");
        assert_eq!(body["stream"], false);
        Json(json!({
            "choices": [{
                "message": { "content": "<!DOCTYPE html><html><head></head><body>ok</body></html>" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 3, "completion_tokens": 4, "total_tokens": 7 }
        }))
    }

    async fn stream_handler() -> impl axum::response::IntoResponse {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"<!DOCTYPE html>\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"<html></html>\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":4,\"total_tokens\":7}}\n\n",
            "data: [DONE]\n\n"
        );
        (
            [(header::CONTENT_TYPE, "text/event-stream")],
            body.to_string(),
        )
    }

    #[tokio::test]
    async fn generate_calls_openai_compatible_endpoint() {
        let app = Router::new().route("/v1/chat/completions", post(completion_handler));
        let port = spawn_upstream(app).await;
        let provider = OpenAiCompatibleProvider::new(&provider_config(port)).unwrap();
        let response = provider.generate(chat_request()).await.unwrap();
        assert!(response.content.starts_with("<!DOCTYPE html>"));
        assert_eq!(response.finish_reason.as_deref(), Some("stop"));
        assert_eq!(response.usage.total_tokens, 7);
    }

    #[tokio::test]
    async fn generate_stream_collects_sse_deltas() {
        let app = Router::new().route("/v1/chat/completions", post(stream_handler));
        let port = spawn_upstream(app).await;
        let provider = OpenAiCompatibleProvider::new(&provider_config(port)).unwrap();
        let mut request = chat_request();
        request.stream = true;
        let stream = provider.generate_stream(request).await.unwrap();
        let (text, usage) = collect_stream(stream).await.unwrap();
        assert_eq!(text, "<!DOCTYPE html><html></html>");
        assert_eq!(usage.total_tokens, 7);
    }

    #[tokio::test]
    async fn upstream_server_error_is_retryable() {
        async fn failure() -> (StatusCode, &'static str) {
            (StatusCode::INTERNAL_SERVER_ERROR, "boom")
        }
        let app = Router::new().route("/v1/chat/completions", post(failure));
        let port = spawn_upstream(app).await;
        let provider = OpenAiCompatibleProvider::new(&provider_config(port)).unwrap();
        let error = provider.generate(chat_request()).await.unwrap_err();
        match error {
            ModelError::Upstream {
                status, retryable, ..
            } => {
                assert_eq!(status, 500);
                assert!(retryable);
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn sse_decoder_handles_split_events() {
        let mut decoder = SseDecoder::default();
        let first = decoder.push(b"data: {\"a\":");
        assert!(first.is_empty());
        let second = decoder.push(b"1}\n\n");
        assert_eq!(second, vec!["{\"a\":1}".to_string()]);
    }

    #[test]
    fn sse_decoder_joins_multiline_data() {
        let mut decoder = SseDecoder::default();
        let events = decoder.push(b"data: line1\ndata: line2\n\n");
        assert_eq!(events, vec!["line1\nline2".to_string()]);
    }

    #[test]
    fn stream_payload_parses_usage_only_chunk() {
        let parsed = parse_stream_payload(
            r#"{"choices":[],"usage":{"prompt_tokens":3,"completion_tokens":4,"total_tokens":7}}"#,
        )
        .unwrap();
        assert_eq!(parsed.usage.unwrap().total_tokens, 7);
    }

    #[test]
    fn body_serializes_stream_options() {
        let request = ChatRequest {
            model: "m".to_string(),
            messages: vec![ChatMessage::user("hi")],
            max_tokens: 10,
            temperature: 0.2,
            stream: true,
        };
        let body = ChatBody::new(&request, true);
        let value = serde_json::to_value(&body).unwrap();
        assert_eq!(value["stream"], true);
        assert_eq!(value["stream_options"]["include_usage"], true);
    }
}
