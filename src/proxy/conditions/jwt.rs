use jsonwebtoken::decode;
use regex::Regex;
use serde_json::Value;

use crate::models::rule::Operator;

use std::collections::HashMap;

/// Decode a JWT token and extract the payload (without signature verification).
fn decode_jwt_payload(token: &str) -> Option<Value> {
    // Use decode without signature verification (trust the terminating proxy)
    // The decode function parses the JWT and returns the claims
    let mut parts = token.splitn(3, '.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let _signature = parts.next()?;

    // Decode the payload (base64url)
    let decoded = base64url_decode(payload).ok()?;
    let json: Value = serde_json::from_slice(&decoded).ok()?;
    Some(json)
}

/// Decode base64url encoded string.
fn base64url_decode(input: &str) -> Result<Vec<u8>, base64::DecodeError> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    URL_SAFE_NO_PAD.decode(input)
}

/// Navigate a JSON value through a dot-separated path (e.g., "user.metadata.tenant_id").
fn navigate_path(value: &Value, path: &str) -> Option<&Value> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current: &Value = value;

    for part in parts {
        current = match current {
            Value::Object(map) => map.get(part)?,
            Value::Array(arr) => {
                // If part is a numeric index, try to parse it
                if let Ok(idx) = part.parse::<usize>() {
                    arr.get(idx)?
                } else {
                    return None;
                }
            }
            _ => return None,
        };
    }

    Some(current)
}

/// Convert a JSON value to a string for comparison.
fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null => Some("null".to_string()),
        _ => None,
    }
}

/// Match a JWT condition against a JWT token.
///
/// # Arguments
/// * `jwt_token` - The JWT token string
/// * `claim_path` - The path to the claim to match (e.g., "sub", "user.metadata.tenant_id")
/// * `operator` - The matching operator (Exists, Exact, Regex, Contains)
/// * `value` - The value to match against (not required for Exists)
///
/// # Returns
/// true if the condition matches, false otherwise
pub fn match_jwt(
    jwt_token: &str,
    claim_path: &str,
    operator: &Operator,
    value: Option<&str>,
) -> bool {
    let payload = match decode_jwt_payload(jwt_token) {
        Some(p) => p,
        None => return false,
    };

    match operator {
        Operator::Exists => navigate_path(&payload, claim_path).is_some(),
        Operator::Exact => {
            if let Some(claim) = navigate_path(&payload, claim_path) {
                if let (Some(expected), Some(actual)) = (value, value_to_string(claim)) {
                    return actual == expected;
                }
            }
            false
        }
        Operator::Regex => {
            if let Some(claim) = navigate_path(&payload, claim_path) {
                if let (Some(pattern), Some(actual)) = (value, value_to_string(claim)) {
                    if let Ok(re) = Regex::new(pattern) {
                        return re.is_match(&actual);
                    }
                }
            }
            false
        }
        Operator::Contains => {
            if let Some(claim) = navigate_path(&payload, claim_path) {
                if let (Some(substr), Some(actual)) = (value, value_to_string(claim)) {
                    return actual.contains(substr);
                }
            }
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create a test JWT (header.payload.signature with base64url encoding)
    fn create_test_jwt(claims: Value) -> String {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

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
    fn test_jwt_decode_payload() {
        let claims = serde_json::json!({
            "sub": "user123",
            "role": "admin"
        });
        let token = create_test_jwt(claims);
        let payload = decode_jwt_payload(&token).unwrap();
        assert_eq!(payload["sub"], "user123");
        assert_eq!(payload["role"], "admin");
    }

    #[test]
    fn test_jwt_decode_invalid_token() {
        assert!(decode_jwt_payload("invalid.token").is_none());
        assert!(decode_jwt_payload("").is_none());
        assert!(decode_jwt_payload("onlyone").is_none());
    }

    #[test]
    fn test_jwt_navigate_simple_path() {
        let claims = serde_json::json!({
            "sub": "user123",
            "role": "admin"
        });
        let token = create_test_jwt(claims);
        let payload = decode_jwt_payload(&token).unwrap();

        assert_eq!(navigate_path(&payload, "sub").unwrap(), &serde_json::json!("user123"));
        assert_eq!(navigate_path(&payload, "role").unwrap(), &serde_json::json!("admin"));
    }

    #[test]
    fn test_jwt_navigate_nested_path() {
        let claims = serde_json::json!({
            "user": {
                "metadata": {
                    "tenant_id": "tenant-abc"
                }
            }
        });
        let token = create_test_jwt(claims);
        let payload = decode_jwt_payload(&token).unwrap();

        assert_eq!(
            navigate_path(&payload, "user.metadata.tenant_id").unwrap(),
            &serde_json::json!("tenant-abc")
        );
    }

    #[test]
    fn test_jwt_navigate_array_index() {
        let claims = serde_json::json!({
            "roles": ["admin", "user", "guest"]
        });
        let token = create_test_jwt(claims);
        let payload = decode_jwt_payload(&token).unwrap();

        assert_eq!(navigate_path(&payload, "roles.0").unwrap(), &serde_json::json!("admin"));
        assert_eq!(navigate_path(&payload, "roles.1").unwrap(), &serde_json::json!("user"));
    }

    #[test]
    fn test_jwt_navigate_invalid_path() {
        let claims = serde_json::json!({
            "sub": "user123"
        });
        let token = create_test_jwt(claims);
        let payload = decode_jwt_payload(&token).unwrap();

        assert!(navigate_path(&payload, "nonexistent").is_none());
        assert!(navigate_path(&payload, "user.name").is_none()); // user is string, not object
    }

    #[test]
    fn test_jwt_exists_true() {
        let claims = serde_json::json!({
            "sub": "user123",
            "role": "admin"
        });
        let token = create_test_jwt(claims);
        assert!(match_jwt(&token, "sub", &Operator::Exists, None));
        assert!(match_jwt(&token, "role", &Operator::Exists, None));
    }

    #[test]
    fn test_jwt_exists_false() {
        let claims = serde_json::json!({
            "sub": "user123"
        });
        let token = create_test_jwt(claims);
        assert!(!match_jwt(&token, "nonexistent", &Operator::Exists, None));
    }

    #[test]
    fn test_jwt_exact_match() {
        let claims = serde_json::json!({
            "sub": "user123"
        });
        let token = create_test_jwt(claims);
        assert!(match_jwt(&token, "sub", &Operator::Exact, Some("user123")));
        assert!(!match_jwt(&token, "sub", &Operator::Exact, Some("user456")));
    }

    #[test]
    fn test_jwt_exact_numeric() {
        let claims = serde_json::json!({
            "age": 25
        });
        let token = create_test_jwt(claims);
        // Number is converted to string for comparison
        assert!(match_jwt(&token, "age", &Operator::Exact, Some("25")));
        assert!(!match_jwt(&token, "age", &Operator::Exact, Some("30")));
    }

    #[test]
    fn test_jwt_regex_match() {
        let claims = serde_json::json!({
            "email": "user@example.com"
        });
        let token = create_test_jwt(claims);
        assert!(match_jwt(&token, "email", &Operator::Regex, Some(r".*@example\.com")));
        assert!(!match_jwt(&token, "email", &Operator::Regex, Some(r".*@other\.com")));
    }

    #[test]
    fn test_jwt_regex_invalid_pattern() {
        let claims = serde_json::json!({
            "email": "user@example.com"
        });
        let token = create_test_jwt(claims);
        assert!(!match_jwt(&token, "email", &Operator::Regex, Some(r"[invalid")));
    }

    #[test]
    fn test_jwt_contains_match() {
        let claims = serde_json::json!({
            "email": "user@example.com"
        });
        let token = create_test_jwt(claims);
        assert!(match_jwt(&token, "email", &Operator::Contains, Some("@example")));
        assert!(!match_jwt(&token, "email", &Operator::Contains, Some("@other")));
    }

    #[test]
    fn test_jwt_contains_nested() {
        let claims = serde_json::json!({
            "user": {
                "name": "John Doe"
            }
        });
        let token = create_test_jwt(claims);
        assert!(match_jwt(&token, "user.name", &Operator::Contains, Some("John")));
        assert!(!match_jwt(&token, "user.name", &Operator::Contains, Some("Jane")));
    }

    #[test]
    fn test_jwt_complex_nested_claims() {
        let claims = serde_json::json!({
            "user": {
                "metadata": {
                    "tenant_id": "tenant-abc",
                    "roles": ["admin", "user"]
                }
            }
        });
        let token = create_test_jwt(claims);
        assert!(match_jwt(&token, "user.metadata.tenant_id", &Operator::Exact, Some("tenant-abc")));
        assert!(match_jwt(&token, "user.metadata.tenant_id", &Operator::Regex, Some(r"tenant-.*")));
        assert!(match_jwt(&token, "user.metadata.tenant_id", &Operator::Contains, Some("abc")));
    }
}
