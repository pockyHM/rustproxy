use http::Request;

use crate::models::rule::{Condition, ConditionType, Rule};
use crate::proxy::conditions::{match_cookie, match_header, match_jwt};

pub struct Matcher {
    rules: Vec<Rule>,
}

impl Matcher {
    pub fn new(mut rules: Vec<Rule>) -> Self {
        rules.sort_by(|a, b| b.priority.cmp(&a.priority));
        Self { rules }
    }

    /// Match a request against all rules, returning the first matching rule.
    ///
    /// Rules are evaluated in priority order (descending). All conditions
    /// within a rule must match (AND logic).
    pub fn match_request(&self, request: &Request<()>) -> Option<&Rule> {
        self.rules
            .iter()
            .find(|rule| Self::rule_matches(rule, request))
    }

    /// Check if a single rule matches the request (all conditions must match).
    fn rule_matches(rule: &Rule, request: &Request<()>) -> bool {
        rule.conditions
            .iter()
            .all(|condition| Self::condition_matches(condition, request))
    }

    /// Check if a single condition matches the request.
    fn condition_matches(condition: &Condition, request: &Request<()>) -> bool {
        let headers = request.headers();

        match &condition.condition_type {
            ConditionType::Header => {
                let key = condition.key.as_deref().unwrap_or("");
                let value = condition.value.as_deref();
                match_header(headers, key, &condition.operator, value)
            }
            ConditionType::Cookie => {
                let key = condition.key.as_deref().unwrap_or("");
                let value = condition.value.as_deref();
                let cookie_header = headers
                    .get("Cookie")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                match_cookie(cookie_header, key, &condition.operator, value)
            }
            ConditionType::Jwt => {
                let claim_path = condition.claim_path.as_deref().unwrap_or("");
                let value = condition.value.as_deref();
                if let Some(jwt_token) = Self::extract_jwt_token(request) {
                    match_jwt(&jwt_token, claim_path, &condition.operator, value)
                } else {
                    false
                }
            }
        }
    }

    /// Extract JWT token from the Authorization header (Bearer token).
    fn extract_jwt_token(request: &Request<()>) -> Option<String> {
        let auth_header = request.headers().get("Authorization")?;
        let auth_str = auth_header.to_str().ok()?;
        let token = auth_str.strip_prefix("Bearer ")?;
        Some(token.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::Matcher;
    use crate::models::rule::{Condition, ConditionType, Operator, Rule};
    use http::{HeaderMap, HeaderValue, Request};

    fn create_request_with_headers(headers: &[(&str, &str)]) -> Request<()> {
        let mut req = Request::new(());
        let header_map: HeaderMap = headers
            .iter()
            .map(|(k, v)| (k.parse().unwrap(), HeaderValue::from_str(v).unwrap()))
            .collect();
        *req.headers_mut() = header_map;
        req
    }

    fn create_rule(priority: i32, conditions: Vec<Condition>) -> Rule {
        Rule {
            id: "test-rule".to_string(),
            name: "Test Rule".to_string(),
            priority,
            conditions,
            upstream: "backend-1".to_string(),
            weight: 100,
        }
    }

    fn create_header_condition(key: &str, operator: Operator, value: Option<&str>) -> Condition {
        Condition {
            condition_type: ConditionType::Header,
            key: Some(key.to_string()),
            claim_path: None,
            operator,
            value: value.map(|s| s.to_string()),
        }
    }

    fn create_cookie_condition(key: &str, operator: Operator, value: Option<&str>) -> Condition {
        Condition {
            condition_type: ConditionType::Cookie,
            key: Some(key.to_string()),
            claim_path: None,
            operator,
            value: value.map(|s| s.to_string()),
        }
    }

    fn create_jwt_condition(
        claim_path: &str,
        operator: Operator,
        value: Option<&str>,
    ) -> Condition {
        Condition {
            condition_type: ConditionType::Jwt,
            key: None,
            claim_path: Some(claim_path.to_string()),
            operator,
            value: value.map(|s| s.to_string()),
        }
    }

    // Helper to create a test JWT (header.payload.signature with base64url encoding)
    fn create_test_jwt(claims: serde_json::Value) -> String {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        use std::collections::HashMap;

        let header = HashMap::from([
            ("alg".to_string(), "HS256".to_string()),
            ("typ".to_string(), "JWT".to_string()),
        ]);
        let header_json = serde_json::to_string(&header).unwrap();
        let payload_json = serde_json::to_string(&claims).unwrap();

        let header_b64 = URL_SAFE_NO_PAD.encode(header_json.as_bytes());
        let payload_b64 = URL_SAFE_NO_PAD.encode(payload_json.as_bytes());
        let signature_b64 = "fake_signature".to_string();

        format!("{}.{}.{}", header_b64, payload_b64, signature_b64)
    }

    #[test]
    fn test_rules_sorted_by_priority_descending() {
        let rules = vec![
            create_rule(10, vec![]),
            create_rule(50, vec![]),
            create_rule(30, vec![]),
        ];
        let matcher = Matcher::new(rules);
        let request = create_request_with_headers(&[]);

        // Highest priority rule should match first (rule with priority 50)
        let result = matcher.match_request(&request);
        assert!(result.is_some());
        assert_eq!(result.unwrap().priority, 50);
    }

    #[test]
    fn test_and_logic_all_conditions_must_match() {
        let rule = create_rule(
            10,
            vec![
                create_header_condition("Host", Operator::Exact, Some("example.com")),
                create_header_condition("Content-Type", Operator::Exact, Some("application/json")),
            ],
        );

        let matcher = Matcher::new(vec![rule]);
        let request = create_request_with_headers(&[
            ("Host", "example.com"),
            ("Content-Type", "application/json"),
        ]);

        let result = matcher.match_request(&request);
        assert!(result.is_some());

        // Only one condition matches - should not match
        let request_partial = create_request_with_headers(&[
            ("Host", "example.com"),
            ("Content-Type", "text/html"), // Different value
        ]);
        let result_partial = matcher.match_request(&request_partial);
        assert!(result_partial.is_none());
    }

    #[test]
    fn test_returns_first_matching_rule() {
        // Two rules, first one should match
        let rule1 = create_rule(
            100,
            vec![create_header_condition(
                "Host",
                Operator::Exact,
                Some("example.com"),
            )],
        );
        let rule2 = create_rule(
            50,
            vec![create_header_condition(
                "Host",
                Operator::Exact,
                Some("example.com"),
            )],
        );

        let matcher = Matcher::new(vec![rule2, rule1]); // Inserted in reverse order
        let request = create_request_with_headers(&[("Host", "example.com")]);

        let result = matcher.match_request(&request);
        assert!(result.is_some());
        assert_eq!(result.unwrap().priority, 100); // Higher priority rule returned
    }

    #[test]
    fn test_returns_none_when_no_rule_matches() {
        let rule = create_rule(
            10,
            vec![create_header_condition(
                "Host",
                Operator::Exact,
                Some("example.com"),
            )],
        );

        let matcher = Matcher::new(vec![rule]);
        let request = create_request_with_headers(&[("Host", "other.com")]);

        let result = matcher.match_request(&request);
        assert!(result.is_none());
    }

    #[test]
    fn test_header_condition_matching() {
        let rule = create_rule(
            10,
            vec![create_header_condition("X-API-Key", Operator::Exists, None)],
        );

        let matcher = Matcher::new(vec![rule]);

        // Header exists - should match
        let request_with_header = create_request_with_headers(&[("X-API-Key", "secret123")]);
        assert!(matcher.match_request(&request_with_header).is_some());

        // Header missing - should not match
        let request_without_header = create_request_with_headers(&[("Host", "example.com")]);
        assert!(matcher.match_request(&request_without_header).is_none());
    }

    #[test]
    fn test_cookie_condition_matching() {
        let rule = create_rule(
            10,
            vec![create_cookie_condition(
                "session",
                Operator::Exact,
                Some("abc123"),
            )],
        );

        let matcher = Matcher::new(vec![rule]);

        // Cookie matches - should match
        let request = create_request_with_headers(&[("Cookie", "session=abc123; theme=dark")]);
        assert!(matcher.match_request(&request).is_some());

        // Cookie value different - should not match
        let request_diff = create_request_with_headers(&[("Cookie", "session=xyz789; theme=dark")]);
        assert!(matcher.match_request(&request_diff).is_none());

        // Cookie missing - should not match
        let request_no_cookie = create_request_with_headers(&[("Host", "example.com")]);
        assert!(matcher.match_request(&request_no_cookie).is_none());
    }

    #[test]
    fn test_jwt_condition_matching() {
        let claims = serde_json::json!({
            "sub": "user123",
            "role": "admin"
        });
        let token = create_test_jwt(claims);

        let rule = create_rule(
            10,
            vec![create_jwt_condition(
                "sub",
                Operator::Exact,
                Some("user123"),
            )],
        );

        let matcher = Matcher::new(vec![rule]);

        // Request with valid JWT - should match
        let mut request = create_request_with_headers(&[]);
        request.headers_mut().insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {}", token)).unwrap(),
        );
        assert!(matcher.match_request(&request).is_some());

        // Request with different JWT claim value - should not match
        let rule_diff = create_rule(
            10,
            vec![create_jwt_condition(
                "sub",
                Operator::Exact,
                Some("user456"),
            )],
        );
        let matcher_diff = Matcher::new(vec![rule_diff]);
        assert!(matcher_diff.match_request(&request).is_none());
    }

    #[test]
    fn test_jwt_extracted_from_authorization_header() {
        let claims = serde_json::json!({
            "sub": "user123"
        });
        let token = create_test_jwt(claims);

        let rule = create_rule(
            10,
            vec![create_jwt_condition("sub", Operator::Exists, None)],
        );

        let matcher = Matcher::new(vec![rule]);
        let mut request = create_request_with_headers(&[]);
        request.headers_mut().insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {}", token)).unwrap(),
        );

        // Should match with Bearer token
        assert!(matcher.match_request(&request).is_some());

        // Request with Basic auth - should not match (no valid JWT)
        let mut request_basic = create_request_with_headers(&[]);
        request_basic.headers_mut().insert(
            "Authorization",
            HeaderValue::from_static("Basic dXNlcjpwYXNz"),
        );
        assert!(matcher.match_request(&request_basic).is_none());

        // Request with no Authorization header - should not match
        let request_no_auth = create_request_with_headers(&[]);
        assert!(matcher.match_request(&request_no_auth).is_none());
    }

    #[test]
    fn test_empty_rules_returns_none() {
        let matcher = Matcher::new(vec![]);
        let request = create_request_with_headers(&[("Host", "example.com")]);
        assert!(matcher.match_request(&request).is_none());
    }

    #[test]
    fn test_rule_with_no_conditions_always_matches() {
        let rule = create_rule(10, vec![]);
        let matcher = Matcher::new(vec![rule]);
        let request = create_request_with_headers(&[("Host", "anything.com")]);
        assert!(matcher.match_request(&request).is_some());
    }

    #[test]
    fn test_multiple_rules_first_match_wins() {
        let rule1 = create_rule(
            100,
            vec![create_header_condition(
                "Host",
                Operator::Exact,
                Some("a.com"),
            )],
        );
        let rule2 = create_rule(
            90,
            vec![create_header_condition(
                "Host",
                Operator::Exact,
                Some("b.com"),
            )],
        );
        let rule3 = create_rule(
            80,
            vec![create_header_condition(
                "Host",
                Operator::Exact,
                Some("c.com"),
            )],
        );

        let matcher = Matcher::new(vec![rule1, rule2, rule3]);

        // Request matches rule3 (priority 80), but rule1 (priority 100) comes first
        let request = create_request_with_headers(&[("Host", "c.com")]);
        let result = matcher.match_request(&request);
        assert!(result.is_some());
        assert_eq!(result.unwrap().priority, 100);
    }

    #[test]
    fn test_complex_and_conditions_with_mixed_types() {
        let claims = serde_json::json!({
            "sub": "user123",
            "role": "admin"
        });
        let token = create_test_jwt(claims);

        let rule = create_rule(
            10,
            vec![
                create_header_condition("Host", Operator::Exact, Some("example.com")),
                create_cookie_condition("session", Operator::Regex, Some(r"^abc[0-9]+$")),
                create_jwt_condition("role", Operator::Exact, Some("admin")),
            ],
        );

        let matcher = Matcher::new(vec![rule]);

        // All conditions match
        let mut request = create_request_with_headers(&[
            ("Host", "example.com"),
            ("Cookie", "session=abc123; theme=dark"),
        ]);
        request.headers_mut().insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {}", token)).unwrap(),
        );
        assert!(matcher.match_request(&request).is_some());

        // One condition fails (cookie regex)
        let mut request_fail = create_request_with_headers(&[
            ("Host", "example.com"),
            ("Cookie", "session=xyz; theme=dark"), // Doesn't match regex
        ]);
        request_fail.headers_mut().insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {}", token)).unwrap(),
        );
        assert!(matcher.match_request(&request_fail).is_none());
    }
}
