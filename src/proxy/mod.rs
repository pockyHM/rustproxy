pub mod balancer;
pub mod conditions;
pub mod health;
pub mod matcher;
pub mod upstream;

use std::{convert::Infallible, sync::Arc, time::Duration};

use axum::body::Body;
use http::{header, HeaderMap, Request, Response, StatusCode, Uri};
use hyper_util::client::legacy::Client;
use hyper_util::rt::{TokioExecutor, TokioIo};
use rustls_native_certs::load_native_certs;

use crate::{
    config::yaml::AppConfig, observability::metrics::ProxyMetrics, proxy::balancer::Balancer,
    proxy::matcher::Matcher,
};

// ── TLS verification bypass ──

#[derive(Debug)]
struct NoCertificateVerification;

impl rustls::client::danger::ServerCertVerifier for NoCertificateVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
        ]
    }
}

// ── Shared connection-pooled clients ──

/// Long-lived HTTP/HTTPS clients with connection pooling.
/// Created once at startup, shared across all proxy requests.
pub struct ProxyClients {
    http_client: Client<hyper_util::client::legacy::connect::HttpConnector, Body>,
    https_client: Client<
        hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>,
        Body,
    >,
    https_insecure_client: Client<
        hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>,
        Body,
    >,
}

#[derive(Clone, Debug)]
pub struct ProxyMetricLabels {
    pub rule: String,
    pub upstream: String,
}

impl ProxyMetricLabels {
    pub fn fallback() -> Self {
        Self {
            rule: "fallback".to_string(),
            upstream: "fallback".to_string(),
        }
    }
}

impl ProxyClients {
    pub fn new(
        connect_timeout: Option<Duration>,
        pool_max_idle_per_host: usize,
        pool_idle_timeout: Option<Duration>,
        tcp_keepalive: Option<Duration>,
    ) -> Self {
        // Ensure a crypto provider is installed (required by rustls 0.23+)
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

        // HTTP client with connection pooling
        let mut http_connector = hyper_util::client::legacy::connect::HttpConnector::new();
        http_connector.set_connect_timeout(connect_timeout);
        http_connector.set_reuse_address(true);
        http_connector.set_keepalive(tcp_keepalive);
        let http_client = Client::builder(TokioExecutor::new())
            .pool_max_idle_per_host(pool_max_idle_per_host)
            .pool_idle_timeout(pool_idle_timeout)
            .build(http_connector);

        let mut root_certs = rustls::RootCertStore::empty();
        let native_certs = load_native_certs();
        for error in native_certs.errors {
            tracing::warn!(%error, "failed to load a native certificate");
        }
        for cert in native_certs.certs {
            root_certs.add(cert).ok();
        }
        let tls_config = rustls::ClientConfig::builder()
            .with_root_certificates(root_certs)
            .with_no_client_auth();
        let insecure_tls_config = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoCertificateVerification))
            .with_no_client_auth();

        let secure_connector = https_connector(tls_config);
        let https_insecure_connector = https_connector(insecure_tls_config);

        let https_client = Client::builder(TokioExecutor::new())
            .pool_max_idle_per_host(pool_max_idle_per_host)
            .pool_idle_timeout(pool_idle_timeout)
            .build(secure_connector);
        let https_insecure_client = Client::builder(TokioExecutor::new())
            .pool_max_idle_per_host(pool_max_idle_per_host)
            .pool_idle_timeout(pool_idle_timeout)
            .build(https_insecure_connector);

        Self {
            http_client,
            https_client,
            https_insecure_client,
        }
    }
}

fn https_connector(
    tls_config: rustls::ClientConfig,
) -> hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector> {
    hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(tls_config)
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .build()
}

// ── Proxy handler ──

/// Full proxy handler: match rule, select upstream, forward request.
pub async fn handle_proxy(
    request: Request<Body>,
    config: Arc<AppConfig>,
    matcher: Arc<Matcher>,
    balancer: Arc<Balancer>,
    clients: Arc<ProxyClients>,
    listen_addr: Option<String>,
) -> Result<Response<Body>, Infallible> {
    let match_request = request_for_matching(&request);
    let target_base = listen_addr
        .as_deref()
        .and_then(|addr| matcher.match_request(&match_request, Some(addr)))
        .and_then(|rule| balancer.select(&rule.upstream))
        .unwrap_or_else(|| config.fallback.url.clone());

    handle_proxy_with_target(
        request,
        config,
        balancer,
        clients,
        target_base,
        None,
        ProxyMetricLabels::fallback(),
    )
    .await
}

/// Forward a request to the given target base URL.
pub async fn handle_proxy_with_target(
    mut request: Request<Body>,
    config: Arc<AppConfig>,
    _balancer: Arc<Balancer>,
    clients: Arc<ProxyClients>,
    target_base: String,
    metrics: Option<Arc<ProxyMetrics>>,
    metric_labels: ProxyMetricLabels,
) -> Result<Response<Body>, Infallible> {
    let start = std::time::Instant::now();
    let _active_connection = metrics.as_ref().map(|metrics| {
        metrics.active_connections.inc();
        ActiveConnectionGuard {
            metrics: Arc::clone(metrics),
        }
    });

    tracing::debug!(
        method = %request.method(),
        original_uri = %request.uri(),
        target_base = %target_base,
        "proxy request incoming"
    );

    if is_builtin_not_found_target(&target_base) {
        return Ok(record_proxy_metrics(
            not_found_page(),
            metrics.as_deref(),
            &metric_labels,
            start,
        ));
    }

    let target_uri = match build_target_uri(&target_base, request.uri()) {
        Ok(uri) => uri,
        Err(e) => {
            tracing::error!(target_base = %target_base, error = %e, "failed to build target URI");
            return Ok(record_proxy_metrics(
                bad_gateway(),
                metrics.as_deref(),
                &metric_labels,
                start,
            ));
        }
    };

    tracing::debug!(target_uri = %target_uri, "proxy target resolved");

    let upstream_config = config.upstreams.get(&metric_labels.upstream);
    let upstream_websocket = upstream_config.is_some_and(|upstream| upstream.websocket);
    let upstream_skip_ssl = upstream_config.is_some_and(|upstream| upstream.skip_ssl);

    let is_websocket = is_websocket_upgrade(request.headers());
    if is_websocket && !upstream_websocket {
        tracing::warn!("websocket upgrade rejected because websocket support is disabled");
        return Ok(record_proxy_metrics(
            websocket_disabled(),
            metrics.as_deref(),
            &metric_labels,
            start,
        ));
    }

    // Set the Host header to match the target
    if let Some(host) = target_uri.host() {
        let host_value = if let Some(port) = target_uri.port_u16() {
            format!("{}:{}", host, port)
        } else {
            host.to_string()
        };
        if let Ok(value) = http::HeaderValue::from_str(&host_value) {
            request.headers_mut().insert("host", value);
        }
    }

    *request.uri_mut() = target_uri;

    let is_https = request
        .uri()
        .scheme_str()
        .is_some_and(|s| s.eq_ignore_ascii_case("https"));

    tracing::trace!(%is_https, "proxy scheme determined");

    let request_timeout = if config.request_timeout > 0 {
        Some(Duration::from_secs(config.request_timeout))
    } else {
        None
    };

    let client_upgrade = if is_websocket {
        Some(hyper::upgrade::on(&mut request))
    } else {
        None
    };

    let send_future = if is_https && upstream_skip_ssl {
        clients.https_insecure_client.request(request)
    } else if is_https {
        clients.https_client.request(request)
    } else {
        clients.http_client.request(request)
    };

    let result = match request_timeout {
        Some(timeout) => tokio::time::timeout(timeout, send_future).await,
        None => Ok(send_future.await),
    };

    match result {
        Ok(Ok(mut resp)) => {
            tracing::debug!(status = %resp.status(), "proxy response received");
            if is_websocket && resp.status() == StatusCode::SWITCHING_PROTOCOLS {
                if let Some(client_upgrade) = client_upgrade {
                    let upstream_upgrade = hyper::upgrade::on(&mut resp);
                    tokio::spawn(async move {
                        if let Err(e) = tunnel_upgraded(client_upgrade, upstream_upgrade).await {
                            tracing::warn!(%e, "websocket tunnel closed with error");
                        }
                    });
                }
            }
            Ok(record_proxy_metrics(
                resp.map(Body::new),
                metrics.as_deref(),
                &metric_labels,
                start,
            ))
        }
        Ok(Err(e)) => {
            tracing::warn!(target = %target_base, %is_https, %e, "proxy request failed");
            Ok(record_proxy_metrics(
                bad_gateway(),
                metrics.as_deref(),
                &metric_labels,
                start,
            ))
        }
        Err(_) => {
            tracing::warn!("proxy request timed out");
            Ok(record_proxy_metrics(
                gateway_timeout(),
                metrics.as_deref(),
                &metric_labels,
                start,
            ))
        }
    }
}

struct ActiveConnectionGuard {
    metrics: Arc<ProxyMetrics>,
}

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        self.metrics.active_connections.dec();
    }
}

fn record_proxy_metrics(
    response: Response<Body>,
    metrics: Option<&ProxyMetrics>,
    labels: &ProxyMetricLabels,
    start: std::time::Instant,
) -> Response<Body> {
    if let Some(metrics) = metrics {
        let status = response.status().as_u16().to_string();
        metrics
            .requests_total
            .with_label_values(&[
                labels.rule.as_str(),
                labels.upstream.as_str(),
                status.as_str(),
            ])
            .inc();
        metrics
            .request_duration
            .with_label_values(&[labels.rule.as_str(), labels.upstream.as_str()])
            .observe(start.elapsed().as_secs_f64());
    }

    response
}

async fn tunnel_upgraded(
    client_upgrade: hyper::upgrade::OnUpgrade,
    upstream_upgrade: hyper::upgrade::OnUpgrade,
) -> anyhow::Result<()> {
    let (client, upstream) = tokio::try_join!(client_upgrade, upstream_upgrade)?;
    let mut client = TokioIo::new(client);
    let mut upstream = TokioIo::new(upstream);
    tokio::io::copy_bidirectional(&mut client, &mut upstream).await?;
    Ok(())
}

// ── Helpers ──

pub fn request_for_matching(request: &Request<Body>) -> Request<()> {
    let mut match_request = Request::builder()
        .method(request.method().clone())
        .uri(request.uri().clone())
        .body(())
        .expect("request method and URI came from a valid request");
    *match_request.headers_mut() = request.headers().clone();
    match_request
}

fn build_target_uri(target_base: &str, original_uri: &Uri) -> Result<Uri, http::uri::InvalidUri> {
    let path_and_query = original_uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    format!("{}{}", target_base.trim_end_matches('/'), path_and_query).parse()
}

fn is_websocket_upgrade(headers: &HeaderMap) -> bool {
    let has_connection_upgrade = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|token| token.trim().eq_ignore_ascii_case("upgrade"));

    let has_websocket_upgrade = headers
        .get(header::UPGRADE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));

    has_connection_upgrade && has_websocket_upgrade
}

fn bad_gateway() -> Response<Body> {
    Response::builder()
        .status(StatusCode::BAD_GATEWAY)
        .body(Body::from("Bad Gateway"))
        .expect("static bad gateway response is valid")
}

fn not_found_page() -> Response<Body> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(Body::from(
            r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>404 Not Found</title>
  <style>
    body { margin: 0; min-height: 100vh; display: grid; place-items: center; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; background: #f7f7f4; color: #171717; }
    main { width: min(520px, calc(100vw - 40px)); border: 1px solid #d8d8d2; background: #fff; padding: 32px; }
    h1 { margin: 0 0 10px; font-size: 42px; line-height: 1; }
    p { margin: 0; color: #5f5f58; line-height: 1.6; }
  </style>
</head>
<body><main><h1>404</h1><p>No proxy rule matched this request.</p></main></body>
</html>"#,
        ))
        .expect("static not found response is valid")
}

fn is_builtin_not_found_target(target: &str) -> bool {
    target.trim().eq_ignore_ascii_case("404")
}

fn gateway_timeout() -> Response<Body> {
    Response::builder()
        .status(StatusCode::GATEWAY_TIMEOUT)
        .body(Body::from("Gateway Timeout"))
        .expect("static gateway timeout response is valid")
}

fn websocket_disabled() -> Response<Body> {
    Response::builder()
        .status(StatusCode::UPGRADE_REQUIRED)
        .body(Body::from("WebSocket support is disabled"))
        .expect("static websocket disabled response is valid")
}

#[cfg(test)]
mod tests {
    use super::{
        build_target_uri, is_websocket_upgrade, not_found_page, record_proxy_metrics,
        ProxyMetricLabels,
    };
    use crate::observability::metrics::ProxyMetrics;
    use axum::body::Body;
    use http::{HeaderMap, Response, StatusCode, Uri};
    use std::time::Instant;

    #[test]
    fn builds_target_uri_for_matched_upstream() {
        let original_uri: Uri = "/api/users?page=1".parse().unwrap();
        let target_uri = build_target_uri("http://backend.internal:8080", &original_uri).unwrap();

        assert_eq!(target_uri, "http://backend.internal:8080/api/users?page=1");
    }

    #[test]
    fn builds_target_uri_for_fallback() {
        let original_uri: Uri = "/missing".parse().unwrap();
        let target_uri = build_target_uri("http://fallback.internal", &original_uri).unwrap();

        assert_eq!(target_uri, "http://fallback.internal/missing");
    }

    #[test]
    fn builtin_404_target_returns_not_found_page() {
        let response = not_found_page();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn avoids_double_slashes_between_target_and_path() {
        let original_uri: Uri = "/api/users".parse().unwrap();
        let target_uri = build_target_uri("http://backend.internal/", &original_uri).unwrap();

        assert_eq!(target_uri, "http://backend.internal/api/users");
    }

    #[test]
    fn detects_websocket_upgrade_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("connection", "keep-alive, Upgrade".parse().unwrap());
        headers.insert("upgrade", "websocket".parse().unwrap());

        assert!(is_websocket_upgrade(&headers));
    }

    #[test]
    fn ignores_non_websocket_upgrade_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("connection", "keep-alive".parse().unwrap());
        headers.insert("upgrade", "websocket".parse().unwrap());

        assert!(!is_websocket_upgrade(&headers));
    }

    #[test]
    fn records_proxy_metrics_with_rule_labels() {
        let metrics = ProxyMetrics::new().unwrap();
        let labels = ProxyMetricLabels {
            rule: "canary-header".to_string(),
            upstream: "canary".to_string(),
        };
        let response = Response::builder()
            .status(StatusCode::ACCEPTED)
            .body(Body::empty())
            .unwrap();

        let response = record_proxy_metrics(response, Some(&metrics), &labels, Instant::now());
        let output = metrics.gather().unwrap();

        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert!(output.contains("proxy_requests_total"));
        assert!(output.contains("rule=\"canary-header\""));
        assert!(output.contains("upstream=\"canary\""));
        assert!(output.contains("status=\"202\""));
        assert!(output.contains("proxy_request_duration_seconds"));
    }
}
