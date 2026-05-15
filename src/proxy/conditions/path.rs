use crate::models::rule::Operator;

pub fn match_path(request_path: &str, operator: &Operator, value: Option<&str>) -> bool {
    let Some(expected) = value else {
        return operator == &Operator::Exists;
    };

    match operator {
        Operator::Exact => request_path == expected,
        Operator::Prefix => request_path.starts_with(expected),
        Operator::Regex => {
            let Ok(re) = regex::Regex::new(expected) else {
                return false;
            };
            re.is_match(request_path)
        }
        Operator::Contains => request_path.contains(expected),
        Operator::Exists => !request_path.is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_exact() {
        assert!(match_path(
            "/api/users",
            &Operator::Exact,
            Some("/api/users")
        ));
        assert!(!match_path(
            "/api/users?page=1",
            &Operator::Exact,
            Some("/api/users")
        ));
        assert!(!match_path("/api", &Operator::Exact, Some("/api/users")));
    }

    #[test]
    fn test_path_prefix() {
        assert!(match_path("/api/users", &Operator::Prefix, Some("/api")));
        assert!(match_path("/api", &Operator::Prefix, Some("/api")));
        assert!(!match_path("/web/page", &Operator::Prefix, Some("/api")));
    }

    #[test]
    fn test_path_regex() {
        assert!(match_path(
            "/api/v1/users",
            &Operator::Regex,
            Some(r"^/api/v[0-9]+")
        ));
        assert!(!match_path(
            "/web/v1/users",
            &Operator::Regex,
            Some(r"^/api/v[0-9]+")
        ));
    }

    #[test]
    fn test_path_contains() {
        assert!(match_path("/api/v1/users", &Operator::Contains, Some("v1")));
        assert!(!match_path(
            "/api/v2/users",
            &Operator::Contains,
            Some("v1")
        ));
    }
}
