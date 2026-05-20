use http::StatusCode;

use crate::models::RetryPolicy;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptOutcome {
    Response(StatusCode),
    Timeout,
    ConnectError,
}

pub fn should_retry(policy: &RetryPolicy, attempt_index: u32, outcome: AttemptOutcome) -> bool {
    if attempt_index >= policy.attempts {
        return false;
    }

    match outcome {
        AttemptOutcome::Response(status) => policy.retry_on_status.contains(&status.as_u16()),
        AttemptOutcome::Timeout => policy.retry_on_timeout,
        AttemptOutcome::ConnectError => policy.retry_on_connect_error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{RetryPolicy, Target, Upstream};
    use crate::proxy::balancer::{BalanceContext, Balancer};
    use http::StatusCode;
    use std::collections::HashMap;

    #[test]
    fn retries_configured_status_codes_until_attempts_are_exhausted() {
        let policy = RetryPolicy {
            attempts: 2,
            retry_on_status: vec![StatusCode::BAD_GATEWAY.as_u16()],
            ..Default::default()
        };

        assert!(should_retry(
            &policy,
            0,
            AttemptOutcome::Response(StatusCode::BAD_GATEWAY)
        ));
        assert!(should_retry(
            &policy,
            1,
            AttemptOutcome::Response(StatusCode::BAD_GATEWAY)
        ));
        assert!(!should_retry(
            &policy,
            2,
            AttemptOutcome::Response(StatusCode::BAD_GATEWAY)
        ));
        assert!(!should_retry(
            &policy,
            0,
            AttemptOutcome::Response(StatusCode::SERVICE_UNAVAILABLE)
        ));
    }

    #[test]
    fn retries_timeout_when_enabled() {
        let policy = RetryPolicy {
            attempts: 1,
            retry_on_timeout: true,
            ..Default::default()
        };

        assert!(should_retry(&policy, 0, AttemptOutcome::Timeout));
        assert!(!should_retry(&policy, 1, AttemptOutcome::Timeout));
    }

    #[test]
    fn retries_connect_error_when_enabled() {
        let policy = RetryPolicy {
            attempts: 1,
            retry_on_connect_error: true,
            ..Default::default()
        };

        assert!(should_retry(&policy, 0, AttemptOutcome::ConnectError));
        assert!(!should_retry(&policy, 1, AttemptOutcome::ConnectError));
    }

    #[test]
    fn retry_after_502_can_select_second_target_and_succeed() {
        let policy = RetryPolicy {
            attempts: 1,
            retry_on_status: vec![StatusCode::BAD_GATEWAY.as_u16()],
            ..Default::default()
        };
        let upstream = Upstream {
            name: "backend".to_string(),
            skip_ssl: false,
            websocket: false,
            targets: vec![
                Target {
                    url: "http://first".to_string(),
                    weight: 1,
                },
                Target {
                    url: "http://second".to_string(),
                    weight: 1,
                },
            ],
            health_check: Default::default(),
            balance: Default::default(),
            retry: policy.clone(),
            sticky: Default::default(),
        };
        let mut upstreams = HashMap::new();
        upstreams.insert("backend".to_string(), upstream);
        let balancer = Balancer::new(upstreams);

        let first = balancer
            .select(
                "backend",
                BalanceContext {
                    client_ip: None,
                    path: "/",
                    sticky_key: None,
                },
            )
            .unwrap();
        assert_eq!(first.url, "http://first");
        assert!(should_retry(
            &policy,
            0,
            AttemptOutcome::Response(StatusCode::BAD_GATEWAY)
        ));

        let second = balancer
            .select_excluding(
                "backend",
                BalanceContext {
                    client_ip: None,
                    path: "/",
                    sticky_key: None,
                },
                Some(first.url.as_str()),
            )
            .unwrap();

        assert_eq!(second.url, "http://second");
        assert!(!should_retry(
            &policy,
            1,
            AttemptOutcome::Response(StatusCode::OK)
        ));
    }
}
