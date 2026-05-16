use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use axum::body::Body;
use http::{Method, Request, Uri};
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use tokio::net::TcpStream;

use crate::models::{HealthCheck, HealthCheckMode, Upstream};

#[derive(Clone, Default)]
pub struct HealthRegistry {
    targets: Arc<RwLock<HashMap<String, TargetHealth>>>,
}

#[derive(Clone)]
struct TargetHealth {
    healthy: bool,
    successes: u32,
    failures: u32,
}

impl Default for TargetHealth {
    fn default() -> Self {
        Self {
            healthy: true,
            successes: 0,
            failures: 0,
        }
    }
}

#[derive(Clone)]
pub struct HealthProbe {
    pub key: String,
    pub target_url: String,
    pub check: HealthCheck,
    pub skip_ssl: bool,
}

impl HealthRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn target_key(upstream: &str, target_url: &str) -> String {
        format!("{upstream}\u{0}{target_url}")
    }

    pub fn is_healthy(&self, key: &str) -> bool {
        self.targets
            .read()
            .ok()
            .and_then(|targets| targets.get(key).map(|target| target.healthy))
            .unwrap_or(true)
    }

    pub fn record_probe_result(&self, key: &str, check: &HealthCheck, passed: bool) {
        let Ok(mut targets) = self.targets.write() else {
            return;
        };
        let target = targets.entry(key.to_string()).or_default();

        if passed {
            target.successes = target.successes.saturating_add(1);
            target.failures = 0;
            if target.successes >= check.healthy_threshold.max(1) {
                target.healthy = true;
            }
        } else {
            target.failures = target.failures.saturating_add(1);
            target.successes = 0;
            if target.failures >= check.unhealthy_threshold.max(1) {
                target.healthy = false;
            }
        }
    }

    pub fn retain_keys(&self, keys: &HashSet<String>) {
        let Ok(mut targets) = self.targets.write() else {
            return;
        };
        targets.retain(|key, _| keys.contains(key));
    }
}

pub async fn run_health_checks(registry: HealthRegistry, config_rx: ConfigSnapshot) {
    let mut last_checked = HashMap::<String, Instant>::new();
    let mut ticker = tokio::time::interval(Duration::from_secs(1));

    loop {
        ticker.tick().await;
        let probes = config_rx.probes();
        let active_keys = probes.iter().map(|probe| probe.key.clone()).collect();
        registry.retain_keys(&active_keys);

        let now = Instant::now();
        let mut handles = Vec::new();
        for probe in probes {
            let interval = Duration::from_secs(probe.check.interval_seconds.max(1));
            let should_probe = last_checked
                .get(&probe.key)
                .is_none_or(|checked_at| now.duration_since(*checked_at) >= interval);
            if !should_probe {
                continue;
            }

            last_checked.insert(probe.key.clone(), now);
            handles.push(tokio::spawn(run_probe(probe)));
        }

        for handle in handles {
            match handle.await {
                Ok((probe, passed)) => {
                    registry.record_probe_result(&probe.key, &probe.check, passed);
                }
                Err(error) => {
                    tracing::warn!(%error, "health probe task failed");
                }
            }
        }
        last_checked.retain(|key, _| active_keys.contains(key));
    }
}

pub struct ConfigSnapshot {
    upstreams: Arc<RwLock<Vec<HealthProbe>>>,
}

impl ConfigSnapshot {
    pub fn new() -> Self {
        Self {
            upstreams: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn update(&self, upstreams: &HashMap<String, Upstream>) {
        let probes = upstreams
            .values()
            .filter(|upstream| upstream.health_check.enabled)
            .flat_map(|upstream| {
                upstream.targets.iter().map(|target| HealthProbe {
                    key: HealthRegistry::target_key(&upstream.name, &target.url),
                    target_url: target.url.clone(),
                    check: upstream.health_check.clone(),
                    skip_ssl: upstream.skip_ssl,
                })
            })
            .collect();

        if let Ok(mut guard) = self.upstreams.write() {
            *guard = probes;
        }
    }

    fn probes(&self) -> Vec<HealthProbe> {
        self.upstreams
            .read()
            .map(|upstreams| upstreams.clone())
            .unwrap_or_default()
    }
}

impl Default for ConfigSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for ConfigSnapshot {
    fn clone(&self) -> Self {
        Self {
            upstreams: Arc::clone(&self.upstreams),
        }
    }
}

async fn run_probe(probe: HealthProbe) -> (HealthProbe, bool) {
    let timeout = Duration::from_secs(probe.check.timeout_seconds.max(1));
    let passed = match probe.check.mode {
        HealthCheckMode::Tcp => check_tcp(&probe.target_url, timeout).await,
        HealthCheckMode::Http => check_http(&probe, timeout).await,
    };
    (probe, passed)
}

async fn check_tcp(target_url: &str, timeout: Duration) -> bool {
    let Some(addr) = socket_addr_from_target(target_url) else {
        return false;
    };

    matches!(
        tokio::time::timeout(timeout, TcpStream::connect(addr)).await,
        Ok(Ok(_))
    )
}

async fn check_http(probe: &HealthProbe, timeout: Duration) -> bool {
    let Some(uri) = health_uri(&probe.target_url, &probe.check.path) else {
        return false;
    };

    let request = match Request::builder()
        .method(Method::GET)
        .uri(uri.clone())
        .body(Body::empty())
    {
        Ok(request) => request,
        Err(_) => return false,
    };

    let result = if uri
        .scheme_str()
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https"))
    {
        send_https_health_request(request, timeout, probe.skip_ssl).await
    } else {
        send_http_health_request(request, timeout).await
    };

    result.is_some_and(|status| status.as_u16() == probe.check.expected_status)
}

async fn send_http_health_request(
    request: Request<Body>,
    timeout: Duration,
) -> Option<http::StatusCode> {
    let mut connector = hyper_util::client::legacy::connect::HttpConnector::new();
    connector.set_connect_timeout(Some(timeout));
    let client: Client<_, Body> = Client::builder(TokioExecutor::new()).build(connector);
    tokio::time::timeout(timeout, client.request(request))
        .await
        .ok()?
        .ok()
        .map(|response| response.status())
}

async fn send_https_health_request(
    request: Request<Body>,
    timeout: Duration,
    skip_ssl: bool,
) -> Option<http::StatusCode> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let tls_config = if skip_ssl {
        rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(super::NoCertificateVerification))
            .with_no_client_auth()
    } else {
        let mut root_certs = rustls::RootCertStore::empty();
        let native_certs = rustls_native_certs::load_native_certs();
        for error in native_certs.errors {
            tracing::warn!(%error, "failed to load a native certificate for health check");
        }
        if native_certs.certs.is_empty() {
            return None;
        }
        for cert in native_certs.certs {
            root_certs.add(cert).ok();
        }
        rustls::ClientConfig::builder()
            .with_root_certificates(root_certs)
            .with_no_client_auth()
    };

    let mut http_connector = hyper_util::client::legacy::connect::HttpConnector::new();
    http_connector.set_connect_timeout(Some(timeout));
    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(tls_config)
        .https_only()
        .enable_http1()
        .enable_http2()
        .wrap_connector(http_connector);
    let client: Client<_, Body> = Client::builder(TokioExecutor::new()).build(https);

    tokio::time::timeout(timeout, client.request(request))
        .await
        .ok()?
        .ok()
        .map(|response| response.status())
}

fn health_uri(target_url: &str, path: &str) -> Option<Uri> {
    let base: Uri = target_url.parse().ok()?;
    let scheme = base.scheme_str()?;
    let authority = base.authority()?;
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    format!("{scheme}://{authority}{path}").parse().ok()
}

fn socket_addr_from_target(target_url: &str) -> Option<String> {
    let uri: Uri = target_url.parse().ok()?;
    let host = uri.host()?;
    let port = uri.port_u16().or_else(|| match uri.scheme_str() {
        Some("https") => Some(443),
        Some("http") => Some(80),
        _ => None,
    })?;
    Some(format!("{host}:{port}"))
}

#[cfg(test)]
mod tests {
    use super::{health_uri, socket_addr_from_target, HealthRegistry};

    #[test]
    fn tcp_health_uses_only_target_authority() {
        assert_eq!(
            socket_addr_from_target("http://127.0.0.1:8080/api/users"),
            Some("127.0.0.1:8080".to_string())
        );
    }

    #[test]
    fn http_health_replaces_target_path_with_check_path() {
        assert_eq!(
            health_uri("http://backend.internal:8080/api/users", "/ready").unwrap(),
            "http://backend.internal:8080/ready"
        );
    }

    #[test]
    fn unknown_health_status_is_treated_as_healthy() {
        let registry = HealthRegistry::new();

        assert!(registry.is_healthy("missing"));
    }
}
