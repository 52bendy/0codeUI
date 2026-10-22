//! 进程内响应缓存。
//!
//! 缓存的是「已通过 HTML 校验、尚未混淆」的生成结果：同一份输入在 TTL 内
//! 直接复用模型输出，跳过推理；每次命中仍重新执行输出混淆，避免相同页面
//! 因缓存产生固定指纹。缓存容量固定，按插入顺序做 FIFO 淘汰，避免无界增长。

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::model::Usage;

/// 一条可缓存的生成结果。
#[derive(Debug, Clone)]
pub struct CachedGeneration {
    /// 原始单文件 HTML（已通过校验）。
    pub html: String,
    /// 生成模型。
    pub model: String,
    /// token 用量。
    pub usage: Usage,
    /// 估算成本（美元）。
    pub estimated_cost_usd: f64,
    /// 结束原因。
    pub finish_reason: Option<String>,
    /// Prompt 稳定指纹。
    pub prompt_hash: String,
    /// 可追溯版本串。
    pub version: String,
}

#[derive(Debug)]
struct CacheEntry {
    value: CachedGeneration,
    expires_at: Instant,
}

#[derive(Debug, Default)]
struct CacheInner {
    entries: HashMap<String, CacheEntry>,
    order: VecDeque<String>,
}

/// 线程安全的 TTL + FIFO 缓存。
#[derive(Debug)]
pub struct ResponseCache {
    ttl: Duration,
    max_entries: usize,
    inner: Mutex<CacheInner>,
}

impl ResponseCache {
    /// 创建缓存。
    pub fn new(ttl_secs: u64, max_entries: usize) -> Self {
        Self {
            ttl: Duration::from_secs(ttl_secs.max(1)),
            max_entries: max_entries.max(1),
            inner: Mutex::new(CacheInner::default()),
        }
    }

    /// 读取缓存；过期条目会被惰性删除。
    pub fn get(&self, key: &str) -> Option<CachedGeneration> {
        let now = Instant::now();
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let expired = inner
            .entries
            .get(key)
            .map(|entry| entry.expires_at <= now)
            .unwrap_or(false);
        if expired {
            inner.entries.remove(key);
            inner.order.retain(|item| item != key);
            return None;
        }
        inner.entries.get(key).map(|entry| entry.value.clone())
    }

    /// 写入缓存；容量满时淘汰最早插入的条目。
    pub fn insert(&self, key: String, value: CachedGeneration) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if inner.entries.contains_key(&key) {
            inner.order.retain(|item| item != &key);
        }
        while inner.entries.len() >= self.max_entries {
            let Some(oldest) = inner.order.pop_front() else {
                break;
            };
            inner.entries.remove(&oldest);
        }
        inner.entries.insert(
            key.clone(),
            CacheEntry {
                value,
                expires_at: Instant::now() + self.ttl,
            },
        );
        inner.order.push_back(key);
    }

    /// 当前条目数（含尚未惰性清理的过期条目）。
    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .entries
            .len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generation(html: &str) -> CachedGeneration {
        CachedGeneration {
            html: html.to_string(),
            model: "mock".to_string(),
            usage: Usage::default(),
            estimated_cost_usd: 0.0,
            finish_reason: Some("stop".to_string()),
            prompt_hash: "abc".to_string(),
            version: "dynamic:SCENE:v1-abc".to_string(),
        }
    }

    #[test]
    fn insert_and_get_round_trip() {
        let cache = ResponseCache::new(60, 2);
        cache.insert("k1".to_string(), generation("a"));
        assert_eq!(cache.get("k1").unwrap().html, "a");
        assert!(cache.get("missing").is_none());
    }

    #[test]
    fn capacity_evicts_oldest() {
        let cache = ResponseCache::new(60, 2);
        cache.insert("k1".to_string(), generation("a"));
        cache.insert("k2".to_string(), generation("b"));
        cache.insert("k3".to_string(), generation("c"));
        assert!(cache.get("k1").is_none());
        assert!(cache.get("k2").is_some());
        assert!(cache.get("k3").is_some());
    }

    #[test]
    fn expired_entry_is_not_returned() {
        let cache = ResponseCache::new(1, 4);
        cache.insert("k1".to_string(), generation("a"));
        std::thread::sleep(Duration::from_millis(1100));
        assert!(cache.get("k1").is_none());
        assert!(cache.is_empty());
    }
}
