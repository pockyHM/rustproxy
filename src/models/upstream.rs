use serde::{Deserialize, Serialize};

use crate::stick::StickyPolicy;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_target_serde() {
        let target = Target {
            url: "http://localhost:8080".to_string(),
            weight: 100,
        };
        let json = serde_json::to_string(&target).unwrap();
        let parsed: Target = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.url, "http://localhost:8080");
        assert_eq!(parsed.weight, 100);
    }

    #[test]
    fn test_target_clone_and_partial_eq() {
        let target1 = Target {
            url: "http://localhost:8080".to_string(),
            weight: 50,
        };
        let target2 = target1.clone();
        assert_eq!(target1, target2);
    }

    #[test]
    fn test_upstream_serde() {
        let upstream = Upstream {
            name: "backend".to_string(),
            skip_ssl: false,
            websocket: false,
            targets: vec![
                Target {
                    url: "http://localhost:8080".to_string(),
                    weight: 80,
                },
                Target {
                    url: "http://localhost:8081".to_string(),
                    weight: 20,
                },
            ],
            health_check: Default::default(),
            balance: Default::default(),
            retry: Default::default(),
            sticky: Default::default(),
        };
        let json = serde_json::to_string(&upstream).unwrap();
        let parsed: Upstream = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "backend");
        assert_eq!(parsed.targets.len(), 2);
        assert_eq!(parsed.targets[0].url, "http://localhost:8080");
        assert_eq!(parsed.targets[0].weight, 80);
        assert_eq!(parsed.targets[1].url, "http://localhost:8081");
        assert_eq!(parsed.targets[1].weight, 20);
    }

    #[test]
    fn test_upstream_single_target() {
        let upstream = Upstream {
            name: "single".to_string(),
            skip_ssl: false,
            websocket: false,
            targets: vec![Target {
                url: "http://localhost:9090".to_string(),
                weight: 100,
            }],
            health_check: Default::default(),
            balance: Default::default(),
            retry: Default::default(),
            sticky: Default::default(),
        };
        assert_eq!(upstream.targets.len(), 1);
        assert_eq!(upstream.targets[0].url, "http://localhost:9090");
    }

    #[test]
    fn test_upstream_clone() {
        let upstream = Upstream {
            name: "clone-test".to_string(),
            skip_ssl: false,
            websocket: false,
            targets: vec![
                Target {
                    url: "http://localhost:8080".to_string(),
                    weight: 50,
                },
                Target {
                    url: "http://localhost:8081".to_string(),
                    weight: 50,
                },
            ],
            health_check: Default::default(),
            balance: Default::default(),
            retry: Default::default(),
            sticky: Default::default(),
        };
        let cloned = upstream.clone();
        assert_eq!(upstream, cloned);
        assert_eq!(cloned.name, "clone-test");
        assert_eq!(cloned.targets.len(), 2);
    }

    #[test]
    fn test_target_debug() {
        let target = Target {
            url: "http://debug:9999".to_string(),
            weight: 42,
        };
        let debug_str = format!("{:?}", target);
        assert!(debug_str.contains("http://debug:9999"));
        assert!(debug_str.contains("42"));
    }

    #[test]
    fn test_upstream_debug() {
        let upstream = Upstream {
            name: "debug-upstream".to_string(),
            skip_ssl: false,
            websocket: false,
            targets: vec![],
            health_check: Default::default(),
            balance: Default::default(),
            retry: Default::default(),
            sticky: Default::default(),
        };
        let debug_str = format!("{:?}", upstream);
        assert!(debug_str.contains("debug-upstream"));
    }

    #[test]
    fn test_upstream_health_check_serde() {
        let yaml = r#"
name: backend
targets:
  - url: http://localhost:8080
    weight: 100
health_check:
  enabled: true
  mode: http
  path: /ready
  expected_status: 204
"#;
        let upstream: Upstream = serde_yaml::from_str(yaml).unwrap();

        assert!(upstream.health_check.enabled);
        assert_eq!(upstream.health_check.mode, HealthCheckMode::Http);
        assert_eq!(upstream.health_check.path, "/ready");
        assert_eq!(upstream.health_check.expected_status, 204);
    }

    #[test]
    fn test_upstream_health_check_defaults_to_disabled() {
        let yaml = r#"
name: backend
targets:
  - url: http://localhost:8080
    weight: 100
"#;
        let upstream: Upstream = serde_yaml::from_str(yaml).unwrap();

        assert!(!upstream.health_check.enabled);
        assert_eq!(upstream.health_check.mode, HealthCheckMode::Tcp);
    }

    #[test]
    fn test_upstream_policy_defaults() {
        let yaml = r#"
name: api
targets:
  - url: http://127.0.0.1:8080
    weight: 100
"#;
        let upstream: Upstream = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(upstream.balance, BalanceAlgorithm::WeightedRoundRobin);
        assert_eq!(upstream.retry.attempts, 0);
        assert_eq!(upstream.sticky, StickyPolicy::default());
    }

    #[test]
    fn test_upstream_sticky_policy_serde() {
        let yaml = r#"
name: api
sticky:
  enabled: true
  source:
    type: header
    name: x-session
  ttl_seconds: 120
targets:
  - url: http://127.0.0.1:8080
    weight: 100
"#;

        let upstream: Upstream = serde_yaml::from_str(yaml).unwrap();

        assert!(upstream.sticky.enabled);
        assert_eq!(upstream.sticky.ttl_seconds, 120);
        assert_eq!(
            upstream.sticky.source,
            crate::stick::StickyKeySource::Header {
                name: "x-session".to_string()
            }
        );

        let serialized = serde_json::to_string(&upstream).unwrap();
        assert!(serialized.contains("\"sticky\""));
        assert!(serialized.contains("\"ttl_seconds\":120"));
    }

    #[test]
    fn test_upstream_and_target_timeout_policy_is_ignored() {
        let yaml = r#"
name: api
timeouts:
  connect_timeout_seconds: 3
targets:
  - url: http://127.0.0.1:8080
    weight: 100
    timeouts:
      server_timeout_seconds: 9
"#;

        let upstream: Upstream = serde_yaml::from_str(yaml).unwrap();

        let serialized = serde_json::to_string(&upstream).unwrap();
        assert!(!serialized.contains("\"timeouts\""));
        assert_eq!(upstream.targets[0].url, "http://127.0.0.1:8080");
    }

    #[test]
    fn validate_target_protocols_allows_http_and_https_targets() {
        let upstream = Upstream {
            name: "web".to_string(),
            skip_ssl: false,
            websocket: false,
            targets: vec![
                Target {
                    url: "http://127.0.0.1:8080".to_string(),
                    weight: 100,
                },
                Target {
                    url: "https://127.0.0.1:8443".to_string(),
                    weight: 100,
                },
            ],
            health_check: Default::default(),
            balance: Default::default(),
            retry: Default::default(),
            sticky: Default::default(),
        };

        upstream.validate_target_protocols().unwrap();
    }

    #[test]
    fn validate_target_protocols_allows_tcp_and_socket_targets() {
        let upstream = Upstream {
            name: "redis".to_string(),
            skip_ssl: false,
            websocket: false,
            targets: vec![
                Target {
                    url: "tcp://127.0.0.1:6379".to_string(),
                    weight: 100,
                },
                Target {
                    url: "127.0.0.1:6380".to_string(),
                    weight: 100,
                },
            ],
            health_check: Default::default(),
            balance: Default::default(),
            retry: Default::default(),
            sticky: Default::default(),
        };

        upstream.validate_target_protocols().unwrap();
    }

    #[test]
    fn validate_target_protocols_rejects_http_and_tcp_mix() {
        let upstream = Upstream {
            name: "mixed".to_string(),
            skip_ssl: false,
            websocket: false,
            targets: vec![
                Target {
                    url: "http://127.0.0.1:8080".to_string(),
                    weight: 100,
                },
                Target {
                    url: "tcp://127.0.0.1:6379".to_string(),
                    weight: 100,
                },
            ],
            health_check: Default::default(),
            balance: Default::default(),
            retry: Default::default(),
            sticky: Default::default(),
        };

        let error = upstream
            .validate_target_protocols()
            .unwrap_err()
            .to_string();
        assert!(error.contains("cannot mix HTTP and TCP targets"));
    }

    #[test]
    fn validate_target_protocols_rejects_unsupported_scheme() {
        let upstream = Upstream {
            name: "bad".to_string(),
            skip_ssl: false,
            websocket: false,
            targets: vec![Target {
                url: "redis://127.0.0.1:6379".to_string(),
                weight: 100,
            }],
            health_check: Default::default(),
            balance: Default::default(),
            retry: Default::default(),
            sticky: Default::default(),
        };

        let error = upstream
            .validate_target_protocols()
            .unwrap_err()
            .to_string();
        assert!(error.contains("unsupported target scheme"));
    }

    #[test]
    fn validate_target_protocols_rejects_tcp_target_without_port() {
        let upstream = Upstream {
            name: "bad".to_string(),
            skip_ssl: false,
            websocket: false,
            targets: vec![Target {
                url: "tcp://redis.internal".to_string(),
                weight: 100,
            }],
            health_check: Default::default(),
            balance: Default::default(),
            retry: Default::default(),
            sticky: Default::default(),
        };

        let error = upstream
            .validate_target_protocols()
            .unwrap_err()
            .to_string();
        assert!(error.contains("requires host and port"));
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Target {
    pub url: String,
    pub weight: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BalanceAlgorithm {
    #[default]
    WeightedRoundRobin,
    LeastConnections,
    IpHash,
    ConsistentHash,
    UrlHash,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RetryPolicy {
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub retry_on_status: Vec<u16>,
    #[serde(default)]
    pub retry_on_timeout: bool,
    #[serde(default)]
    pub retry_on_connect_error: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthCheckMode {
    Tcp,
    Http,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HealthCheck {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_health_check_mode")]
    pub mode: HealthCheckMode,
    #[serde(default = "default_health_check_path")]
    pub path: String,
    #[serde(default = "default_expected_status")]
    pub expected_status: u16,
    #[serde(default = "default_interval_seconds")]
    pub interval_seconds: u64,
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: u64,
    #[serde(default = "default_threshold")]
    pub healthy_threshold: u32,
    #[serde(default = "default_threshold")]
    pub unhealthy_threshold: u32,
}

impl Default for HealthCheck {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: default_health_check_mode(),
            path: default_health_check_path(),
            expected_status: default_expected_status(),
            interval_seconds: default_interval_seconds(),
            timeout_seconds: default_timeout_seconds(),
            healthy_threshold: default_threshold(),
            unhealthy_threshold: default_threshold(),
        }
    }
}

fn default_health_check_mode() -> HealthCheckMode {
    HealthCheckMode::Tcp
}

fn default_health_check_path() -> String {
    "/health".to_string()
}

fn default_expected_status() -> u16 {
    200
}

fn default_interval_seconds() -> u64 {
    10
}

fn default_timeout_seconds() -> u64 {
    2
}

fn default_threshold() -> u32 {
    2
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Upstream {
    pub name: String,
    #[serde(default)]
    pub skip_ssl: bool,
    #[serde(default)]
    pub websocket: bool,
    pub targets: Vec<Target>,
    #[serde(default)]
    pub health_check: HealthCheck,
    #[serde(default)]
    pub balance: BalanceAlgorithm,
    #[serde(default)]
    pub retry: RetryPolicy,
    #[serde(default)]
    pub sticky: StickyPolicy,
}

impl Upstream {
    pub fn validate_target_protocols(&self) -> anyhow::Result<()> {
        let mut protocol: Option<TargetProtocol> = None;
        for target in &self.targets {
            let current = TargetProtocol::from_url(&target.url).map_err(|error| {
                anyhow::anyhow!(
                    "upstream '{}' has invalid target '{}': {error}",
                    self.name,
                    target.url
                )
            })?;
            if let Some(previous) = protocol {
                if previous != current {
                    anyhow::bail!(
                        "upstream '{}' cannot mix HTTP and TCP targets; target '{}' is {} but previous targets are {}",
                        self.name,
                        target.url,
                        current.label(),
                        previous.label()
                    );
                }
            } else {
                protocol = Some(current);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetProtocol {
    Http,
    Tcp,
}

impl TargetProtocol {
    fn from_url(url: &str) -> anyhow::Result<Self> {
        let trimmed = url.trim();
        if trimmed.is_empty() {
            anyhow::bail!("target URL cannot be empty");
        }
        if trimmed.parse::<std::net::SocketAddr>().is_ok() {
            return Ok(TargetProtocol::Tcp);
        }

        let uri: http::Uri = trimmed
            .parse()
            .map_err(|error| anyhow::anyhow!("invalid target URL: {error}"))?;
        match uri.scheme_str() {
            Some("http") | Some("https") => {
                if uri.authority().is_none() {
                    anyhow::bail!("HTTP target requires host");
                }
                Ok(TargetProtocol::Http)
            }
            Some("tcp") => {
                if uri.host().is_none() || uri.port_u16().is_none() {
                    anyhow::bail!("TCP target requires host and port");
                }
                Ok(TargetProtocol::Tcp)
            }
            Some(scheme) => anyhow::bail!(
                "unsupported target scheme '{scheme}'; use http://, https://, tcp://, or host:port"
            ),
            None => anyhow::bail!("target URL must use http://, https://, tcp://, or host:port"),
        }
    }

    fn label(self) -> &'static str {
        match self {
            TargetProtocol::Http => "HTTP",
            TargetProtocol::Tcp => "TCP",
        }
    }
}
