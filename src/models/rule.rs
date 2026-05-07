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
            conditions: vec![
                Condition {
                    condition_type: ConditionType::Header,
                    key: Some("Host".to_string()),
                    claim_path: None,
                    operator: Operator::Exact,
                    value: Some("example.com".to_string()),
                },
            ],
            upstream: "backend-1".to_string(),
            weight: 100,
        };
        let json = serde_json::to_string(&rule).unwrap();
        let parsed: Rule = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "rule-1");
        assert_eq!(parsed.name, "Test Rule");
        assert_eq!(parsed.priority, 10);
        assert_eq!(parsed.conditions.len(), 1);
        assert_eq!(parsed.upstream, "backend-1");
        assert_eq!(parsed.weight, 100);
    }

    #[test]
    fn test_rule_multiple_conditions() {
        let rule = Rule {
            id: "rule-2".to_string(),
            name: "Multi Condition Rule".to_string(),
            priority: 5,
            conditions: vec![
                Condition {
                    condition_type: ConditionType::Header,
                    key: Some("X-Api-Key".to_string()),
                    claim_path: None,
                    operator: Operator::Exists,
                    value: None,
                },
                Condition {
                    condition_type: ConditionType::Cookie,
                    key: Some("session".to_string()),
                    claim_path: None,
                    operator: Operator::Regex,
                    value: Some("^abc[0-9]+$".to_string()),
                },
                Condition {
                    condition_type: ConditionType::Jwt,
                    key: None,
                    claim_path: Some("sub".to_string()),
                    operator: Operator::Exact,
                    value: Some("user123".to_string()),
                },
            ],
            upstream: "backend-2".to_string(),
            weight: 50,
        };
        assert_eq!(rule.conditions.len(), 3);
        assert_eq!(rule.conditions[0].condition_type, ConditionType::Header);
        assert_eq!(rule.conditions[1].condition_type, ConditionType::Cookie);
        assert_eq!(rule.conditions[2].condition_type, ConditionType::Jwt);
    }

    #[test]
    fn test_rule_clone_and_partial_eq() {
        let rule1 = Rule {
            id: "rule-1".to_string(),
            name: "Test".to_string(),
            priority: 1,
            conditions: vec![],
            upstream: "up".to_string(),
            weight: 100,
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
    Header,
    Cookie,
    Jwt,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Operator {
    Exact,
    Regex,
    Exists,
    Contains,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Condition {
    #[serde(rename = "type")]
    pub condition_type: ConditionType,
    pub key: Option<String>,
    pub claim_path: Option<String>,
    pub operator: Operator,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub priority: i32,
    pub conditions: Vec<Condition>,
    pub upstream: String,
    pub weight: u32,
}
