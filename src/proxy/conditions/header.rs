use http::HeaderMap;
use regex::Regex;

use crate::models::rule::Operator;

/// Match a header condition against request headers.
///
/// # Arguments
/// * `request_headers` - The HTTP request headers
/// * `key` - The header name to match
/// * `operator` - The matching operator (Exists, Exact, Regex, Contains)
/// * `value` - The value to match against (not required for Exists)
///
/// # Returns
/// true if the condition matches, false otherwise
pub fn match_header(
    request_headers: &HeaderMap,
    key: &str,
    operator: &Operator,
    value: Option<&str>,
) -> bool {
    match operator {
        Operator::Exists => request_headers.contains_key(key),
        Operator::Exact => {
            if let Some(header_value) = request_headers.get(key) {
                if let (Some(expected), Ok(actual)) = (value, header_value.to_str()) {
                    return actual == expected;
                }
            }
            false
        }
        Operator::Regex => {
            if let Some(header_value) = request_headers.get(key) {
                if let (Some(pattern), Ok(actual)) = (value, header_value.to_str()) {
                    if let Ok(re) = Regex::new(pattern) {
                        return re.is_match(actual);
                    }
                }
            }
            false
        }
        Operator::Contains => {
            if let Some(header_value) = request_headers.get(key) {
                if let (Some(substr), Ok(actual)) = (value, header_value.to_str()) {
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
    use http::HeaderValue;

    fn create_header_map(headers: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (key, value) in headers {
            map.insert(
                (*key).parse::<http::HeaderName>().unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        map
    }

    #[test]
    fn test_header_exists_true() {
        let headers = create_header_map(&[("Content-Type", "application/json")]);
        assert!(match_header(
            &headers,
            "Content-Type",
            &Operator::Exists,
            None
        ));
    }

    #[test]
    fn test_header_exists_false() {
        let headers = create_header_map(&[("Content-Type", "application/json")]);
        assert!(!match_header(
            &headers,
            "X-Custom-Header",
            &Operator::Exists,
            None
        ));
    }

    #[test]
    fn test_header_exact_match() {
        let headers = create_header_map(&[("Content-Type", "application/json")]);
        assert!(match_header(
            &headers,
            "Content-Type",
            &Operator::Exact,
            Some("application/json")
        ));
    }

    #[test]
    fn test_header_exact_no_match() {
        let headers = create_header_map(&[("Content-Type", "application/json")]);
        assert!(!match_header(
            &headers,
            "Content-Type",
            &Operator::Exact,
            Some("text/html")
        ));
    }

    #[test]
    fn test_header_exact_missing_header() {
        let headers = create_header_map(&[("Content-Type", "application/json")]);
        assert!(!match_header(
            &headers,
            "X-Custom-Header",
            &Operator::Exact,
            Some("anything")
        ));
    }

    #[test]
    fn test_header_regex_match() {
        let headers = create_header_map(&[("Content-Type", "application/json")]);
        assert!(match_header(
            &headers,
            "Content-Type",
            &Operator::Regex,
            Some(r"application/.*")
        ));
    }

    #[test]
    fn test_header_regex_no_match() {
        let headers = create_header_map(&[("Content-Type", "application/json")]);
        assert!(!match_header(
            &headers,
            "Content-Type",
            &Operator::Regex,
            Some(r"text/.*")
        ));
    }

    #[test]
    fn test_header_regex_invalid_pattern() {
        let headers = create_header_map(&[("Content-Type", "application/json")]);
        // Invalid regex should not panic, just return false
        assert!(!match_header(
            &headers,
            "Content-Type",
            &Operator::Regex,
            Some(r"[invalid")
        ));
    }

    #[test]
    fn test_header_contains_match() {
        let headers = create_header_map(&[("Content-Type", "application/json")]);
        assert!(match_header(
            &headers,
            "Content-Type",
            &Operator::Contains,
            Some("json")
        ));
    }

    #[test]
    fn test_header_contains_no_match() {
        let headers = create_header_map(&[("Content-Type", "application/json")]);
        assert!(!match_header(
            &headers,
            "Content-Type",
            &Operator::Contains,
            Some("xml")
        ));
    }

    #[test]
    fn test_header_multiple_values() {
        let mut headers = HeaderMap::new();
        headers.append("Set-Cookie", "cookie1=value1".parse().unwrap());
        headers.append("Set-Cookie", "cookie2=value2".parse().unwrap());

        // Should find at least one matching header for Exists
        assert!(match_header(
            &headers,
            "Set-Cookie",
            &Operator::Exists,
            None
        ));
    }
}
