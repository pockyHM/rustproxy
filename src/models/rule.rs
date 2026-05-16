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
            tls: None,
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
            tls: None,
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
            match_set: None,
            conditions: None,
            upstream: "up".to_string(),
            weight: 100,
            is_fallback: false,
            listen: None,
            tls: None,
        };
        let rule2 = rule1.clone();
        assert_eq!(rule1, rule2);
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
    pub match_set: Option<String>,
    pub conditions: Option<ConditionExpr>,
    pub upstream: String,
    pub weight: u32,
    pub is_fallback: bool,
    /// Dedicated listen address for this rule (e.g. "0.0.0.0:9090").
    /// If set, the proxy binds a separate listener and routes all traffic
    /// on that port through this rule's upstream. If None, uses the default port.
    pub listen: Option<String>,
    pub tls: Option<RuleTls>,
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
        let mut state = serializer.serialize_struct("Rule", 10)?;
        state.serialize_field("id", &self.id)?;
        state.serialize_field("name", &self.name)?;
        state.serialize_field("priority", &self.priority)?;
        state.serialize_field("match_set", &self.match_set)?;
        state.serialize_field("conditions", &self.conditions)?;
        state.serialize_field("upstream", &self.upstream)?;
        state.serialize_field("weight", &self.weight)?;
        state.serialize_field("is_fallback", &self.is_fallback)?;
        state.serialize_field("listen", &self.listen)?;
        state.serialize_field("tls", &self.tls)?;
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
            tls: Option<RuleTls>,
        }
        let helper = RuleHelper::deserialize(deserializer)?;
        Ok(Rule {
            id: helper.id,
            name: helper.name,
            priority: helper.priority,
            match_set: helper.match_set,
            conditions: helper.conditions,
            upstream: helper.upstream,
            weight: helper.weight,
            is_fallback: helper.is_fallback,
            listen: helper.listen,
            tls: helper.tls,
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
