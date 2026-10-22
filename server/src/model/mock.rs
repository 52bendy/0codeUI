//! 确定性 mock 提供者：无 GPU / 无外网时用于联调、CI 与压测基线。
//!
//! 输出是一份结构完整、可被单文件校验器接受的 HTML，内容由 prompt 决定，
//! 因此同一 prompt 的响应稳定可复现。

use std::time::Duration;

use async_trait::async_trait;
use futures_util::stream;

use super::error::ModelError;
use super::provider::{ModelProvider, ModelStream};
use super::types::{ChatRequest, ChatResponse, StreamChunk, Usage};

/// 确定性 mock 推理提供者。
#[derive(Debug, Clone)]
pub struct MockProvider {
    latency_ms: u64,
    stream_delay_ms: u64,
}

impl MockProvider {
    /// 构造 mock 提供者。
    pub fn new(latency_ms: u64, stream_delay_ms: u64) -> Self {
        Self {
            latency_ms,
            stream_delay_ms,
        }
    }

    fn render(&self, request: &ChatRequest) -> String {
        let prompt = request
            .messages
            .iter()
            .rev()
            .find(|message| matches!(message.role, super::types::Role::User))
            .map(|message| message.content.as_str())
            .unwrap_or("UI 页面");
        let safe_prompt = escape_html(&truncate_chars(prompt, 160));
        let title = format!("InTimeUI · {}", truncate_chars(prompt, 24));
        format!(
            r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src data:; base-uri 'none'; form-action 'none'">
<title>{title}</title>
<style>
:root{{--bg:#f7f7f5;--fg:#111;--muted:#666;--line:#d9d9d4;--card:#fff}}
[data-theme="dark"]{{--bg:#111;--fg:#f4f4f0;--muted:#aaa;--line:#333;--card:#1a1a1a}}
*{{box-sizing:border-box}}
body{{margin:0;background:var(--bg);color:var(--fg);font:16px/1.6 ui-sans-serif,system-ui,-apple-system,"Segoe UI",sans-serif;transition:background .2s,color .2s}}
header{{display:flex;align-items:center;justify-content:space-between;padding:20px 24px;border-bottom:1px solid var(--line)}}
h1{{margin:0;font-size:clamp(20px,4vw,32px);letter-spacing:-.02em}}
button{{border:1px solid var(--line);background:var(--card);color:var(--fg);border-radius:999px;padding:8px 14px;cursor:pointer}}
button:hover{{transform:translateY(-1px)}}
button:focus-visible{{outline:2px solid var(--fg);outline-offset:2px}}
main{{max-width:720px;margin:0 auto;padding:48px 24px}}
.card{{background:var(--card);border:1px solid var(--line);border-radius:16px;padding:24px;box-shadow:0 18px 40px rgba(0,0,0,.06)}}
.label{{color:var(--muted);font-size:13px;text-transform:uppercase;letter-spacing:.12em}}
.prompt{{font-size:18px;margin:12px 0 0}}
@media (prefers-reduced-motion: reduce){{*{{transition:none!important}}}}
</style>
</head>
<body>
<header>
  <span class="label">InTimeUI / model gateway</span>
  <button type="button" id="theme-toggle" aria-label="切换明暗主题">切换主题</button>
</header>
<main>
  <section class="card" aria-labelledby="page-title">
    <p class="label">单文件 HTML · 模型联调输出</p>
    <h1 id="page-title">{title}</h1>
    <p class="prompt">{safe_prompt}</p>
    <p>本页面由 mock 推理提供者生成，用于验证模型接入、流式输出与单文件 HTML 校验链路。</p>
  </section>
</main>
<script>
(function(){{
  var root=document.documentElement;
  var saved=null;
  try{{saved=localStorage.getItem('intimeui-theme');}}catch(e){{}}
  if(saved==='dark'){{root.setAttribute('data-theme','dark');}}
  document.getElementById('theme-toggle').addEventListener('click',function(){{
    var dark=root.getAttribute('data-theme')==='dark';
    if(dark){{root.removeAttribute('data-theme');}}else{{root.setAttribute('data-theme','dark');}}
    try{{localStorage.setItem('intimeui-theme',dark?'light':'dark');}}catch(e){{}}
  }});
}})();
</script>
</body>
</html>"#
        )
    }
}

#[async_trait]
impl ModelProvider for MockProvider {
    fn name(&self) -> &'static str {
        "mock"
    }

    async fn generate(&self, request: ChatRequest) -> Result<ChatResponse, ModelError> {
        if self.latency_ms > 0 {
            tokio::time::sleep(Duration::from_millis(self.latency_ms)).await;
        }
        let content = self.render(&request);
        let usage = estimate_usage(&request, &content);
        Ok(ChatResponse {
            content,
            finish_reason: Some("stop".to_string()),
            usage,
        })
    }

    async fn generate_stream(&self, request: ChatRequest) -> Result<ModelStream, ModelError> {
        let content = self.render(&request);
        let usage = estimate_usage(&request, &content);
        let chunks = split_chunks(&content, 96);
        let total = chunks.len();
        let delay = self.stream_delay_ms;

        let stream = stream::unfold(
            (chunks, 0usize, total, delay, usage),
            |(chunks, index, total, delay, usage)| async move {
                if index >= total {
                    return None;
                }
                if delay > 0 {
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                }
                let is_last = index + 1 == total;
                let chunk = StreamChunk {
                    delta: Some(chunks[index].clone()),
                    finish_reason: is_last.then(|| "stop".to_string()),
                    usage: is_last.then_some(usage),
                };
                Some((Ok(chunk), (chunks, index + 1, total, delay, usage)))
            },
        );
        Ok(Box::pin(stream))
    }
}

fn truncate_chars(value: &str, max: usize) -> String {
    let mut result = String::new();
    for (count, ch) in value.chars().enumerate() {
        if count >= max {
            result.push('…');
            break;
        }
        result.push(ch);
    }
    result
}

fn escape_html(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '"' => result.push_str("&quot;"),
            '\'' => result.push_str("&#39;"),
            other => result.push(other),
        }
    }
    result
}

fn split_chunks(value: &str, size: usize) -> Vec<String> {
    let chars: Vec<char> = value.chars().collect();
    chars
        .chunks(size.max(1))
        .map(|chunk| chunk.iter().collect())
        .collect()
}

fn estimate_usage(request: &ChatRequest, content: &str) -> Usage {
    let prompt_chars: usize = request
        .messages
        .iter()
        .map(|message| message.content.chars().count())
        .sum();
    let prompt_tokens = (prompt_chars / 4).max(1) as u32;
    let completion_tokens = (content.chars().count() / 4).max(1) as u32;
    Usage {
        prompt_tokens,
        completion_tokens,
        total_tokens: prompt_tokens + completion_tokens,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::types::ChatMessage;

    fn request() -> ChatRequest {
        ChatRequest {
            model: "mock".to_string(),
            messages: vec![
                ChatMessage::system("system"),
                ChatMessage::user("<script>alert(1)</script> 订单页"),
            ],
            max_tokens: 128,
            temperature: 0.2,
            stream: false,
        }
    }

    #[tokio::test]
    async fn mock_output_is_single_file_html_and_escapes_prompt() {
        let provider = MockProvider::new(0, 0);
        let response = provider.generate(request()).await.unwrap();
        assert!(response.content.starts_with("<!DOCTYPE html>"));
        assert!(response.content.contains("</html>"));
        assert!(!response.content.contains("<script>alert(1)</script>"));
        assert!(response.content.contains("&lt;script&gt;"));
        assert!(response.usage.total_tokens > 0);
    }

    #[tokio::test]
    async fn mock_stream_emits_deltas_and_usage() {
        let provider = MockProvider::new(0, 0);
        let mut stream = provider.generate_stream(request()).await.unwrap();
        let mut text = String::new();
        let mut usage = None;
        while let Some(chunk) = futures_util::StreamExt::next(&mut stream).await {
            let chunk = chunk.unwrap();
            if let Some(delta) = chunk.delta {
                text.push_str(&delta);
            }
            if let Some(value) = chunk.usage {
                usage = Some(value);
            }
        }
        assert!(text.starts_with("<!DOCTYPE html>"));
        assert!(usage.is_some());
    }
}
