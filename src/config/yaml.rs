use crate::models::{HostMatchType, LocationMatchType, MatchSet, Rule, Upstream};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fallback {
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Certificate {
    pub name: String,
    pub cert: String,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TlsListener {
    #[serde(default = "default_tls_listener_enabled")]
    pub enabled: bool,
    pub listen: String,
    pub certificate: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccessLogConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default = "default_access_log_buffer_size")]
    pub buffer_size: Option<usize>,
}

impl Default for AccessLogConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            path: None,
            buffer_size: default_access_log_buffer_size(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    pub version: String,
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default = "default_proxy_listen")]
    pub proxy_listen: String,
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout: u64,
    #[serde(default = "default_request_timeout")]
    pub request_timeout: u64,
    #[serde(default = "default_pool_max_idle_per_host")]
    pub pool_max_idle_per_host: usize,
    #[serde(default = "default_pool_idle_timeout")]
    pub pool_idle_timeout: u64,
    #[serde(default = "default_tcp_keepalive")]
    pub tcp_keepalive: u64,
    #[serde(default = "default_certificate_dir")]
    pub certificate_dir: String,
    #[serde(default)]
    pub access_log: AccessLogConfig,
    #[serde(default)]
    pub certificates: Vec<Certificate>,
    #[serde(default)]
    pub tls_listeners: Vec<TlsListener>,
    #[serde(default)]
    pub match_sets: Vec<MatchSet>,
    pub rules: Vec<Rule>,
    pub upstreams: HashMap<String, Upstream>,
    pub fallback: Fallback,
}

fn default_listen() -> String {
    "0.0.0.0:3000".to_string()
}

fn default_proxy_listen() -> String {
    "0.0.0.0:80".to_string()
}

fn default_connect_timeout() -> u64 {
    10
}

fn default_request_timeout() -> u64 {
    60
}

fn default_pool_max_idle_per_host() -> usize {
    32
}

fn default_pool_idle_timeout() -> u64 {
    90
}

fn default_tcp_keepalive() -> u64 {
    60
}

fn default_certificate_dir() -> String {
    "/etc/rustproxy/cert.d".to_string()
}

fn default_tls_listener_enabled() -> bool {
    true
}

fn default_access_log_buffer_size() -> Option<usize> {
    Some(8192)
}

impl AppConfig {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let mut config: AppConfig = serde_yaml::from_str(&content)?;
        config.normalize_rules();
        Ok(config)
    }

    pub fn normalize_rules(&mut self) {
        if self.proxy_listen.trim().is_empty() {
            self.proxy_listen = default_proxy_listen();
        }
        let default_listen = self.proxy_listen.clone();
        for rule in &mut self.rules {
            Self::normalize_rule_with_default(rule, &default_listen);
        }
    }

    pub fn normalize_rule_with_default(rule: &mut Rule, default_listen: &str) {
        if rule.listen.as_deref().unwrap_or_default().trim().is_empty() {
            rule.listen = Some(default_listen.to_string());
        }
        if matches!(rule.host.match_type, HostMatchType::Any) {
            rule.host.value = None;
        }
        if rule.location.value.trim().is_empty() {
            rule.location.value = "/".to_string();
        }
        if matches!(
            rule.location.match_type,
            LocationMatchType::Exact | LocationMatchType::Prefix
        ) && !rule.location.value.starts_with('/')
        {
            rule.location.value = format!("/{}", rule.location.value);
        }
        if rule.is_fallback {
            rule.priority = 0;
            rule.conditions = None;
            rule.match_set = None;
            rule.weight = 100;
            rule.tls = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_test_config_file(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file.flush().unwrap();
        file
    }

    #[test]
    fn test_app_config_load() {
        let yaml_content = r#"
version: "1.0"
listen: "0.0.0.0:8080"
rules:
  - id: "rule-1"
    name: "Test Rule"
    priority: 10
    conditions:
      - type: header
        key: "Host"
        operator: exact
        value: "example.com"
    upstream: "backend-1"
    weight: 100
upstreams:
  backend-1:
    name: "backend-1"
    targets:
      - url: "http://localhost:8080"
        weight: 100
fallback:
  url: "http://fallback.example.com"
"#;
        let file = create_test_config_file(yaml_content);
        let config = AppConfig::load(file.path().to_str().unwrap()).unwrap();

        assert_eq!(config.version, "1.0");
        assert_eq!(config.listen, "0.0.0.0:8080");
        assert_eq!(config.rules.len(), 1);
        assert_eq!(config.rules[0].id, "rule-1");
        assert_eq!(config.rules[0].upstream, "backend-1");
        assert!(config.upstreams.contains_key("backend-1"));
        assert_eq!(config.fallback.url, "http://fallback.example.com");
    }

    #[test]
    fn test_app_config_load_empty_rules() {
        let yaml_content = r#"
version: "1.0"
listen: "0.0.0.0:8080"
rules: []
upstreams: {}
fallback:
  url: "http://fallback.example.com"
"#;
        let file = create_test_config_file(yaml_content);
        let config = AppConfig::load(file.path().to_str().unwrap()).unwrap();

        assert_eq!(config.version, "1.0");
        assert!(config.rules.is_empty());
        assert!(config.upstreams.is_empty());
    }

    #[test]
    fn test_app_config_load_multiple_rules_and_upstreams() {
        let yaml_content = r#"
version: "2.0"
listen: "0.0.0.0:9090"
rules:
  - id: "rule-1"
    name: "Header Rule"
    priority: 10
    conditions:
      - type: header
        key: "X-Api-Key"
        operator: exists
    upstream: "api-backend"
    weight: 80
  - id: "rule-2"
    name: "JWT Rule"
    priority: 5
    conditions:
      - type: jwt
        claim_path: "roles"
        operator: contains
        value: "admin"
    upstream: "admin-backend"
    weight: 100
upstreams:
  api-backend:
    name: "api-backend"
    targets:
      - url: "http://api1.example.com"
        weight: 70
      - url: "http://api2.example.com"
        weight: 30
  admin-backend:
    name: "admin-backend"
    targets:
      - url: "http://admin.example.com"
        weight: 100
fallback:
  url: "http://default.example.com"
"#;
        let file = create_test_config_file(yaml_content);
        let config = AppConfig::load(file.path().to_str().unwrap()).unwrap();

        assert_eq!(config.version, "2.0");
        assert_eq!(config.listen, "0.0.0.0:9090");
        assert_eq!(config.rules.len(), 2);
        assert_eq!(config.rules[0].id, "rule-1");
        assert_eq!(config.rules[1].id, "rule-2");
        assert_eq!(config.upstreams.len(), 2);
        assert!(config.upstreams.contains_key("api-backend"));
        assert!(config.upstreams.contains_key("admin-backend"));
        assert_eq!(config.upstreams["api-backend"].targets.len(), 2);
    }

    #[test]
    fn test_app_config_load_file_not_found() {
        let result = AppConfig::load("/nonexistent/path/config.yaml");
        assert!(result.is_err());
    }

    #[test]
    fn test_app_config_load_invalid_yaml() {
        let yaml_content = "invalid: yaml: content: [";
        let file = create_test_config_file(yaml_content);
        let result = AppConfig::load(file.path().to_str().unwrap());
        assert!(result.is_err());
    }
}
