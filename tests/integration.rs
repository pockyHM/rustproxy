use std::collections::HashMap;

use http::Request;
use rustproxy::{
    config::yaml::{AppConfig, Fallback},
    models::{ConditionExpr, ConditionType, Operator, Rule, Target, Upstream},
    observability::metrics::ProxyMetrics,
    proxy::{
        balancer::{BalanceContext, Balancer},
        matcher::Matcher,
    },
};

fn build_config() -> AppConfig {
    let canary_upstream = Upstream {
        name: "canary".to_string(),
        skip_ssl: false,
        websocket: false,
        targets: vec![Target {
            url: "http://canary.internal:8080".to_string(),
            weight: 100,
            timeouts: Default::default(),
        }],
        health_check: Default::default(),
        balance: Default::default(),
        retry: Default::default(),
        timeouts: Default::default(),
    };

    let mut upstreams = HashMap::new();
    upstreams.insert(canary_upstream.name.clone(), canary_upstream);

    AppConfig {
        listen: "127.0.0.1:0".to_string(),
        proxy_listen: "0.0.0.0:80".to_string(),
        timeouts: Default::default(),
        limits: Default::default(),
        rules: vec![Rule {
            id: "canary-header".to_string(),
            name: "Route canary header".to_string(),
            priority: 100,
            host: Default::default(),
            location: Default::default(),
            match_set: None,
            conditions: Some(ConditionExpr::Leaf {
                condition_type: ConditionType::Header,
                key: Some("x-route".to_string()),
                claim_path: None,
                operator: Operator::Exact,
                value: Some("canary".to_string()),
            }),
            upstream: "canary".to_string(),
            weight: 100,
            is_fallback: false,
            listen: None,
            request_timeout: 0,
            timeouts: Default::default(),
            tls: None,
            header_policy: Default::default(),
            path_actions: Vec::new(),
            limit_policy: Default::default(),
        }],
        upstreams,
        fallback: Fallback {
            url: "http://fallback.internal:8080".to_string(),
        },
        connect_timeout: 10,
        request_timeout: 60,
        pool_max_idle_per_host: 32,
        pool_idle_timeout: 90,
        tcp_keepalive: 60,
        certificate_dir: "/etc/rustproxy/cert.d".to_string(),
        access_log: Default::default(),
        monitoring: Default::default(),
        certificates: Vec::new(),
        tls_listeners: Vec::new(),
        match_sets: Vec::new(),
    }
}

#[tokio::test]
async fn test_header_routing() {
    let config = build_config();
    let matcher = Matcher::new(config.rules.clone());
    let balancer = Balancer::new(config.upstreams.clone());
    let request = Request::builder()
        .uri("/users")
        .header("x-route", "canary")
        .body(())
        .unwrap();

    let matched_rule = matcher
        .match_request(&request, None)
        .expect("header should match canary rule");
    let selected_target = balancer
        .select(
            &matched_rule.upstream,
            BalanceContext {
                client_ip: None,
                path: request.uri().path(),
            },
        )
        .expect("matched upstream should have a selectable target");

    assert_eq!(matched_rule.id, "canary-header");
    assert_eq!(matched_rule.upstream, "canary");
    assert_eq!(selected_target.url, "http://canary.internal:8080");
}

#[tokio::test]
async fn test_fallback_when_header_does_not_match() {
    let config = build_config();
    let matcher = Matcher::new(config.rules.clone());
    let balancer = Balancer::new(config.upstreams.clone());
    let request = Request::builder().uri("/users").body(()).unwrap();

    let selected_target = matcher
        .match_request(&request, None)
        .and_then(|rule| {
            balancer.select(
                &rule.upstream,
                BalanceContext {
                    client_ip: None,
                    path: request.uri().path(),
                },
            )
        })
        .map(|target| target.url)
        .unwrap_or_else(|| config.fallback.url.clone());

    assert!(matcher.match_request(&request, None).is_none());
    assert_eq!(selected_target, "http://fallback.internal:8080");
}

#[test]
fn proxy_metrics_export_target_limit_and_retry_metrics() {
    let metrics = ProxyMetrics::new().unwrap();

    metrics
        .target_active_connections
        .with_label_values(&["canary", "http://canary.internal:8080"])
        .set(1.0);
    metrics
        .target_queue_length
        .with_label_values(&["canary", "http://canary.internal:8080"])
        .set(0.0);
    metrics
        .target_connection_rejections
        .with_label_values(&["canary", "http://canary.internal:8080", "queue_timeout"])
        .inc();
    metrics
        .upstream_retries
        .with_label_values(&["canary", "http://canary.internal:8080", "timeout"])
        .inc();

    let output = metrics.gather().unwrap();

    assert!(output.contains("rustproxy_proxy_target_active_connections"));
    assert!(output.contains("rustproxy_proxy_target_queue_length"));
    assert!(output.contains("rustproxy_proxy_target_connection_rejections_total"));
    assert!(output.contains("rustproxy_proxy_upstream_retries_total"));
}
