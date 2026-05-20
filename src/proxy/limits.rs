use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use http::{header, HeaderMap};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};

use crate::models::{LimitPolicy, RateLimitKey};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct LimitContext {
    pub listen: String,
    pub rule: String,
    pub client_ip: String,
    pub host: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LimitRejection {
    RateLimited,
    BodyTooLarge,
    QueueTimeout,
}

#[derive(Debug, Default)]
pub struct LimitState {
    buckets: Mutex<HashMap<LimitKey, BucketState>>,
    semaphores: Mutex<HashMap<String, Arc<Semaphore>>>,
}

#[derive(Debug)]
pub struct LimitPermit {
    _connection: Option<OwnedSemaphorePermit>,
}

impl LimitState {
    pub async fn check(
        &self,
        ctx: &LimitContext,
        policy: &LimitPolicy,
        headers: &HeaderMap,
    ) -> Result<LimitPermit, LimitRejection> {
        if let Some(max_body_bytes) = policy.max_body_bytes {
            if content_length(headers).is_some_and(|length| length > max_body_bytes) {
                return Err(LimitRejection::BodyTooLarge);
            }
        }

        if let Some(rate) = policy.rate_per_second {
            if rate > 0 && !self.allow_rate(ctx, policy.rate_key, rate).await {
                return Err(LimitRejection::RateLimited);
            }
        }

        let connection = if let Some(max_connections) = policy.max_connections {
            if max_connections == 0 {
                return Err(LimitRejection::QueueTimeout);
            }
            let semaphore = self
                .semaphore_for(format!("{}:{}", ctx.listen, ctx.rule), max_connections)
                .await;
            Some(acquire_permit(semaphore, policy.queue_timeout_ms).await?)
        } else {
            None
        };

        Ok(LimitPermit {
            _connection: connection,
        })
    }

    async fn allow_rate(&self, ctx: &LimitContext, rate_key: RateLimitKey, rate: u32) -> bool {
        let key = LimitKey::new(ctx, rate_key);
        let mut buckets = self.buckets.lock().await;
        let bucket = buckets.entry(key).or_insert_with(BucketState::new);
        bucket.allow(rate)
    }

    async fn semaphore_for(&self, key: String, permits: u32) -> Arc<Semaphore> {
        let mut semaphores = self.semaphores.lock().await;
        semaphores
            .entry(key)
            .or_insert_with(|| Arc::new(Semaphore::new(permits as usize)))
            .clone()
    }
}

async fn acquire_permit(
    semaphore: Arc<Semaphore>,
    queue_timeout_ms: Option<u64>,
) -> Result<OwnedSemaphorePermit, LimitRejection> {
    let acquire = semaphore.acquire_owned();
    match queue_timeout_ms {
        Some(timeout_ms) => tokio::time::timeout(Duration::from_millis(timeout_ms), acquire)
            .await
            .map_err(|_| LimitRejection::QueueTimeout)?
            .map_err(|_| LimitRejection::QueueTimeout),
        None => acquire.await.map_err(|_| LimitRejection::QueueTimeout),
    }
}

fn content_length(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct LimitKey {
    listen: String,
    rule: String,
    value: String,
}

impl LimitKey {
    fn new(ctx: &LimitContext, rate_key: RateLimitKey) -> Self {
        let value = match rate_key {
            RateLimitKey::Ip => ctx.client_ip.clone(),
            RateLimitKey::Host => ctx.host.clone(),
            RateLimitKey::Route => "route".to_string(),
        };
        Self {
            listen: ctx.listen.clone(),
            rule: ctx.rule.clone(),
            value,
        }
    }
}

#[derive(Debug)]
struct BucketState {
    window_started: Instant,
    used: u32,
}

impl BucketState {
    fn new() -> Self {
        Self {
            window_started: Instant::now(),
            used: 0,
        }
    }

    fn allow(&mut self, rate: u32) -> bool {
        if self.window_started.elapsed() >= Duration::from_secs(1) {
            self.window_started = Instant::now();
            self.used = 0;
        }
        if self.used >= rate {
            return false;
        }
        self.used += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{LimitPolicy, RateLimitKey};
    use http::HeaderMap;
    use std::time::Duration;

    fn ctx() -> LimitContext {
        LimitContext {
            listen: "0.0.0.0:80".to_string(),
            rule: "api".to_string(),
            client_ip: "203.0.113.7".to_string(),
            host: "example.com".to_string(),
        }
    }

    #[tokio::test]
    async fn per_ip_token_bucket_rejects_above_rate() {
        let limits = LimitState::default();
        let policy = LimitPolicy {
            rate_per_second: Some(1),
            rate_key: RateLimitKey::Ip,
            ..Default::default()
        };

        let first = limits.check(&ctx(), &policy, &HeaderMap::new()).await;
        let second = limits.check(&ctx(), &policy, &HeaderMap::new()).await;

        assert!(first.is_ok());
        assert_eq!(second.unwrap_err(), LimitRejection::RateLimited);
    }

    #[tokio::test]
    async fn max_connections_queues_until_permit_is_released() {
        let limits = LimitState::default();
        let policy = LimitPolicy {
            max_connections: Some(1),
            queue_timeout_ms: Some(100),
            ..Default::default()
        };

        let first = limits
            .check(&ctx(), &policy, &HeaderMap::new())
            .await
            .unwrap();
        let second_ctx = ctx();
        let second_headers = HeaderMap::new();
        let pending = limits.check(&second_ctx, &policy, &second_headers);
        tokio::pin!(pending);

        tokio::select! {
            _ = &mut pending => panic!("second request should wait for permit"),
            _ = tokio::time::sleep(Duration::from_millis(10)) => {}
        }

        drop(first);
        assert!(pending.await.is_ok());
    }

    #[tokio::test]
    async fn queue_timeout_returns_unavailable_when_no_permit_arrives() {
        let limits = LimitState::default();
        let policy = LimitPolicy {
            max_connections: Some(1),
            queue_timeout_ms: Some(1),
            ..Default::default()
        };

        let _first = limits
            .check(&ctx(), &policy, &HeaderMap::new())
            .await
            .unwrap();
        let second = limits.check(&ctx(), &policy, &HeaderMap::new()).await;

        assert_eq!(second.unwrap_err(), LimitRejection::QueueTimeout);
    }

    #[tokio::test]
    async fn rule_maxconn_zero_queue_rejects_second_request() {
        let limits = LimitState::default();
        let policy = LimitPolicy {
            max_connections: Some(1),
            queue_timeout_ms: Some(0),
            ..Default::default()
        };

        let first = limits
            .check(&ctx(), &policy, &HeaderMap::new())
            .await
            .unwrap();
        let second = limits.check(&ctx(), &policy, &HeaderMap::new()).await;

        assert!(matches!(second, Err(LimitRejection::QueueTimeout)));
        drop(first);
    }

    #[tokio::test]
    async fn max_body_bytes_rejects_large_content_length() {
        let limits = LimitState::default();
        let policy = LimitPolicy {
            max_body_bytes: Some(100),
            ..Default::default()
        };
        let mut headers = HeaderMap::new();
        headers.insert(http::header::CONTENT_LENGTH, "101".parse().unwrap());

        let result = limits.check(&ctx(), &policy, &headers).await;

        assert_eq!(result.unwrap_err(), LimitRejection::BodyTooLarge);
    }
}
