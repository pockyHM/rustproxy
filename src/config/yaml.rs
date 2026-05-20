use crate::models::{HostMatchType, LocationMatchType, MatchSet, Rule, Upstream};
use crate::runtime::timeouts::{ConnectionLimitPolicy, TimeoutPolicy};
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
    #[serde(default)]
    pub level: AccessLogLevel,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccessLogLevel {
    Debug,
    #[default]
    Info,
    Warn,
    Error,
}

impl AccessLogLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            AccessLogLevel::Debug => "debug",
            AccessLogLevel::Info => "info",
            AccessLogLevel::Warn => "warn",
            AccessLogLevel::Error => "error",
        }
    }

    pub fn severity(self) -> u8 {
        match self {
            AccessLogLevel::Debug => 0,
            AccessLogLevel::Info => 1,
            AccessLogLevel::Warn => 2,
            AccessLogLevel::Error => 3,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MonitoringConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub prometheus: PrometheusConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PrometheusConfig {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub auth: PrometheusAuthConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrometheusAuthConfig {
    #[serde(default = "default_prometheus_auth_type")]
    pub auth_type: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub bearer_token: Option<String>,
    #[serde(default)]
    pub header_name: Option<String>,
    #[serde(default)]
    pub header_value: Option<String>,
}

impl Default for PrometheusAuthConfig {
    fn default() -> Self {
        Self {
            auth_type: default_prometheus_auth_type(),
            username: None,
            password: None,
            bearer_token: None,
            header_name: None,
            header_value: None,
        }
    }
}

impl Default for AccessLogConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            path: None,
            buffer_size: default_access_log_buffer_size(),
            level: AccessLogLevel::Info,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default = "default_proxy_listen")]
    pub proxy_listen: String,
    #[serde(default, skip_serializing_if = "TimeoutPolicy::is_default")]
    pub timeouts: TimeoutPolicy,
    #[serde(default, skip_serializing_if = "ConnectionLimitPolicy::is_default")]
    pub limits: ConnectionLimitPolicy,
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
    pub monitoring: MonitoringConfig,
    #[serde(default)]
    pub certificates: Vec<Certificate>,
    #[serde(default)]
    pub tls_listeners: Vec<TlsListener>,
    #[serde(default)]
    pub match_sets: Vec<MatchSet>,
    #[serde(default)]
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub upstreams: HashMap<String, Upstream>,
    pub fallback: Fallback,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            proxy_listen: default_proxy_listen(),
            timeouts: TimeoutPolicy::default(),
            limits: ConnectionLimitPolicy::default(),
            connect_timeout: default_connect_timeout(),
            request_timeout: default_request_timeout(),
            pool_max_idle_per_host: default_pool_max_idle_per_host(),
            pool_idle_timeout: default_pool_idle_timeout(),
            tcp_keepalive: default_tcp_keepalive(),
            certificate_dir: default_certificate_dir(),
            access_log: AccessLogConfig::default(),
            monitoring: MonitoringConfig::default(),
            certificates: Vec::new(),
            tls_listeners: Vec::new(),
            match_sets: Vec::new(),
            rules: Vec::new(),
            upstreams: HashMap::new(),
            fallback: Fallback {
                url: "404".to_string(),
            },
        }
    }
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

fn default_prometheus_auth_type() -> String {
    "none".to_string()
}

impl AppConfig {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let mut config: AppConfig = serde_yaml::from_str(&content)?;
        config.normalize_timeout_aliases();
        config.normalize_rules();
        Ok(config)
    }

    pub fn to_compact_yaml(&self) -> anyhow::Result<String> {
        let mut config = self.clone();
        config.normalize_timeout_aliases();
        let mut value = serde_yaml::to_value(&config)?;
        compact_yaml_value(&mut value);
        Ok(serde_yaml::to_string(&value)?)
    }

    pub fn normalize_timeout_aliases(&mut self) {
        let legacy_alias_changed = self.connect_timeout != default_connect_timeout()
            || self.request_timeout != default_request_timeout()
            || self.pool_idle_timeout != default_pool_idle_timeout();

        if self.timeouts.is_default() && legacy_alias_changed {
            self.timeouts.connect_timeout_seconds = self.connect_timeout;
            self.timeouts.server_timeout_seconds = self.request_timeout;
            self.timeouts.http_request_timeout_seconds = self.request_timeout;
            self.timeouts.http_keepalive_timeout_seconds = self.pool_idle_timeout;
        } else {
            self.connect_timeout = self.timeouts.connect_timeout_seconds;
            self.request_timeout = self.timeouts.server_timeout_seconds;
            self.pool_idle_timeout = self.timeouts.http_keepalive_timeout_seconds;
        }
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
            rule.request_timeout = 0;
            rule.tls = None;
        }
    }
}

fn compact_yaml_value(value: &mut serde_yaml::Value) -> bool {
    match value {
        serde_yaml::Value::Null => true,
        serde_yaml::Value::Sequence(items) => {
            items.retain_mut(|item| !compact_yaml_value(item));
            items.is_empty()
        }
        serde_yaml::Value::Mapping(mapping) => {
            compact_known_disabled_sections(mapping);
            mapping.retain(|_, item| !compact_yaml_value(item));
            mapping.is_empty()
        }
        _ => false,
    }
}

fn compact_known_disabled_sections(mapping: &mut serde_yaml::Mapping) {
    if bool_field(mapping, "enabled") == Some(false) {
        if mapping.contains_key(&key("prometheus")) {
            retain_only(mapping, &["enabled"]);
        } else if mapping.contains_key(&key("mode"))
            && mapping.contains_key(&key("expected_status"))
            && mapping.contains_key(&key("interval_seconds"))
        {
            retain_only(mapping, &["enabled"]);
        } else if mapping.contains_key(&key("buffer_size")) || mapping.contains_key(&key("level")) {
            retain_only(mapping, &["enabled"]);
        }
    }

    if numeric_field(mapping, "request_timeout") == Some(0)
        && mapping.contains_key(&key("upstream"))
        && mapping.contains_key(&key("priority"))
    {
        mapping.remove(&key("request_timeout"));
    }

    if string_field(mapping, "rate_key") == Some("ip")
        && mapping.contains_key(&key("rate_per_second"))
        && mapping.contains_key(&key("max_connections"))
        && mapping.contains_key(&key("max_body_bytes"))
        && mapping.contains_key(&key("queue_timeout_ms"))
    {
        mapping.remove(&key("rate_key"));
    }

    if string_field(mapping, "balance") == Some("weighted_round_robin") {
        mapping.remove(&key("balance"));
    }

    if numeric_field(mapping, "attempts") == Some(0)
        && mapping.contains_key(&key("retry_on_timeout"))
        && mapping.contains_key(&key("retry_on_connect_error"))
    {
        mapping.remove(&key("attempts"));
    }
    if bool_field(mapping, "retry_on_timeout") == Some(false) {
        mapping.remove(&key("retry_on_timeout"));
    }
    if bool_field(mapping, "retry_on_connect_error") == Some(false) {
        mapping.remove(&key("retry_on_connect_error"));
    }
}

fn bool_field(mapping: &serde_yaml::Mapping, field: &str) -> Option<bool> {
    match mapping.get(&key(field)) {
        Some(serde_yaml::Value::Bool(value)) => Some(*value),
        _ => None,
    }
}

fn numeric_field(mapping: &serde_yaml::Mapping, field: &str) -> Option<i64> {
    match mapping.get(&key(field)) {
        Some(serde_yaml::Value::Number(value)) => value.as_i64(),
        _ => None,
    }
}

fn string_field<'a>(mapping: &'a serde_yaml::Mapping, field: &str) -> Option<&'a str> {
    match mapping.get(&key(field)) {
        Some(serde_yaml::Value::String(value)) => Some(value),
        _ => None,
    }
}

fn retain_only(mapping: &mut serde_yaml::Mapping, fields: &[&str]) {
    mapping.retain(|key, _| {
        key.as_str()
            .is_some_and(|field| fields.iter().any(|allowed| field == *allowed))
    });
}

fn key(field: &str) -> serde_yaml::Value {
    serde_yaml::Value::String(field.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Target;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_test_config_file(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file.flush().unwrap();
        file
    }

    #[test]
    fn timeout_policy_defaults_keep_existing_behavior() {
        let yaml = r#"
fallback: { url: "404" }
"#;
        let config: AppConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.timeouts.connect_timeout_seconds, 10);
        assert_eq!(config.timeouts.server_timeout_seconds, 60);
        assert_eq!(config.limits.global_maxconn, None);
    }

    #[test]
    fn timeout_policy_wins_over_legacy_aliases_when_both_present() {
        let yaml = r#"
connect_timeout: 2
request_timeout: 30
pool_idle_timeout: 45
timeouts:
  connect_timeout_seconds: 3
  server_timeout_seconds: 7
  http_keepalive_timeout_seconds: 11
fallback: { url: "404" }
"#;
        let file = create_test_config_file(yaml);
        let config = AppConfig::load(file.path().to_str().unwrap()).unwrap();

        assert_eq!(config.timeouts.connect_timeout_seconds, 3);
        assert_eq!(config.timeouts.server_timeout_seconds, 7);
        assert_eq!(config.timeouts.http_keepalive_timeout_seconds, 11);
        assert_eq!(config.connect_timeout, 3);
        assert_eq!(config.request_timeout, 7);
        assert_eq!(config.pool_idle_timeout, 11);
    }

    #[test]
    fn test_app_config_load() {
        let yaml_content = r#"
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
listen: "0.0.0.0:8080"
rules: []
upstreams: {}
fallback:
  url: "http://fallback.example.com"
"#;
        let file = create_test_config_file(yaml_content);
        let config = AppConfig::load(file.path().to_str().unwrap()).unwrap();

        assert!(config.rules.is_empty());
        assert!(config.upstreams.is_empty());
    }

    #[test]
    fn test_app_config_load_multiple_rules_and_upstreams() {
        let yaml_content = r#"
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
    fn test_compact_yaml_hides_disabled_and_empty_display_fields() {
        let mut upstreams = HashMap::new();
        upstreams.insert(
            "backend".to_string(),
            Upstream {
                name: "backend".to_string(),
                skip_ssl: false,
                websocket: false,
                targets: vec![Target {
                    url: "http://localhost:8080".to_string(),
                    weight: 100,
                    timeouts: Default::default(),
                }],
                health_check: Default::default(),
                balance: Default::default(),
                retry: Default::default(),
                timeouts: Default::default(),
            },
        );
        let config = AppConfig {
            listen: "0.0.0.0:3000".to_string(),
            proxy_listen: "0.0.0.0:80".to_string(),
            timeouts: Default::default(),
            limits: Default::default(),
            connect_timeout: 10,
            request_timeout: 60,
            pool_max_idle_per_host: 32,
            pool_idle_timeout: 90,
            tcp_keepalive: 60,
            certificate_dir: "/etc/rustproxy/cert.d".to_string(),
            access_log: AccessLogConfig {
                enabled: false,
                path: Some("/tmp/access.log".to_string()),
                buffer_size: Some(8192),
                level: AccessLogLevel::Debug,
            },
            monitoring: MonitoringConfig {
                enabled: false,
                prometheus: PrometheusConfig {
                    url: "http://prometheus:9090".to_string(),
                    auth: PrometheusAuthConfig {
                        auth_type: "basic".to_string(),
                        username: Some("admin".to_string()),
                        password: Some("secret".to_string()),
                        bearer_token: None,
                        header_name: None,
                        header_value: None,
                    },
                },
            },
            certificates: Vec::new(),
            tls_listeners: Vec::new(),
            match_sets: Vec::new(),
            rules: vec![Rule {
                id: "rule-1".to_string(),
                name: "Rule 1".to_string(),
                priority: 10,
                host: Default::default(),
                location: Default::default(),
                match_set: None,
                conditions: None,
                upstream: "backend".to_string(),
                weight: 100,
                is_fallback: false,
                listen: None,
                request_timeout: 0,
                timeouts: Default::default(),
                tls: None,
                header_policy: Default::default(),
                path_actions: Vec::new(),
                limit_policy: Default::default(),
            }],
            upstreams,
            fallback: Fallback {
                url: "404".to_string(),
            },
        };

        let full_monitoring_json = serde_json::to_string(&config.monitoring).unwrap();
        let yaml = config.to_compact_yaml().unwrap();

        assert!(full_monitoring_json.contains("prometheus"));
        assert!(yaml.contains("monitoring:"));
        assert!(!yaml.contains("prometheus:"));
        assert!(yaml.contains("health_check:"));
        assert!(!yaml.contains("expected_status:"));
        assert!(!yaml.contains("request_timeout: 0"));
        assert!(!yaml.contains("header_policy:"));
        assert!(!yaml.contains("path_actions:"));
        assert!(!yaml.contains("limit_policy:"));
        assert!(!yaml.contains("balance: weighted_round_robin"));
        assert!(!yaml.contains("retry:"));
        assert!(!yaml.contains("timeouts:"));
        assert!(!yaml.contains("limits:"));
        assert!(!yaml.contains("certificates: []"));
        assert!(!yaml.contains("path: /tmp/access.log"));
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
