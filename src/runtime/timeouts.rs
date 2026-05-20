use crate::models::{RuleTimeoutPolicy, TargetTimeoutPolicy, UpstreamTimeoutPolicy};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeoutPolicy {
    #[serde(default = "default_connect_timeout_seconds")]
    pub connect_timeout_seconds: u64,
    #[serde(default = "default_client_timeout_seconds")]
    pub client_timeout_seconds: u64,
    #[serde(default = "default_server_timeout_seconds")]
    pub server_timeout_seconds: u64,
    #[serde(default = "default_http_request_timeout_seconds")]
    pub http_request_timeout_seconds: u64,
    #[serde(default = "default_http_keepalive_timeout_seconds")]
    pub http_keepalive_timeout_seconds: u64,
    #[serde(default = "default_tunnel_timeout_seconds")]
    pub tunnel_timeout_seconds: u64,
    #[serde(default = "default_queue_timeout_ms")]
    pub queue_timeout_ms: u64,
}

impl Default for TimeoutPolicy {
    fn default() -> Self {
        Self {
            connect_timeout_seconds: default_connect_timeout_seconds(),
            client_timeout_seconds: default_client_timeout_seconds(),
            server_timeout_seconds: default_server_timeout_seconds(),
            http_request_timeout_seconds: default_http_request_timeout_seconds(),
            http_keepalive_timeout_seconds: default_http_keepalive_timeout_seconds(),
            tunnel_timeout_seconds: default_tunnel_timeout_seconds(),
            queue_timeout_ms: default_queue_timeout_ms(),
        }
    }
}

impl TimeoutPolicy {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionLimitPolicy {
    #[serde(default)]
    pub global_maxconn: Option<u32>,
    #[serde(default)]
    pub listener_maxconn: Option<u32>,
}

impl Default for ConnectionLimitPolicy {
    fn default() -> Self {
        Self {
            global_maxconn: None,
            listener_maxconn: None,
        }
    }
}

impl ConnectionLimitPolicy {
    pub fn is_default(&self) -> bool {
        self.global_maxconn.is_none() && self.listener_maxconn.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTimeoutPolicy {
    pub connect_timeout: Duration,
    pub client_timeout: Duration,
    pub server_timeout: Duration,
    pub http_request_timeout: Duration,
    pub http_keepalive_timeout: Duration,
    pub tunnel_timeout: Duration,
    pub queue_timeout: Duration,
}

impl ResolvedTimeoutPolicy {
    pub fn resolve(
        global: &TimeoutPolicy,
        rule: Option<&RuleTimeoutPolicy>,
        upstream: Option<&UpstreamTimeoutPolicy>,
        target: Option<&TargetTimeoutPolicy>,
    ) -> Self {
        Self {
            connect_timeout: Duration::from_secs(resolve_seconds(
                target.and_then(|policy| policy.connect_timeout_seconds),
                upstream.and_then(|policy| policy.connect_timeout_seconds),
                rule.and_then(|policy| policy.connect_timeout_seconds),
                global.connect_timeout_seconds,
            )),
            client_timeout: Duration::from_secs(resolve_seconds(
                target.and_then(|policy| policy.client_timeout_seconds),
                upstream.and_then(|policy| policy.client_timeout_seconds),
                rule.and_then(|policy| policy.client_timeout_seconds),
                global.client_timeout_seconds,
            )),
            server_timeout: Duration::from_secs(resolve_seconds(
                target.and_then(|policy| policy.server_timeout_seconds),
                upstream.and_then(|policy| policy.server_timeout_seconds),
                rule.and_then(|policy| policy.server_timeout_seconds),
                global.server_timeout_seconds,
            )),
            http_request_timeout: Duration::from_secs(resolve_seconds(
                target.and_then(|policy| policy.http_request_timeout_seconds),
                upstream.and_then(|policy| policy.http_request_timeout_seconds),
                rule.and_then(|policy| policy.http_request_timeout_seconds),
                global.http_request_timeout_seconds,
            )),
            http_keepalive_timeout: Duration::from_secs(resolve_seconds(
                target.and_then(|policy| policy.http_keepalive_timeout_seconds),
                upstream.and_then(|policy| policy.http_keepalive_timeout_seconds),
                rule.and_then(|policy| policy.http_keepalive_timeout_seconds),
                global.http_keepalive_timeout_seconds,
            )),
            tunnel_timeout: Duration::from_secs(resolve_seconds(
                target.and_then(|policy| policy.tunnel_timeout_seconds),
                upstream.and_then(|policy| policy.tunnel_timeout_seconds),
                rule.and_then(|policy| policy.tunnel_timeout_seconds),
                global.tunnel_timeout_seconds,
            )),
            queue_timeout: Duration::from_millis(resolve_u64(
                target.and_then(|policy| policy.queue_timeout_ms),
                upstream.and_then(|policy| policy.queue_timeout_ms),
                rule.and_then(|policy| policy.queue_timeout_ms),
                global.queue_timeout_ms,
            )),
        }
    }
}

fn resolve_seconds(
    target: Option<u64>,
    upstream: Option<u64>,
    rule: Option<u64>,
    global: u64,
) -> u64 {
    target.or(upstream).or(rule).unwrap_or(global)
}

fn resolve_u64(target: Option<u64>, upstream: Option<u64>, rule: Option<u64>, global: u64) -> u64 {
    target.or(upstream).or(rule).unwrap_or(global)
}

fn default_connect_timeout_seconds() -> u64 {
    10
}

fn default_client_timeout_seconds() -> u64 {
    60
}

fn default_server_timeout_seconds() -> u64 {
    60
}

fn default_http_request_timeout_seconds() -> u64 {
    60
}

fn default_http_keepalive_timeout_seconds() -> u64 {
    90
}

fn default_tunnel_timeout_seconds() -> u64 {
    60
}

fn default_queue_timeout_ms() -> u64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::RuleTimeoutPolicy;
    use std::time::Duration;

    #[test]
    fn rule_timeout_overrides_global_timeout() {
        let global = TimeoutPolicy {
            server_timeout_seconds: 60,
            ..Default::default()
        };
        let rule = RuleTimeoutPolicy {
            server_timeout_seconds: Some(7),
            ..Default::default()
        };

        let resolved = ResolvedTimeoutPolicy::resolve(&global, Some(&rule), None, None);

        assert_eq!(resolved.server_timeout, Duration::from_secs(7));
    }
}
