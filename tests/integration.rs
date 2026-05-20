use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use http::Request;
use rustproxy::{
    config::yaml::{AppConfig, Fallback, TcpListenerConfig, TcpListenerMode},
    models::{ConditionExpr, ConditionType, Operator, Rule, Target, Upstream},
    observability::metrics::ProxyMetrics,
    proxy::{
        balancer::{BalanceContext, Balancer},
        matcher::Matcher,
    },
    runtime::drain::DrainController,
    tcp::{run_tcp_listener, TcpRuntime, TcpRuntimeSnapshot},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
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
        tcp_listeners: Vec::new(),
        match_sets: Vec::new(),
    }
}

struct StaticTcpRuntime {
    snapshot: TcpRuntimeSnapshot,
}

impl TcpRuntime for StaticTcpRuntime {
    fn snapshot(&self) -> TcpRuntimeSnapshot {
        self.snapshot.clone()
    }
}

struct SwitchingTcpRuntime {
    snapshot: RwLock<TcpRuntimeSnapshot>,
}

impl SwitchingTcpRuntime {
    fn replace(&self, snapshot: TcpRuntimeSnapshot) {
        *self.snapshot.write().unwrap() = snapshot;
    }
}

impl TcpRuntime for SwitchingTcpRuntime {
    fn snapshot(&self) -> TcpRuntimeSnapshot {
        self.snapshot.read().unwrap().clone()
    }
}

async fn spawn_one_shot_tcp_server(
    response: [u8; 4],
) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = [0_u8; 4];
        stream.read_exact(&mut buf).await.unwrap();
        stream.write_all(&response).await.unwrap();
    });
    (addr, task)
}

fn tcp_runtime_snapshot(
    upstream_addr: std::net::SocketAddr,
    listener_config: TcpListenerConfig,
) -> TcpRuntimeSnapshot {
    tcp_runtime_snapshot_named("echo", upstream_addr, listener_config)
}

fn tcp_runtime_snapshot_named(
    upstream_name: &str,
    upstream_addr: std::net::SocketAddr,
    listener_config: TcpListenerConfig,
) -> TcpRuntimeSnapshot {
    let mut upstreams = HashMap::new();
    upstreams.insert(
        upstream_name.to_string(),
        Upstream {
            name: upstream_name.to_string(),
            skip_ssl: false,
            websocket: false,
            targets: vec![Target {
                url: format!("tcp://{upstream_addr}"),
                weight: 100,
                timeouts: Default::default(),
            }],
            health_check: Default::default(),
            balance: Default::default(),
            retry: Default::default(),
            timeouts: Default::default(),
        },
    );
    TcpRuntimeSnapshot {
        balancer: Arc::new(Balancer::new(upstreams.clone())),
        config: Arc::new(AppConfig {
            upstreams,
            tcp_listeners: vec![listener_config],
            ..build_config()
        }),
    }
}

async fn spawn_tls_passthrough_upstream(
    response: [u8; 4],
) -> (std::net::SocketAddr, tokio::task::JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut header = [0_u8; 5];
        stream.read_exact(&mut header).await.unwrap();
        let record_len = u16::from_be_bytes([header[3], header[4]]) as usize;
        let mut prefix = header.to_vec();
        let start = prefix.len();
        prefix.resize(start + record_len, 0);
        stream.read_exact(&mut prefix[start..]).await.unwrap();
        stream.write_all(&response).await.unwrap();
        prefix
    });
    (addr, task)
}

fn client_hello(host: &str) -> Vec<u8> {
    let host = host.as_bytes();
    let mut server_name = Vec::new();
    server_name.extend_from_slice(&((host.len() + 3) as u16).to_be_bytes());
    server_name.push(0);
    server_name.extend_from_slice(&(host.len() as u16).to_be_bytes());
    server_name.extend_from_slice(host);

    let mut extensions = Vec::new();
    extensions.extend_from_slice(&0_u16.to_be_bytes());
    extensions.extend_from_slice(&(server_name.len() as u16).to_be_bytes());
    extensions.extend_from_slice(&server_name);

    let mut body = Vec::new();
    body.extend_from_slice(&[0x03, 0x03]);
    body.extend_from_slice(&[0_u8; 32]);
    body.push(0);
    body.extend_from_slice(&2_u16.to_be_bytes());
    body.extend_from_slice(&[0x13, 0x01]);
    body.push(1);
    body.push(0);
    body.extend_from_slice(&(extensions.len() as u16).to_be_bytes());
    body.extend_from_slice(&extensions);

    let mut handshake = Vec::new();
    handshake.push(1);
    handshake.push(((body.len() >> 16) & 0xff) as u8);
    handshake.push(((body.len() >> 8) & 0xff) as u8);
    handshake.push((body.len() & 0xff) as u8);
    handshake.extend_from_slice(&body);

    let mut record = Vec::new();
    record.push(22);
    record.extend_from_slice(&[0x03, 0x01]);
    record.extend_from_slice(&(handshake.len() as u16).to_be_bytes());
    record.extend_from_slice(&handshake);
    record
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

#[tokio::test]
async fn test_tcp_listener_forwards_bytes() {
    let (upstream_addr, upstream_task) = spawn_one_shot_tcp_server(*b"ping").await;

    let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();
    let listener_config = TcpListenerConfig {
        name: "echo".to_string(),
        listen: proxy_addr.to_string(),
        mode: TcpListenerMode::Tcp,
        upstream: Some("echo".to_string()),
        sni_routes: HashMap::new(),
        maxconn: None,
    };
    let runtime_snapshot = tcp_runtime_snapshot(upstream_addr, listener_config.clone());
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let proxy_task = tokio::spawn(run_tcp_listener(
        proxy_listener,
        listener_config,
        Arc::new(StaticTcpRuntime {
            snapshot: runtime_snapshot,
        }),
        Arc::new(ProxyMetrics::new().unwrap()),
        DrainController::default(),
        shutdown_rx,
    ));

    let mut client = TcpStream::connect(proxy_addr).await.unwrap();
    client.write_all(b"ping").await.unwrap();
    let mut response = [0_u8; 4];
    client.read_exact(&mut response).await.unwrap();

    assert_eq!(&response, b"ping");

    let _ = shutdown_tx.send(());
    proxy_task.await.unwrap().unwrap();
    upstream_task.await.unwrap();
}

#[tokio::test]
async fn test_tcp_listener_uses_latest_runtime_snapshot() {
    let (first_upstream, first_task) = spawn_one_shot_tcp_server(*b"old!").await;
    let (second_upstream, second_task) = spawn_one_shot_tcp_server(*b"new!").await;

    let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();
    let listener_config = TcpListenerConfig {
        name: "echo".to_string(),
        listen: proxy_addr.to_string(),
        mode: TcpListenerMode::Tcp,
        upstream: Some("echo".to_string()),
        sni_routes: HashMap::new(),
        maxconn: None,
    };
    let runtime = Arc::new(SwitchingTcpRuntime {
        snapshot: RwLock::new(tcp_runtime_snapshot(
            first_upstream,
            listener_config.clone(),
        )),
    });
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let proxy_task = tokio::spawn(run_tcp_listener(
        proxy_listener,
        listener_config.clone(),
        runtime.clone(),
        Arc::new(ProxyMetrics::new().unwrap()),
        DrainController::default(),
        shutdown_rx,
    ));

    let mut first_client = TcpStream::connect(proxy_addr).await.unwrap();
    first_client.write_all(b"ping").await.unwrap();
    let mut response = [0_u8; 4];
    first_client.read_exact(&mut response).await.unwrap();
    assert_eq!(&response, b"old!");
    first_task.await.unwrap();

    runtime.replace(tcp_runtime_snapshot(second_upstream, listener_config));
    let mut second_client = TcpStream::connect(proxy_addr).await.unwrap();
    second_client.write_all(b"ping").await.unwrap();
    second_client.read_exact(&mut response).await.unwrap();
    assert_eq!(&response, b"new!");

    let _ = shutdown_tx.send(());
    proxy_task.await.unwrap().unwrap();
    second_task.await.unwrap();
}

#[tokio::test]
async fn test_tls_passthrough_routes_by_sni() {
    let (app_upstream, app_task) = spawn_tls_passthrough_upstream(*b"app!").await;

    let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();
    let listener_config = TcpListenerConfig {
        name: "tls-app".to_string(),
        listen: proxy_addr.to_string(),
        mode: TcpListenerMode::TlsPassthrough,
        upstream: None,
        sni_routes: {
            let mut routes = HashMap::new();
            routes.insert("app.example.com".to_string(), "app".to_string());
            routes
        },
        maxconn: None,
    };
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let proxy_task = tokio::spawn(run_tcp_listener(
        proxy_listener,
        listener_config.clone(),
        Arc::new(StaticTcpRuntime {
            snapshot: tcp_runtime_snapshot_named("app", app_upstream, listener_config),
        }),
        Arc::new(ProxyMetrics::new().unwrap()),
        DrainController::default(),
        shutdown_rx,
    ));

    let hello = client_hello("app.example.com");
    let mut client = TcpStream::connect(proxy_addr).await.unwrap();
    client.write_all(&hello).await.unwrap();
    let mut response = [0_u8; 4];
    client.read_exact(&mut response).await.unwrap();

    assert_eq!(&response, b"app!");
    assert_eq!(app_task.await.unwrap(), hello);

    let _ = shutdown_tx.send(());
    proxy_task.await.unwrap().unwrap();
}

#[tokio::test]
async fn test_tls_passthrough_uses_default_upstream_when_sni_misses() {
    let (default_upstream, default_task) = spawn_tls_passthrough_upstream(*b"def!").await;

    let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();
    let listener_config = TcpListenerConfig {
        name: "tls-default".to_string(),
        listen: proxy_addr.to_string(),
        mode: TcpListenerMode::TlsPassthrough,
        upstream: Some("default".to_string()),
        sni_routes: HashMap::new(),
        maxconn: None,
    };
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let proxy_task = tokio::spawn(run_tcp_listener(
        proxy_listener,
        listener_config.clone(),
        Arc::new(StaticTcpRuntime {
            snapshot: tcp_runtime_snapshot_named("default", default_upstream, listener_config),
        }),
        Arc::new(ProxyMetrics::new().unwrap()),
        DrainController::default(),
        shutdown_rx,
    ));

    let mut client = TcpStream::connect(proxy_addr).await.unwrap();
    client
        .write_all(&client_hello("missing.example.com"))
        .await
        .unwrap();
    let mut response = [0_u8; 4];
    client.read_exact(&mut response).await.unwrap();

    assert_eq!(&response, b"def!");

    let _ = shutdown_tx.send(());
    proxy_task.await.unwrap().unwrap();
    default_task.await.unwrap();
}

#[tokio::test]
async fn test_tls_passthrough_records_no_sni_route_rejection() {
    let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();
    let listener_config = TcpListenerConfig {
        name: "tls-strict".to_string(),
        listen: proxy_addr.to_string(),
        mode: TcpListenerMode::TlsPassthrough,
        upstream: None,
        sni_routes: {
            let mut routes = HashMap::new();
            routes.insert("app.example.com".to_string(), "app".to_string());
            routes
        },
        maxconn: None,
    };
    let metrics = Arc::new(ProxyMetrics::new().unwrap());
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let proxy_task = tokio::spawn(run_tcp_listener(
        proxy_listener,
        listener_config.clone(),
        Arc::new(StaticTcpRuntime {
            snapshot: tcp_runtime_snapshot_named("app", proxy_addr, listener_config),
        }),
        Arc::clone(&metrics),
        DrainController::default(),
        shutdown_rx,
    ));

    let mut client = TcpStream::connect(proxy_addr).await.unwrap();
    client
        .write_all(&client_hello("missing.example.com"))
        .await
        .unwrap();
    let mut response = [0_u8; 1];
    assert_eq!(client.read(&mut response).await.unwrap(), 0);

    let output = metrics.gather().unwrap();
    assert!(output.contains("reason=\"no_sni_route\""));

    let _ = shutdown_tx.send(());
    proxy_task.await.unwrap().unwrap();
}
