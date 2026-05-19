use serde::{Deserialize, Serialize};

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
}
