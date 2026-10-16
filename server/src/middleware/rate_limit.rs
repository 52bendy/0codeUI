//! 基础限流：进程内令牌桶（按凭据 / 客户端 IP 维度）。
//!
//! 说明：M0 阶段采用单机内存实现，无额外组件依赖；多实例部署时可在网关层
//! （Nginx / Cloudflare 等）叠加全局限流，或后续替换为共享存储实现。

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Request, State};
use axum::http::header::AUTHORIZATION;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::error::AppError;
use crate::state::AppState;

/// 单桶最大数量（防止内存无限增长）。
const MAX_BUCKETS: usize = 100_000;
/// 空桶回收阈值：容量紧张时，超过该时长未访问的桶会被清理。
const STALE_BUCKET_AGE: Duration = Duration::from_secs(600);

/// 令牌桶。
#[derive(Debug)]
struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(capacity: f64, now: Instant) -> Self {
        Self {
            tokens: capacity,
            last_refill: now,
        }
    }

    /// 尝试取走一个令牌；返回是否放行。
    fn try_acquire(&mut self, now: Instant, rate_per_sec: f64, capacity: f64) -> bool {
        let elapsed = now
            .saturating_duration_since(self.last_refill)
            .as_secs_f64();
        self.tokens = (self.tokens + elapsed * rate_per_sec).min(capacity);
        self.last_refill = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// 进程内限流器（按 key 维护令牌桶）。
#[derive(Debug)]
pub struct RateLimiter {
    enabled: bool,
    rate_per_sec: f64,
    burst: u32,
    buckets: Mutex<HashMap<String, TokenBucket>>,
}

impl RateLimiter {
    /// 构造限流器：`rate_per_sec` 每秒补充令牌数，`burst` 桶容量。
    pub fn new(rate_per_sec: f64, burst: u32, enabled: bool) -> Self {
        Self {
            enabled,
            rate_per_sec: rate_per_sec.max(f64::MIN_POSITIVE),
            burst: burst.max(1),
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// 是否启用。
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// 判断 key 是否放行（线程安全）。
    pub fn check(&self, key: &str) -> bool {
        self.check_at(key, Instant::now())
    }

    fn check_at(&self, key: &str, now: Instant) -> bool {
        if !self.enabled {
            return true;
        }

        let mut buckets = match self.buckets.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        if buckets.len() >= MAX_BUCKETS && !buckets.contains_key(key) {
            buckets.retain(|_, bucket| {
                now.saturating_duration_since(bucket.last_refill) < STALE_BUCKET_AGE
            });
            if buckets.len() >= MAX_BUCKETS {
                // 仍然满：淘汰最久未访问的桶，保证内存有界。
                if let Some(oldest_key) = buckets
                    .iter()
                    .min_by_key(|(_, bucket)| bucket.last_refill)
                    .map(|(key, _)| key.clone())
                {
                    buckets.remove(&oldest_key);
                }
            }
        }

        let capacity = self.burst as f64;
        let bucket = buckets
            .entry(key.to_string())
            .or_insert_with(|| TokenBucket::new(capacity, now));
        bucket.try_acquire(now, self.rate_per_sec, capacity)
    }

    /// 建议的重试等待秒数（用于 429 的 `Retry-After`）。
    pub fn retry_after_secs(&self) -> u64 {
        (1.0 / self.rate_per_sec).ceil().max(1.0) as u64
    }
}

/// 限流中间件：健康检查路径除外，按「凭据 → X-Forwarded-For → 连接 IP」维度限流。
pub async fn limit_requests(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if !state.rate_limiter.enabled() || is_probe_path(req.uri().path()) {
        return next.run(req).await;
    }

    let key = resolve_key(&req);
    if state.rate_limiter.check(&key) {
        next.run(req).await
    } else {
        AppError::rate_limited(state.rate_limiter.retry_after_secs()).into_response()
    }
}

fn is_probe_path(path: &str) -> bool {
    matches!(path, "/healthz" | "/readyz")
}

fn resolve_key(req: &Request) -> String {
    if let Some(value) = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    {
        if !value.is_empty() {
            return format!("auth:{value}");
        }
    }
    if let Some(forwarded) = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
    {
        if let Some(first) = forwarded.split(',').map(str::trim).find(|s| !s.is_empty()) {
            return format!("ip:{first}");
        }
    }
    if let Some(connect_info) = req.extensions().get::<ConnectInfo<SocketAddr>>() {
        // 仅取 IP（不含源端口），避免同一客户端每次新连接各占一个桶。
        return format!("ip:{}", connect_info.0.ip());
    }
    "anonymous".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_allows_burst_then_limits() {
        let limiter = RateLimiter::new(0.001, 2, true);
        let now = Instant::now();
        assert!(limiter.check_at("k", now));
        assert!(limiter.check_at("k", now));
        assert!(!limiter.check_at("k", now));
    }

    #[test]
    fn bucket_refills_over_time() {
        let limiter = RateLimiter::new(1.0, 1, true);
        let now = Instant::now();
        assert!(limiter.check_at("k", now));
        assert!(!limiter.check_at("k", now));
        assert!(limiter.check_at("k", now + Duration::from_secs(1)));
    }

    #[test]
    fn disabled_limiter_allows_everything() {
        let limiter = RateLimiter::new(0.001, 1, false);
        let now = Instant::now();
        for _ in 0..10 {
            assert!(limiter.check_at("k", now));
        }
    }

    #[test]
    fn keys_are_isolated() {
        let limiter = RateLimiter::new(0.001, 1, true);
        let now = Instant::now();
        assert!(limiter.check_at("a", now));
        assert!(!limiter.check_at("a", now));
        assert!(limiter.check_at("b", now));
    }

    #[test]
    fn connect_info_key_uses_ip_without_port() {
        use axum::body::Body;

        let mut req = Request::builder().uri("/").body(Body::empty()).unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 54321))));
        assert_eq!(resolve_key(&req), "ip:127.0.0.1");
    }
}
