use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_condition_type_variants() {
        assert_eq!(ConditionType::Header, ConditionType::Header);
        assert_eq!(ConditionType::Cookie, ConditionType::Cookie);
        assert_eq!(ConditionType::Jwt, ConditionType::Jwt);
    }

    #[test]
    fn test_condition_type_serde() {
        let header = ConditionType::Header;
        let json = serde_json::to_string(&header).unwrap();
        assert_eq!(json, "\"header\"");
        let parsed: ConditionType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ConditionType::Header);
    }

    #[test]
    fn test_operator_variants() {
        assert_eq!(Operator::Exact, Operator::Exact);
        assert_eq!(Operator::Regex, Operator::Regex);
        assert_eq!(Operator::Exists, Operator::Exists);
        assert_eq!(Operator::Contains, Operator::Contains);
    }

    #[test]
    fn test_operator_serde() {
        let exact = Operator::Exact;
        let json = serde_json::to_string(&exact).unwrap();
        assert_eq!(json, "\"exact\"");
        let parsed: Operator = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, Operator::Exact);

        let regex = Operator::Regex;
        let json = serde_json::to_string(&regex).unwrap();
        assert_eq!(json, "\"regex\"");
        let parsed: Operator = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, Operator::Regex);
    }

    #[test]
    fn test_condition_serde() {
        let condition = Condition {
            condition_type: ConditionType::Header,
            key: Some("Content-Type".to_string()),
            claim_path: None,
            operator: Operator::Exact,
            value: Some("application/json".to_string()),
        };
        let json = serde_json::to_string(&condition).unwrap();
        assert!(json.contains("\"type\":\"header\""));
        assert!(json.contains("\"operator\":\"exact\""));
        let parsed: Condition = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.condition_type, ConditionType::Header);
        assert_eq!(parsed.key, Some("Content-Type".to_string()));
        assert_eq!(parsed.operator, Operator::Exact);
        assert_eq!(parsed.value, Some("application/json".to_string()));
    }

    #[test]
    fn test_condition_jwt_with_claim_path() {
        let condition = Condition {
            condition_type: ConditionType::Jwt,
            key: None,
            claim_path: Some("roles".to_string()),
            operator: Operator::Contains,
            value: Some("admin".to_string()),
        };
        let json = serde_json::to_string(&condition).unwrap();
        let parsed: Condition = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.condition_type, ConditionType::Jwt);
        assert_eq!(parsed.claim_path, Some("roles".to_string()));
        assert_eq!(parsed.operator, Operator::Contains);
    }

    #[test]
    fn test_condition_exists_operator() {
        let condition = Condition {
            condition_type: ConditionType::Header,
            key: Some("X-Custom-Header".to_string()),
            claim_path: None,
            operator: Operator::Exists,
            value: None,
        };
        let json = serde_json::to_string(&condition).unwrap();
        let parsed: Condition = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.operator, Operator::Exists);
        assert_eq!(parsed.value, None);
    }

    #[test]
    fn test_rule_serde() {
        let rule = Rule {
            id: "rule-1".to_string(),
            name: "Test Rule".to_string(),
            priority: 10,
            host: HostMatcher::default(),
            location: LocationMatcher::default(),
            match_set: None,
            conditions: Some(ConditionExpr::And {
                children: vec![ConditionExpr::Leaf {
                    condition_type: ConditionType::Header,
                    key: Some("Host".to_string()),
                    claim_path: None,
                    operator: Operator::Exact,
                    value: Some("example.com".to_string()),
                }],
            }),
            upstream: "backend-1".to_string(),
            weight: 100,
            is_fallback: false,
            listen: None,
            request_timeout: 0,
            timeouts: Default::default(),
            tls: None,
            header_policy: Default::default(),
            path_actions: Vec::new(),
            limit_policy: Default::default(),
        };
        let json = serde_json::to_string(&rule).unwrap();
        let parsed: Rule = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "rule-1");
        assert_eq!(parsed.name, "Test Rule");
        assert_eq!(parsed.priority, 10);
        assert!(parsed.conditions.is_some());
        assert_eq!(parsed.upstream, "backend-1");
        assert_eq!(parsed.weight, 100);
    }

    #[test]
    fn test_rule_backward_compat_legacy_array() {
        // Old format: flat array of conditions should deserialize into AND expression
        let json = r#"{
            "id": "rule-1",
            "name": "Test",
            "priority": 10,
            "conditions": [
                {"type": "header", "key": "Host", "operator": "exact", "value": "example.com"},
                {"type": "cookie", "key": "session", "operator": "exists"}
            ],
            "upstream": "backend-1",
            "weight": 100
        }"#;
        let parsed: Rule = serde_json::from_str(json).unwrap();
        let expr = parsed.conditions.unwrap();
        match expr {
            ConditionExpr::And { children } => assert_eq!(children.len(), 2),
            _ => panic!("expected AND from legacy array"),
        }
    }

    #[test]
    fn test_rule_backward_compat_empty_conditions() {
        let json = r#"{
            "id": "rule-1",
            "name": "Test",
            "priority": 10,
            "conditions": [],
            "upstream": "backend-1",
            "weight": 100
        }"#;
        let parsed: Rule = serde_json::from_str(json).unwrap();
        assert!(parsed.conditions.is_none());
    }

    #[test]
    fn test_rule_or_expression() {
        let rule = Rule {
            id: "rule-1".to_string(),
            name: "OR Rule".to_string(),
            priority: 10,
            host: HostMatcher::default(),
            location: LocationMatcher::default(),
            match_set: None,
            conditions: Some(ConditionExpr::Or {
                children: vec![
                    ConditionExpr::Leaf {
                        condition_type: ConditionType::Host,
                        key: None,
                        claim_path: None,
                        operator: Operator::Exact,
                        value: Some("a.com".to_string()),
                    },
                    ConditionExpr::Leaf {
                        condition_type: ConditionType::Host,
                        key: None,
                        claim_path: None,
                        operator: Operator::Exact,
                        value: Some("b.com".to_string()),
                    },
                ],
            }),
            upstream: "backend-1".to_string(),
            weight: 100,
            is_fallback: false,
            listen: None,
            request_timeout: 0,
            timeouts: Default::default(),
            tls: None,
            header_policy: Default::default(),
            path_actions: Vec::new(),
            limit_policy: Default::default(),
        };
        let json = serde_json::to_string(&rule).unwrap();
        let parsed: Rule = serde_json::from_str(&json).unwrap();
        match parsed.conditions.unwrap() {
            ConditionExpr::Or { children } => assert_eq!(children.len(), 2),
            _ => panic!("expected OR"),
        }
    }

    #[test]
    fn test_rule_clone_and_partial_eq() {
        let rule1 = Rule {
            id: "rule-1".to_string(),
            name: "Test".to_string(),
            priority: 1,
            host: HostMatcher::default(),
            location: LocationMatcher::default(),
            match_set: None,
            conditions: None,
            upstream: "up".to_string(),
            weight: 100,
            is_fallback: false,
            listen: None,
            request_timeout: 0,
            timeouts: Default::default(),
            tls: None,
            header_policy: Default::default(),
            path_actions: Vec::new(),
            limit_policy: Default::default(),
        };
        let rule2 = rule1.clone();
        assert_eq!(rule1, rule2);
    }

    #[test]
    fn test_rule_policy_defaults() {
        let json = r#"{
            "id":"r1","name":"R1","priority":10,"upstream":"api","weight":100
        }"#;
        let rule: Rule = serde_json::from_str(json).unwrap();
        assert!(rule.header_policy.request.is_empty());
        assert!(rule.header_policy.response.is_empty());
        assert!(rule.path_actions.is_empty());
        assert_eq!(rule.limit_policy.rate_per_second, None);
        assert_eq!(rule.timeouts, RuleTimeoutPolicy::default());
    }

    #[test]
    fn test_rule_timeout_policy_serde() {
        let json = r#"{
            "id":"r1",
            "name":"R1",
            "priority":10,
            "upstream":"api",
            "weight":100,
            "timeouts":{"server_timeout_seconds":7}
        }"#;

        let rule: Rule = serde_json::from_str(json).unwrap();

        assert_eq!(rule.timeouts.server_timeout_seconds, Some(7));
        let serialized = serde_json::to_string(&rule).unwrap();
        assert!(serialized.contains("\"timeouts\""));
        assert!(serialized.contains("\"server_timeout_seconds\":7"));
    }

    #[test]
    fn test_rule_omits_inherited_request_timeout() {
        let rule = Rule {
            id: "rule-1".to_string(),
            name: "Test".to_string(),
            priority: 1,
            host: HostMatcher::default(),
            location: LocationMatcher::default(),
            match_set: None,
            conditions: None,
            upstream: "up".to_string(),
            weight: 100,
            is_fallback: false,
            listen: None,
            request_timeout: 0,
            timeouts: Default::default(),
            tls: None,
            header_policy: Default::default(),
            path_actions: Vec::new(),
            limit_policy: Default::default(),
        };

        let yaml = crate::config::yaml::AppConfig {
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
            access_log: Default::default(),
            monitoring: Default::default(),
            certificates: Vec::new(),
            tls_listeners: Vec::new(),
            match_sets: Vec::new(),
            rules: vec![rule],
            upstreams: std::collections::HashMap::new(),
            fallback: crate::config::yaml::Fallback {
                url: "404".to_string(),
            },
        }
        .to_compact_yaml()
        .unwrap();

        assert!(!yaml.contains("    request_timeout: 0"));
    }

    #[test]
    fn test_condition_clone() {
        let cond = Condition {
            condition_type: ConditionType::Header,
            key: Some("key".to_string()),
            claim_path: None,
            operator: Operator::Exact,
            value: Some("value".to_string()),
        };
        let cloned = cond.clone();
        assert_eq!(cond, cloned);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConditionType {
    Host,
    Path,
    Header,
    Cookie,
    Jwt,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Operator {
    Exact,
    Prefix,
    Regex,
    Exists,
    Contains,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    #[serde(rename = "type")]
    pub condition_type: ConditionType,
    pub key: Option<String>,
    pub claim_path: Option<String>,
    pub operator: Operator,
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub priority: i32,
    pub host: HostMatcher,
    pub location: LocationMatcher,
    pub match_set: Option<String>,
    pub conditions: Option<ConditionExpr>,
    pub upstream: String,
    pub weight: u32,
    pub is_fallback: bool,
    /// Dedicated listen address for this rule (e.g. "0.0.0.0:9090").
    /// If set, the proxy binds a separate listener and routes all traffic
    /// on that port through this rule's upstream. If None, uses the default port.
    pub listen: Option<String>,
    /// Request timeout override in seconds. 0 inherits the global request_timeout.
    pub request_timeout: u64,
    pub timeouts: RuleTimeoutPolicy,
    pub tls: Option<RuleTls>,
    pub header_policy: HeaderPolicy,
    pub path_actions: Vec<PathAction>,
    pub limit_policy: LimitPolicy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeaderMutationOp {
    Set,
    Add,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeaderMutation {
    pub op: HeaderMutationOp,
    pub name: String,
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HeaderPolicy {
    #[serde(default)]
    pub request: Vec<HeaderMutation>,
    #[serde(default)]
    pub response: Vec<HeaderMutation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathAction {
    StripPrefix {
        prefix: String,
    },
    Rewrite {
        pattern: String,
        replacement: String,
    },
    Redirect {
        status: u16,
        location: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LimitPolicy {
    #[serde(default)]
    pub rate_per_second: Option<u32>,
    #[serde(default)]
    pub rate_key: RateLimitKey,
    #[serde(default)]
    pub max_connections: Option<u32>,
    #[serde(default)]
    pub max_body_bytes: Option<u64>,
    #[serde(default)]
    pub queue_timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleTimeoutPolicy {
    #[serde(default)]
    pub connect_timeout_seconds: Option<u64>,
    #[serde(default)]
    pub client_timeout_seconds: Option<u64>,
    #[serde(default)]
    pub server_timeout_seconds: Option<u64>,
    #[serde(default)]
    pub http_request_timeout_seconds: Option<u64>,
    #[serde(default)]
    pub http_keepalive_timeout_seconds: Option<u64>,
    #[serde(default)]
    pub tunnel_timeout_seconds: Option<u64>,
    #[serde(default)]
    pub queue_timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateLimitKey {
    #[default]
    Ip,
    Host,
    Route,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostMatcher {
    #[serde(rename = "type")]
    pub match_type: HostMatchType,
    pub value: Option<String>,
}

impl Default for HostMatcher {
    fn default() -> Self {
        Self {
            match_type: HostMatchType::Any,
            value: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HostMatchType {
    Any,
    Exact,
    Wildcard,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocationMatcher {
    #[serde(rename = "type")]
    pub match_type: LocationMatchType,
    pub value: String,
}

impl Default for LocationMatcher {
    fn default() -> Self {
        Self {
            match_type: LocationMatchType::Prefix,
            value: "/".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LocationMatchType {
    Exact,
    Prefix,
    Regex,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatchSet {
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_conditions")]
    pub conditions: Option<ConditionExpr>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleTls {
    #[serde(default)]
    pub enabled: bool,
    pub certificate: String,
}

/// Helper to deserialize conditions: accepts both old flat array and new expression tree.
fn deserialize_conditions<'de, D>(deserializer: D) -> Result<Option<ConditionExpr>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value: serde_json::Value = serde::Deserialize::deserialize(deserializer)?;

    // Try new expression format first
    if let Ok(expr) = serde_json::from_value::<ConditionExpr>(value.clone()) {
        return Ok(Some(expr));
    }

    // Try old flat array format -> wrap in AND
    if let Ok(conditions) = serde_json::from_value::<Vec<Condition>>(value.clone()) {
        if conditions.is_empty() {
            return Ok(None);
        }
        let children: Vec<ConditionExpr> = conditions
            .into_iter()
            .map(|c| ConditionExpr::Leaf {
                condition_type: c.condition_type,
                key: c.key,
                claim_path: c.claim_path,
                operator: c.operator,
                value: c.value,
            })
            .collect();
        return Ok(Some(ConditionExpr::And { children }));
    }

    // Null or missing
    Ok(None)
}

impl Serialize for Rule {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Rule", 17)?;
        state.serialize_field("id", &self.id)?;
        state.serialize_field("name", &self.name)?;
        state.serialize_field("priority", &self.priority)?;
        state.serialize_field("host", &self.host)?;
        state.serialize_field("location", &self.location)?;
        state.serialize_field("match_set", &self.match_set)?;
        state.serialize_field("conditions", &self.conditions)?;
        state.serialize_field("upstream", &self.upstream)?;
        state.serialize_field("weight", &self.weight)?;
        state.serialize_field("is_fallback", &self.is_fallback)?;
        state.serialize_field("listen", &self.listen)?;
        state.serialize_field("request_timeout", &self.request_timeout)?;
        state.serialize_field("timeouts", &self.timeouts)?;
        state.serialize_field("tls", &self.tls)?;
        state.serialize_field("header_policy", &self.header_policy)?;
        state.serialize_field("path_actions", &self.path_actions)?;
        state.serialize_field("limit_policy", &self.limit_policy)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for Rule {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RuleHelper {
            #[serde(default)]
            id: String,
            name: String,
            priority: i32,
            #[serde(default)]
            host: HostMatcher,
            #[serde(default)]
            location: LocationMatcher,
            #[serde(default)]
            match_set: Option<String>,
            #[serde(default, deserialize_with = "deserialize_conditions")]
            conditions: Option<ConditionExpr>,
            upstream: String,
            weight: u32,
            #[serde(default)]
            is_fallback: bool,
            #[serde(default)]
            listen: Option<String>,
            #[serde(default)]
            request_timeout: u64,
            #[serde(default)]
            timeouts: RuleTimeoutPolicy,
            #[serde(default)]
            tls: Option<RuleTls>,
            #[serde(default)]
            header_policy: HeaderPolicy,
            #[serde(default)]
            path_actions: Vec<PathAction>,
            #[serde(default)]
            limit_policy: LimitPolicy,
        }
        let helper = RuleHelper::deserialize(deserializer)?;
        let mut timeouts = helper.timeouts;
        if timeouts.server_timeout_seconds.is_none() && helper.request_timeout > 0 {
            timeouts.server_timeout_seconds = Some(helper.request_timeout);
        }

        Ok(Rule {
            id: helper.id,
            name: helper.name,
            priority: helper.priority,
            host: helper.host,
            location: helper.location,
            match_set: helper.match_set,
            conditions: helper.conditions,
            upstream: helper.upstream,
            weight: helper.weight,
            is_fallback: helper.is_fallback,
            listen: helper.listen,
            request_timeout: helper.request_timeout,
            timeouts,
            tls: helper.tls,
            header_policy: helper.header_policy,
            path_actions: helper.path_actions,
            limit_policy: helper.limit_policy,
        })
    }
}

/// Recursive boolean expression tree for rule conditions.
///
/// - `Leaf`: a single condition match
/// - `And`: all children must match
/// - `Or`: any child must match
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ConditionExpr {
    #[serde(rename = "leaf")]
    Leaf {
        #[serde(rename = "conditionType")]
        condition_type: ConditionType,
        key: Option<String>,
        #[serde(rename = "claimPath")]
        claim_path: Option<String>,
        operator: Operator,
        value: Option<String>,
    },
    #[serde(rename = "and")]
    And { children: Vec<ConditionExpr> },
    #[serde(rename = "or")]
    Or { children: Vec<ConditionExpr> },
}
