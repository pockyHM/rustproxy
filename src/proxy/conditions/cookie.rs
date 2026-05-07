use regex::Regex;

use crate::models::rule::Operator;

/// Parse a Cookie header string into key=value pairs.
fn parse_cookies(cookie_header: &str) -> Vec<(String, String)> {
    cookie_header
        .split(';')
        .filter_map(|pair| {
            let pair = pair.trim();
            let idx = pair.find('=')?;
            let key = pair[..idx].trim().to_string();
            let value = pair[idx + 1..].trim().to_string();
            Some((key, value))
        })
        .collect()
}

/// Match a cookie condition against a Cookie header value.
///
/// # Arguments
/// * `cookie_header` - The raw Cookie header value (e.g., "session=abc123; theme=dark")
/// * `key` - The cookie name to match
/// * `operator` - The matching operator (Exists, Exact, Regex, Contains)
/// * `value` - The value to match against (not required for Exists)
///
/// # Returns
/// true if the condition matches, false otherwise
pub fn match_cookie(
    cookie_header: &str,
    key: &str,
    operator: &Operator,
    value: Option<&str>,
) -> bool {
    let cookies = parse_cookies(cookie_header);

    match operator {
        Operator::Exists => cookies.iter().any(|(k, _)| k == key),
        Operator::Exact => {
            cookies
                .iter()
                .any(|(k, v)| k == key && v == value.unwrap_or(""))
        }
        Operator::Regex => {
            if let Some(pattern) = value {
                if let Ok(re) = Regex::new(pattern) {
                    return cookies.iter().any(|(k, v)| k == key && re.is_match(v));
                }
            }
            false
        }
        Operator::Contains => {
            if let Some(substr) = value {
                return cookies.iter().any(|(k, v)| k == key && v.contains(substr));
            }
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cookie_exists_true() {
        let cookie = "session=abc123; theme=dark";
        assert!(match_cookie(cookie, "session", &Operator::Exists, None));
    }

    #[test]
    fn test_cookie_exists_false() {
        let cookie = "session=abc123; theme=dark";
        assert!(!match_cookie(cookie, "auth", &Operator::Exists, None));
    }

    #[test]
    fn test_cookie_exists_empty() {
        assert!(!match_cookie("", "session", &Operator::Exists, None));
    }

    #[test]
    fn test_cookie_exact_match() {
        let cookie = "session=abc123";
        assert!(match_cookie(cookie, "session", &Operator::Exact, Some("abc123")));
    }

    #[test]
    fn test_cookie_exact_no_match() {
        let cookie = "session=abc123";
        assert!(!match_cookie(cookie, "session", &Operator::Exact, Some("def456")));
    }

    #[test]
    fn test_cookie_exact_missing_cookie() {
        let cookie = "session=abc123";
        assert!(!match_cookie(cookie, "theme", &Operator::Exact, Some("dark")));
    }

    #[test]
    fn test_cookie_regex_match() {
        let cookie = "session=abc123";
        assert!(match_cookie(cookie, "session", &Operator::Regex, Some(r"abc\d+")));
    }

    #[test]
    fn test_cookie_regex_no_match() {
        let cookie = "session=abc123";
        assert!(!match_cookie(cookie, "session", &Operator::Regex, Some(r"def\d+")));
    }

    #[test]
    fn test_cookie_regex_invalid_pattern() {
        let cookie = "session=abc123";
        // Invalid regex should not panic, just return false
        assert!(!match_cookie(cookie, "session", &Operator::Regex, Some(r"[invalid")));
    }

    #[test]
    fn test_cookie_contains_match() {
        let cookie = "session=abc123xyz";
        assert!(match_cookie(cookie, "session", &Operator::Contains, Some("123")));
    }

    #[test]
    fn test_cookie_contains_no_match() {
        let cookie = "session=abc123xyz";
        assert!(!match_cookie(cookie, "session", &Operator::Contains, Some("456")));
    }

    #[test]
    fn test_cookie_parse_multiple() {
        let cookie = "session=abc123; theme=dark; lang=en";
        let parsed = parse_cookies(cookie);
        assert_eq!(parsed.len(), 3);
        assert!(parsed.contains(&("session".to_string(), "abc123".to_string())));
        assert!(parsed.contains(&("theme".to_string(), "dark".to_string())));
        assert!(parsed.contains(&("lang".to_string(), "en".to_string())));
    }

    #[test]
    fn test_cookie_parse_with_spaces() {
        let cookie = "session=abc123; theme=dark";
        let parsed = parse_cookies(cookie);
        assert_eq!(parsed.len(), 2);
    }

    #[test]
    fn test_cookie_parse_empty_value() {
        let cookie = "session=; theme=dark";
        let parsed = parse_cookies(cookie);
        assert!(parsed.contains(&("session".to_string(), "".to_string())));
    }

    #[test]
    fn test_cookie_parse_no_value() {
        let cookie = "session";
        let parsed = parse_cookies(cookie);
        // No '=' found, should skip
        assert!(parsed.is_empty());
    }
}
