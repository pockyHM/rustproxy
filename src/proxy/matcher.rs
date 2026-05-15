use std::{cell::OnceCell, collections::HashMap};

use http::{HeaderMap, Request};
use regex::Regex;
use serde_json::Value;

use crate::models::rule::{ConditionExpr, ConditionType, Operator, Rule};

pub struct Matcher {
    rules: Vec<CompiledRule>,
    default_rules: RuleBucket,
    listen_rules: HashMap<String, RuleBucket>,
}

struct CompiledRule {
    rule: Rule,
    conditions: Option<CompiledExpr>,
}

enum CompiledExpr {
    And { children: Vec<CompiledExpr> },
    Or { children: Vec<CompiledExpr> },
    Leaf(CompiledLeaf),
}

struct CompiledLeaf {
    condition_type: ConditionType,
    operator: Operator,
    value: Option<String>,
    key: Option<String>,
    claim_path: Vec<String>,
    regex: Option<Regex>,
}

struct EvalContext<'a> {
    request: &'a Request<()>,
    jwt_payload: OnceCell<Option<Value>>,
}

#[derive(Default)]
struct RuleBucket {
    general: Vec<usize>,
    host_exact: HashMap<String, Vec<usize>>,
}

impl Matcher {
    pub fn new(mut rules: Vec<Rule>) -> Self {
        rules.sort_by(|a, b| b.priority.cmp(&a.priority));
        let rules: Vec<_> = rules
            .into_iter()
            .map(|rule| {
                let conditions = rule.conditions.as_ref().map(CompiledExpr::from_expr);
                CompiledRule { rule, conditions }
            })
            .collect();
        let (default_rules, listen_rules) = Self::build_index(&rules);
        Self {
            rules,
            default_rules,
            listen_rules,
        }
    }

    /// Match a request against all rules, returning the first matching rule.
    ///
    /// If `listen_addr` is Some, only rules with that exact `listen` value
    /// are considered. If None, only rules without a `listen` field are considered
    /// (i.e. the default port).
    pub fn match_request(&self, request: &Request<()>, listen_addr: Option<&str>) -> Option<&Rule> {
        let bucket = match listen_addr {
            Some(addr) => self.listen_rules.get(addr)?,
            None => &self.default_rules,
        };
        let host = request
            .headers()
            .get("Host")
            .and_then(|value| value.to_str().ok())
            .map(|host| host.to_ascii_lowercase());

        bucket
            .first_matching(&self.rules, request, host.as_deref())
            .map(|compiled| &compiled.rule)
    }

    fn build_index(rules: &[CompiledRule]) -> (RuleBucket, HashMap<String, RuleBucket>) {
        let mut default_rules = RuleBucket::default();
        let mut listen_rules = HashMap::new();

        for (idx, compiled) in rules.iter().enumerate() {
            let bucket = match compiled.rule.listen.as_deref() {
                Some(listen) => listen_rules.entry(listen.to_string()).or_default(),
                None => &mut default_rules,
            };
            bucket.insert(idx, compiled.exact_host());
        }

        (default_rules, listen_rules)
    }

    /// Check if a single rule matches the request.
    fn rule_matches(rule: &CompiledRule, request: &Request<()>) -> bool {
        match &rule.conditions {
            None => true,
            Some(expr) => {
                let ctx = EvalContext {
                    request,
                    jwt_payload: OnceCell::new(),
                };
                Self::eval_expr(expr, &ctx)
            }
        }
    }

    /// Recursively evaluate a condition expression.
    fn eval_expr(expr: &CompiledExpr, ctx: &EvalContext<'_>) -> bool {
        match expr {
            CompiledExpr::And { children } => children.iter().all(|c| Self::eval_expr(c, ctx)),
            CompiledExpr::Or { children } => children.iter().any(|c| Self::eval_expr(c, ctx)),
            CompiledExpr::Leaf(leaf) => Self::eval_leaf(leaf, ctx),
        }
    }

    /// Evaluate a single leaf condition against the request.
    fn eval_leaf(leaf: &CompiledLeaf, ctx: &EvalContext<'_>) -> bool {
        let headers = ctx.request.headers();

        match leaf.condition_type {
            ConditionType::Host => {
                let host = headers
                    .get("Host")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                match_text(
                    host,
                    &leaf.operator,
                    leaf.value.as_deref(),
                    leaf.regex.as_ref(),
                    true,
                )
            }
            ConditionType::Path => {
                let path = ctx.request.uri().path();
                match_text(
                    path,
                    &leaf.operator,
                    leaf.value.as_deref(),
                    leaf.regex.as_ref(),
                    false,
                )
            }
            ConditionType::Header => {
                let Some(key) = leaf.key.as_deref() else {
                    return false;
                };
                match_header(headers, key, leaf)
            }
            ConditionType::Cookie => {
                let Some(key) = leaf.key.as_deref() else {
                    return false;
                };
                let cookie_header = headers
                    .get("Cookie")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                match_cookie(cookie_header, key, leaf)
            }
            ConditionType::Jwt => {
                let payload = ctx.jwt_payload.get_or_init(|| {
                    Self::extract_jwt_token(ctx.request).and_then(decode_jwt_payload)
                });
                payload
                    .as_ref()
                    .is_some_and(|payload| match_jwt(payload, &leaf.claim_path, leaf))
            }
        }
    }

    /// Extract JWT token from the Authorization header (Bearer token).
    fn extract_jwt_token(request: &Request<()>) -> Option<&str> {
        let auth_header = request.headers().get("Authorization")?;
        let auth_str = auth_header.to_str().ok()?;
        auth_str.strip_prefix("Bearer ")
    }
}

impl CompiledRule {
    fn exact_host(&self) -> Option<&str> {
        self.conditions.as_ref().and_then(CompiledExpr::exact_host)
    }
}

impl RuleBucket {
    fn insert(&mut self, idx: usize, exact_host: Option<&str>) {
        match exact_host {
            Some(host) => self
                .host_exact
                .entry(host.to_ascii_lowercase())
                .or_default()
                .push(idx),
            None => self.general.push(idx),
        }
    }

    fn first_matching<'a>(
        &self,
        rules: &'a [CompiledRule],
        request: &Request<()>,
        host: Option<&str>,
    ) -> Option<&'a CompiledRule> {
        let host_rules = host.and_then(|host| self.host_exact.get(host));
        let mut general_idx = 0;
        let mut host_idx = 0;

        loop {
            let general_rule_idx = self.general.get(general_idx).copied();
            let host_rule_idx = host_rules.and_then(|rules| rules.get(host_idx).copied());

            let next_rule_idx = match (general_rule_idx, host_rule_idx) {
                (Some(general_rule_idx), Some(host_rule_idx)) => {
                    if general_rule_idx < host_rule_idx {
                        general_idx += 1;
                        general_rule_idx
                    } else {
                        host_idx += 1;
                        host_rule_idx
                    }
                }
                (Some(general_rule_idx), None) => {
                    general_idx += 1;
                    general_rule_idx
                }
                (None, Some(host_rule_idx)) => {
                    host_idx += 1;
                    host_rule_idx
                }
                (None, None) => return None,
            };

            let compiled = &rules[next_rule_idx];
            if Matcher::rule_matches(compiled, request) {
                return Some(compiled);
            }
        }
    }
}

impl CompiledExpr {
    fn from_expr(expr: &ConditionExpr) -> Self {
        match expr {
            ConditionExpr::And { children } => Self::And {
                children: children.iter().map(Self::from_expr).collect(),
            },
            ConditionExpr::Or { children } => Self::Or {
                children: children.iter().map(Self::from_expr).collect(),
            },
            ConditionExpr::Leaf {
                condition_type,
                operator,
                value,
                key,
                claim_path,
            } => Self::Leaf(CompiledLeaf::new(
                condition_type.clone(),
                operator.clone(),
                value.clone(),
                key.clone(),
                claim_path.clone(),
            )),
        }
    }

    fn exact_host(&self) -> Option<&str> {
        match self {
            CompiledExpr::And { children } => children.iter().find_map(Self::exact_host),
            CompiledExpr::Or { .. } => None,
            CompiledExpr::Leaf(leaf) => {
                if leaf.condition_type == ConditionType::Host && leaf.operator == Operator::Exact {
                    leaf.value.as_deref()
                } else {
                    None
                }
            }
        }
    }
}

impl CompiledLeaf {
    fn new(
        condition_type: ConditionType,
        operator: Operator,
        value: Option<String>,
        key: Option<String>,
        claim_path: Option<String>,
    ) -> Self {
        let regex = if operator == Operator::Regex {
            value
                .as_deref()
                .and_then(|pattern| Regex::new(pattern).ok())
        } else {
            None
        };
        let claim_path = claim_path
            .as_deref()
            .map(|path| path.split('.').map(str::to_string).collect())
            .unwrap_or_default();

        Self {
            condition_type,
            operator,
            value,
            key,
            claim_path,
            regex,
        }
    }
}

fn match_header(headers: &HeaderMap, key: &str, leaf: &CompiledLeaf) -> bool {
    match leaf.operator {
        Operator::Exists => headers.contains_key(key),
        _ => headers
            .get(key)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|actual| {
                match_text(
                    actual,
                    &leaf.operator,
                    leaf.value.as_deref(),
                    leaf.regex.as_ref(),
                    false,
                )
            }),
    }
}

fn match_cookie(cookie_header: &str, key: &str, leaf: &CompiledLeaf) -> bool {
    find_cookie_value(cookie_header, key).is_some_and(|actual| match leaf.operator {
        Operator::Exists => true,
        _ => match_text(
            actual,
            &leaf.operator,
            leaf.value.as_deref(),
            leaf.regex.as_ref(),
            false,
        ),
    })
}

fn find_cookie_value<'a>(cookie_header: &'a str, key: &str) -> Option<&'a str> {
    for pair in cookie_header.split(';') {
        let pair = pair.trim();
        let Some(idx) = pair.find('=') else {
            continue;
        };
        if pair[..idx].trim() == key {
            return Some(pair[idx + 1..].trim());
        }
    }
    None
}

fn match_text(
    actual: &str,
    operator: &Operator,
    value: Option<&str>,
    regex: Option<&Regex>,
    ignore_ascii_case: bool,
) -> bool {
    match operator {
        Operator::Exists => !actual.is_empty(),
        Operator::Exact => {
            let Some(expected) = value else {
                return false;
            };
            if ignore_ascii_case {
                actual.eq_ignore_ascii_case(expected)
            } else {
                actual == expected
            }
        }
        Operator::Prefix => {
            let Some(expected) = value else {
                return false;
            };
            if ignore_ascii_case {
                starts_with_ignore_ascii_case(actual, expected)
            } else {
                actual.starts_with(expected)
            }
        }
        Operator::Contains => {
            let Some(expected) = value else {
                return false;
            };
            if ignore_ascii_case {
                contains_ignore_ascii_case(actual, expected)
            } else {
                actual.contains(expected)
            }
        }
        Operator::Regex => regex.is_some_and(|regex| regex.is_match(actual)),
    }
}

fn starts_with_ignore_ascii_case(actual: &str, expected: &str) -> bool {
    actual
        .get(..expected.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(expected))
}

fn contains_ignore_ascii_case(actual: &str, expected: &str) -> bool {
    if expected.is_empty() {
        return true;
    }

    actual
        .as_bytes()
        .windows(expected.len())
        .any(|window| window.eq_ignore_ascii_case(expected.as_bytes()))
}

fn decode_jwt_payload(token: &str) -> Option<Value> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

    let mut parts = token.splitn(3, '.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let _signature = parts.next()?;

    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&decoded).ok()
}

fn match_jwt(payload: &Value, claim_path: &[String], leaf: &CompiledLeaf) -> bool {
    let Some(claim) = navigate_path(payload, claim_path) else {
        return false;
    };

    match leaf.operator {
        Operator::Exists => true,
        _ => value_to_string(claim).is_some_and(|actual| {
            match_text(
                &actual,
                &leaf.operator,
                leaf.value.as_deref(),
                leaf.regex.as_ref(),
                false,
            )
        }),
    }
}

fn navigate_path<'a>(value: &'a Value, path: &[String]) -> Option<&'a Value> {
    let mut current = value;

    for part in path {
        current = match current {
            Value::Object(map) => map.get(part)?,
            Value::Array(arr) => arr.get(part.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }

    Some(current)
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null => Some("null".to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::Matcher;
    use crate::models::rule::{ConditionExpr, ConditionType, Operator, Rule};
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

    fn create_rule(priority: i32, conditions: Option<ConditionExpr>) -> Rule {
        Rule {
            id: "test-rule".to_string(),
            name: "Test Rule".to_string(),
            priority,
            conditions,
            upstream: "backend-1".to_string(),
            weight: 100,
            listen: None,
            tls: None,
        }
    }

    fn create_header_leaf(key: &str, operator: Operator, value: Option<&str>) -> ConditionExpr {
        ConditionExpr::Leaf {
            condition_type: ConditionType::Header,
            key: Some(key.to_string()),
            claim_path: None,
            operator,
            value: value.map(|s| s.to_string()),
        }
    }

    fn create_cookie_leaf(key: &str, operator: Operator, value: Option<&str>) -> ConditionExpr {
        ConditionExpr::Leaf {
            condition_type: ConditionType::Cookie,
            key: Some(key.to_string()),
            claim_path: None,
            operator,
            value: value.map(|s| s.to_string()),
        }
    }

    fn create_jwt_leaf(claim_path: &str, operator: Operator, value: Option<&str>) -> ConditionExpr {
        ConditionExpr::Leaf {
            condition_type: ConditionType::Jwt,
            key: None,
            claim_path: Some(claim_path.to_string()),
            operator,
            value: value.map(|s| s.to_string()),
        }
    }

    fn and(children: Vec<ConditionExpr>) -> Option<ConditionExpr> {
        Some(ConditionExpr::And { children })
    }

    fn or(children: Vec<ConditionExpr>) -> Option<ConditionExpr> {
        Some(ConditionExpr::Or { children })
    }

    fn leaf(expr: ConditionExpr) -> Option<ConditionExpr> {
        Some(expr)
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
            create_rule(10, None),
            create_rule(50, None),
            create_rule(30, None),
        ];
        let matcher = Matcher::new(rules);
        let request = create_request_with_headers(&[]);

        // Highest priority rule should match first (rule with priority 50)
        let result = matcher.match_request(&request, None);
        assert!(result.is_some());
        assert_eq!(result.unwrap().priority, 50);
    }

    #[test]
    fn test_equal_priority_preserves_input_order() {
        let mut first = create_rule(10, None);
        first.id = "first".to_string();
        let mut second = create_rule(10, None);
        second.id = "second".to_string();
        let matcher = Matcher::new(vec![first, second]);
        let request = create_request_with_headers(&[]);
        let result = matcher.match_request(&request, None).unwrap();
        assert_eq!(result.id, "first");
    }

    #[test]
    fn test_and_logic_all_conditions_must_match() {
        let rule = create_rule(
            10,
            and(vec![
                create_header_leaf("Host", Operator::Exact, Some("example.com")),
                create_header_leaf("Content-Type", Operator::Exact, Some("application/json")),
            ]),
        );

        let matcher = Matcher::new(vec![rule]);
        let request = create_request_with_headers(&[
            ("Host", "example.com"),
            ("Content-Type", "application/json"),
        ]);

        let result = matcher.match_request(&request, None);
        assert!(result.is_some());

        // Only one condition matches - should not match
        let request_partial =
            create_request_with_headers(&[("Host", "example.com"), ("Content-Type", "text/html")]);
        let result_partial = matcher.match_request(&request_partial, None);
        assert!(result_partial.is_none());
    }

    #[test]
    fn test_or_logic_any_condition_matches() {
        let rule = create_rule(
            10,
            or(vec![
                create_header_leaf("Host", Operator::Exact, Some("a.com")),
                create_header_leaf("Host", Operator::Exact, Some("b.com")),
            ]),
        );

        let matcher = Matcher::new(vec![rule]);

        // First matches
        let req_a = create_request_with_headers(&[("Host", "a.com")]);
        assert!(matcher.match_request(&req_a, None).is_some());

        // Second matches
        let req_b = create_request_with_headers(&[("Host", "b.com")]);
        assert!(matcher.match_request(&req_b, None).is_some());

        // Neither matches
        let req_c = create_request_with_headers(&[("Host", "c.com")]);
        assert!(matcher.match_request(&req_c, None).is_none());
    }

    #[test]
    fn test_nested_and_or() {
        // (Host=a.com AND Path=/api) OR (Host=b.com AND Path=/web)
        let rule = create_rule(
            10,
            or(vec![
                ConditionExpr::And {
                    children: vec![
                        create_header_leaf("Host", Operator::Exact, Some("a.com")),
                        ConditionExpr::Leaf {
                            condition_type: ConditionType::Path,
                            key: None,
                            claim_path: None,
                            operator: Operator::Prefix,
                            value: Some("/api".to_string()),
                        },
                    ],
                },
                ConditionExpr::And {
                    children: vec![
                        create_header_leaf("Host", Operator::Exact, Some("b.com")),
                        ConditionExpr::Leaf {
                            condition_type: ConditionType::Path,
                            key: None,
                            claim_path: None,
                            operator: Operator::Prefix,
                            value: Some("/web".to_string()),
                        },
                    ],
                },
            ]),
        );

        let matcher = Matcher::new(vec![rule]);

        // Host=a.com, Path=/api/users -> matches first AND group
        let req1 = Request::builder()
            .uri("/api/users")
            .header("Host", "a.com")
            .body(())
            .unwrap();
        assert!(matcher.match_request(&req1, None).is_some());

        // Host=b.com, Path=/web/page -> matches second AND group
        let req2 = Request::builder()
            .uri("/web/page")
            .header("Host", "b.com")
            .body(())
            .unwrap();
        assert!(matcher.match_request(&req2, None).is_some());

        // Host=a.com, Path=/web -> no match (first AND needs /api, second needs Host=b.com)
        let req3 = Request::builder()
            .uri("/web")
            .header("Host", "a.com")
            .body(())
            .unwrap();
        assert!(matcher.match_request(&req3, None).is_none());
    }

    #[test]
    fn test_returns_first_matching_rule() {
        let rule1 = create_rule(
            100,
            leaf(create_header_leaf(
                "Host",
                Operator::Exact,
                Some("example.com"),
            )),
        );
        let rule2 = create_rule(
            50,
            leaf(create_header_leaf(
                "Host",
                Operator::Exact,
                Some("example.com"),
            )),
        );

        let matcher = Matcher::new(vec![rule2, rule1]); // Inserted in reverse order
        let request = create_request_with_headers(&[("Host", "example.com")]);

        let result = matcher.match_request(&request, None);
        assert!(result.is_some());
        assert_eq!(result.unwrap().priority, 100);
    }

    #[test]
    fn test_returns_none_when_no_rule_matches() {
        let rule = create_rule(
            10,
            leaf(create_header_leaf(
                "Host",
                Operator::Exact,
                Some("example.com"),
            )),
        );

        let matcher = Matcher::new(vec![rule]);
        let request = create_request_with_headers(&[("Host", "other.com")]);

        let result = matcher.match_request(&request, None);
        assert!(result.is_none());
    }

    #[test]
    fn test_header_condition_matching() {
        let rule = create_rule(
            10,
            leaf(create_header_leaf("X-API-Key", Operator::Exists, None)),
        );

        let matcher = Matcher::new(vec![rule]);

        let request_with_header = create_request_with_headers(&[("X-API-Key", "secret123")]);
        assert!(matcher.match_request(&request_with_header, None).is_some());

        let request_without_header = create_request_with_headers(&[("Host", "example.com")]);
        assert!(matcher
            .match_request(&request_without_header, None)
            .is_none());
    }

    #[test]
    fn test_cookie_condition_matching() {
        let rule = create_rule(
            10,
            leaf(create_cookie_leaf(
                "session",
                Operator::Exact,
                Some("abc123"),
            )),
        );

        let matcher = Matcher::new(vec![rule]);

        let request = create_request_with_headers(&[("Cookie", "session=abc123; theme=dark")]);
        assert!(matcher.match_request(&request, None).is_some());

        let request_diff = create_request_with_headers(&[("Cookie", "session=xyz789; theme=dark")]);
        assert!(matcher.match_request(&request_diff, None).is_none());

        let request_no_cookie = create_request_with_headers(&[("Host", "example.com")]);
        assert!(matcher.match_request(&request_no_cookie, None).is_none());
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
            leaf(create_jwt_leaf("sub", Operator::Exact, Some("user123"))),
        );

        let matcher = Matcher::new(vec![rule]);

        let mut request = create_request_with_headers(&[]);
        request.headers_mut().insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {}", token)).unwrap(),
        );
        assert!(matcher.match_request(&request, None).is_some());

        let rule_diff = create_rule(
            10,
            leaf(create_jwt_leaf("sub", Operator::Exact, Some("user456"))),
        );
        let matcher_diff = Matcher::new(vec![rule_diff]);
        assert!(matcher_diff.match_request(&request, None).is_none());
    }

    #[test]
    fn test_jwt_extracted_from_authorization_header() {
        let claims = serde_json::json!({
            "sub": "user123"
        });
        let token = create_test_jwt(claims);

        let rule = create_rule(10, leaf(create_jwt_leaf("sub", Operator::Exists, None)));

        let matcher = Matcher::new(vec![rule]);
        let mut request = create_request_with_headers(&[]);
        request.headers_mut().insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {}", token)).unwrap(),
        );

        assert!(matcher.match_request(&request, None).is_some());

        let mut request_basic = create_request_with_headers(&[]);
        request_basic.headers_mut().insert(
            "Authorization",
            HeaderValue::from_static("Basic dXNlcjpwYXNz"),
        );
        assert!(matcher.match_request(&request_basic, None).is_none());

        // Request with no Authorization header - should not match
        let request_no_auth = create_request_with_headers(&[]);
        assert!(matcher.match_request(&request_no_auth, None).is_none());
    }

    #[test]
    fn test_empty_rules_returns_none() {
        let matcher = Matcher::new(vec![]);
        let request = create_request_with_headers(&[("Host", "example.com")]);
        assert!(matcher.match_request(&request, None).is_none());
    }

    #[test]
    fn test_rule_with_no_conditions_always_matches() {
        let rule = create_rule(10, None);
        let matcher = Matcher::new(vec![rule]);
        let request = create_request_with_headers(&[("Host", "anything.com")]);
        assert!(matcher.match_request(&request, None).is_some());
    }

    #[test]
    fn test_multiple_rules_first_match_wins() {
        let rule1 = create_rule(
            100,
            leaf(create_header_leaf("Host", Operator::Exact, Some("a.com"))),
        );
        let rule2 = create_rule(
            90,
            leaf(create_header_leaf("Host", Operator::Exact, Some("b.com"))),
        );
        let rule3 = create_rule(
            80,
            leaf(create_header_leaf("Host", Operator::Exact, Some("c.com"))),
        );

        let matcher = Matcher::new(vec![rule1, rule2, rule3]);

        let request = create_request_with_headers(&[("Host", "c.com")]);
        let result = matcher.match_request(&request, None);
        assert!(result.is_some());
        assert_eq!(result.unwrap().priority, 80);
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
            and(vec![
                create_header_leaf("Host", Operator::Exact, Some("example.com")),
                create_cookie_leaf("session", Operator::Regex, Some(r"^abc[0-9]+$")),
                create_jwt_leaf("role", Operator::Exact, Some("admin")),
            ]),
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
        assert!(matcher.match_request(&request, None).is_some());

        // One condition fails (cookie regex)
        let mut request_fail = create_request_with_headers(&[
            ("Host", "example.com"),
            ("Cookie", "session=xyz; theme=dark"),
        ]);
        request_fail.headers_mut().insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {}", token)).unwrap(),
        );
        assert!(matcher.match_request(&request_fail, None).is_none());
    }
}
