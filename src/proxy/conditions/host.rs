use crate::models::rule::Operator;

pub fn match_host(host: &str, operator: &Operator, value: Option<&str>) -> bool {
    let Some(expected) = value else {
        return operator == &Operator::Exists;
    };

    match operator {
        Operator::Exact => host.eq_ignore_ascii_case(expected),
        Operator::Contains => host.to_lowercase().contains(&expected.to_lowercase()),
        Operator::Regex => {
            let Ok(re) = regex::Regex::new(expected) else {
                return false;
            };
            re.is_match(host)
        }
        Operator::Exists => !host.is_empty(),
        Operator::Prefix => host.to_lowercase().starts_with(&expected.to_lowercase()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_exact() {
        assert!(match_host(
            "example.com",
            &Operator::Exact,
            Some("example.com")
        ));
        assert!(match_host(
            "Example.COM",
            &Operator::Exact,
            Some("example.com")
        ));
        assert!(!match_host(
            "other.com",
            &Operator::Exact,
            Some("example.com")
        ));
    }

    #[test]
    fn test_host_contains() {
        assert!(match_host(
            "api.example.com",
            &Operator::Contains,
            Some("example")
        ));
        assert!(!match_host(
            "api.other.com",
            &Operator::Contains,
            Some("example")
        ));
    }

    #[test]
    fn test_host_prefix() {
        assert!(match_host(
            "api.example.com",
            &Operator::Prefix,
            Some("api.")
        ));
        assert!(!match_host(
            "web.example.com",
            &Operator::Prefix,
            Some("api.")
        ));
    }
}
