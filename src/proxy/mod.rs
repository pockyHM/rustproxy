pub mod balancer;
pub mod headers;
pub mod health;
pub mod limits;
pub mod matcher;
pub mod path;
pub mod retry;
pub mod upstream;

use std::{
    convert::Infallible,
    pin::Pin,
    sync::Arc,
    task::{Context as TaskContext, Poll},
    time::Duration,
};

use axum::body::{Body, Bytes};
use http::{header, HeaderMap, HeaderName, HeaderValue, Request, Response, StatusCode, Uri};
use http_body_util::BodyExt;
use hyper::body::{Frame, SizeHint};
use hyper_util::client::legacy::Client;
use hyper_util::rt::{TokioExecutor, TokioIo};
use rustls_native_certs::load_native_certs;

use crate::{
    config::yaml::AppConfig,
    models::{HeaderPolicy, LimitPolicy, PathAction, RetryPolicy},
    observability::{
        access_log::{AccessLogEntry, AccessLogger},
        metrics::ProxyMetrics,
    },
    proxy::{
        balancer::{BalanceContext, Balancer, SelectedTarget, TargetLease},
        limits::{LimitContext, LimitPermit, LimitState},
        retry::{should_retry, AttemptOutcome},
    },
    runtime::drain::DrainLease,
    runtime::timeouts::ResolvedTimeoutPolicy,
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
    pub listen: String,
    pub rule: String,
    pub upstream: String,
}

impl ProxyMetricLabels {
    pub fn fallback(listen: impl Into<String>) -> Self {
        Self {
            listen: listen.into(),
            rule: "fallback".to_string(),
            upstream: "fallback".to_string(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ProxyAccessLogContext {
    pub source: String,
}

pub struct ProxyRequestContext {
    pub access: ProxyAccessLogContext,
    pub metric_labels: ProxyMetricLabels,
    pub timeout_policy: ResolvedTimeoutPolicy,
    pub header_policy: HeaderPolicy,
    pub path_actions: Vec<PathAction>,
    pub limit_state: Option<Arc<LimitState>>,
    pub limit_context: Option<LimitContext>,
    pub limit_policy: LimitPolicy,
    pub retry_policy: RetryPolicy,
    pub balancer: Option<Arc<Balancer>>,
    pub balance_client_ip: String,
    pub balance_path: String,
    pub target_lease: Option<TargetLease>,
    pub drain_lease: Option<DrainLease>,
}

struct GuardedResponseBody {
    inner: Pin<Box<Body>>,
    _active_connection: Option<ActiveConnectionGuard>,
    _target_metric: Option<TargetMetricGuard>,
    _limit_permit: Option<LimitPermit>,
    _target_lease: Option<TargetLease>,
    _drain_lease: Option<DrainLease>,
}

#[derive(Debug)]
enum RequestBodyReadError {
    TooLarge,
    Read(axum::Error),
}

impl ProxyClients {
    pub fn new(
        connect_timeout: Option<Duration>,
        pool_max_idle_per_host: usize,
        pool_idle_timeout: Option<Duration>,
        tcp_keepalive: Option<Duration>,
    ) -> Self {
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
    /// Send a health check request using the appropriate client, with timeout.
    pub async fn health_check_request(
        &self,
        request: Request<Body>,
        is_https: bool,
        skip_ssl: bool,
        timeout: Duration,
    ) -> Option<StatusCode> {
        let future = if is_https && skip_ssl {
            self.https_insecure_client.request(request)
        } else if is_https {
            self.https_client.request(request)
        } else {
            self.http_client.request(request)
        };
        tokio::time::timeout(timeout, future)
            .await
            .ok()?
            .ok()
            .map(|response| response.status())
    }
}

impl hyper::body::Body for GuardedResponseBody {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        self.inner.as_mut().poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
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

/// Forward a request to the given target base URL.
pub async fn handle_proxy_with_target(
    mut request: Request<Body>,
    config: Arc<AppConfig>,
    clients: Arc<ProxyClients>,
    target_base: String,
    metrics: Option<Arc<ProxyMetrics>>,
    access_logger: Option<Arc<AccessLogger>>,
    mut proxy_context: ProxyRequestContext,
) -> Result<Response<Body>, Infallible> {
    let start = std::time::Instant::now();
    let original_method = request.method().to_string();
    let original_uri = request.uri().to_string();
    let original_host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("-")
        .to_string();
    let mut active_connection = metrics.as_ref().map(|metrics| {
        metrics.active_connections.inc();
        ActiveConnectionGuard {
            metrics: Arc::clone(metrics),
        }
    });
    let mut target_metric = metrics.as_ref().map(|metrics| {
        metrics
            .target_active_connections
            .with_label_values(&[
                proxy_context.metric_labels.upstream.as_str(),
                target_base.as_str(),
            ])
            .inc();
        metrics
            .target_queue_length
            .with_label_values(&[
                proxy_context.metric_labels.upstream.as_str(),
                target_base.as_str(),
            ])
            .set(0.0);
        TargetMetricGuard {
            metrics: Arc::clone(metrics),
            upstream: proxy_context.metric_labels.upstream.clone(),
            target: target_base.clone(),
        }
    });
    if proxy_context.limit_policy.queue_timeout_ms.is_none() {
        proxy_context.limit_policy.queue_timeout_ms =
            Some(duration_millis_u64(proxy_context.timeout_policy.queue_timeout));
    }
    let mut limit_permit = match (
        proxy_context.limit_state.as_deref(),
        proxy_context.limit_context.as_ref(),
    ) {
        (Some(limit_state), Some(limit_context)) => {
            match limit_state
                .check(
                    limit_context,
                    &proxy_context.limit_policy,
                    request.headers(),
                )
                .await
            {
                Ok(permit) => Some(permit),
                Err(limits::LimitRejection::RateLimited) => {
                    return Ok(record_proxy_outcome(
                        too_many_requests(),
                        metrics.as_deref(),
                        access_logger.as_deref(),
                        &proxy_context.access,
                        &proxy_context.metric_labels,
                        start,
                        &original_method,
                        &original_host,
                        &original_uri,
                        &target_base,
                        Some("rate limit exceeded".to_string()),
                    ));
                }
                Err(limits::LimitRejection::BodyTooLarge) => {
                    return Ok(record_proxy_outcome(
                        payload_too_large(),
                        metrics.as_deref(),
                        access_logger.as_deref(),
                        &proxy_context.access,
                        &proxy_context.metric_labels,
                        start,
                        &original_method,
                        &original_host,
                        &original_uri,
                        &target_base,
                        Some("request body too large".to_string()),
                    ));
                }
                Err(limits::LimitRejection::QueueTimeout) => {
                    if let Some(metrics) = metrics.as_deref() {
                        metrics
                            .target_connection_rejections
                            .with_label_values(&[
                                proxy_context.metric_labels.upstream.as_str(),
                                target_base.as_str(),
                                "queue_timeout",
                            ])
                            .inc();
                    }
                    return Ok(record_proxy_outcome(
                        service_unavailable(),
                        metrics.as_deref(),
                        access_logger.as_deref(),
                        &proxy_context.access,
                        &proxy_context.metric_labels,
                        start,
                        &original_method,
                        &original_host,
                        &original_uri,
                        &target_base,
                        Some("connection limit queue timeout".to_string()),
                    ));
                }
            }
        }
        _ => None::<LimitPermit>,
    };
    let retry_enabled = retry_policy_requires_buffering(&proxy_context.retry_policy);
    let max_body_bytes = proxy_context.limit_policy.max_body_bytes;

    tracing::debug!(
        method = %request.method(),
        original_uri = %request.uri(),
        target_base = %target_base,
        "proxy request incoming"
    );

    if is_builtin_not_found_target(&target_base) {
        return Ok(record_proxy_outcome(
            not_found_page(),
            metrics.as_deref(),
            access_logger.as_deref(),
            &proxy_context.access,
            &proxy_context.metric_labels,
            start,
            &original_method,
            &original_host,
            &original_uri,
            &target_base,
            None,
        ));
    }

    let forward_uri = match path::apply_path_actions(request.uri(), &proxy_context.path_actions) {
        Ok(path::PathDecision::Forward(uri)) => uri,
        Ok(path::PathDecision::Redirect { status, location }) => {
            let response = Response::builder()
                .status(status)
                .header(header::LOCATION, location.clone())
                .body(Body::empty())
                .expect("redirect response is valid");
            return Ok(record_proxy_outcome(
                response,
                metrics.as_deref(),
                access_logger.as_deref(),
                &proxy_context.access,
                &proxy_context.metric_labels,
                start,
                &original_method,
                &original_host,
                &original_uri,
                &location,
                None,
            ));
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to apply path policy");
            return Ok(record_proxy_outcome(
                bad_gateway(),
                metrics.as_deref(),
                access_logger.as_deref(),
                &proxy_context.access,
                &proxy_context.metric_labels,
                start,
                &original_method,
                &original_host,
                &original_uri,
                &target_base,
                Some(format!("failed to apply path policy: {e}")),
            ));
        }
    };

    let upstream_config = config.upstreams.get(&proxy_context.metric_labels.upstream);
    let upstream_websocket = upstream_config.is_some_and(|upstream| upstream.websocket);
    let upstream_skip_ssl = upstream_config.is_some_and(|upstream| upstream.skip_ssl);

    let is_websocket = is_websocket_upgrade(request.headers());
    if is_websocket && !upstream_websocket {
        tracing::warn!("websocket upgrade rejected because websocket support is disabled");
        return Ok(record_proxy_outcome(
            websocket_disabled(),
            metrics.as_deref(),
            access_logger.as_deref(),
            &proxy_context.access,
            &proxy_context.metric_labels,
            start,
            &original_method,
            &original_host,
            &original_uri,
            &target_base,
            Some("websocket support is disabled".to_string()),
        ));
    }

    if let Err(e) =
        headers::apply_request_headers(request.headers_mut(), &proxy_context.header_policy)
    {
        tracing::error!(error = %e, "failed to apply request header policy");
        return Ok(record_proxy_outcome(
            bad_gateway(),
            metrics.as_deref(),
            access_logger.as_deref(),
            &proxy_context.access,
            &proxy_context.metric_labels,
            start,
            &original_method,
            &original_host,
            &original_uri,
            &target_base,
            Some(format!("failed to apply request header policy: {e}")),
        ));
    }

    let server_timeout = proxy_context.timeout_policy.server_timeout;
    let tunnel_timeout = proxy_context.timeout_policy.tunnel_timeout;

    if is_websocket {
        let target_uri = match build_target_uri(&target_base, &forward_uri) {
            Ok(uri) => uri,
            Err(e) => {
                tracing::error!(target_base = %target_base, error = %e, "failed to build target URI");
                return Ok(record_proxy_outcome(
                    bad_gateway(),
                    metrics.as_deref(),
                    access_logger.as_deref(),
                    &proxy_context.access,
                    &proxy_context.metric_labels,
                    start,
                    &original_method,
                    &original_host,
                    &original_uri,
                    &target_base,
                    Some(format!("failed to build target URI: {e}")),
                ));
            }
        };
        set_target_request_parts(&mut request, target_uri);
        let is_https = request
            .uri()
            .scheme_str()
            .is_some_and(|s| s.eq_ignore_ascii_case("https"));
        let client_upgrade = Some(hyper::upgrade::on(&mut request));
        let send_future = send_upstream_request(&clients, request, is_https, upstream_skip_ssl);
        let result = timeout_optional(server_timeout, send_future).await;

        return match result {
            Ok(Ok(mut resp)) => {
                if let Err(e) = headers::apply_response_headers(
                    resp.headers_mut(),
                    &proxy_context.header_policy,
                ) {
                    return Ok(record_proxy_outcome(
                        bad_gateway(),
                        metrics.as_deref(),
                        access_logger.as_deref(),
                        &proxy_context.access,
                        &proxy_context.metric_labels,
                        start,
                        &original_method,
                        &original_host,
                        &original_uri,
                        &target_base,
                        Some(format!("failed to apply response header policy: {e}")),
                    ));
                }
                if resp.status() == StatusCode::SWITCHING_PROTOCOLS {
                    if let Some(client_upgrade) = client_upgrade {
                        let upstream_upgrade = hyper::upgrade::on(&mut resp);
                        let active_connection = active_connection.take();
                        let target_metric = target_metric.take();
                        let limit_permit = limit_permit.take();
                        let target_lease = proxy_context.target_lease.take();
                        let drain_lease = proxy_context.drain_lease.take();
                        tokio::spawn(async move {
                            let _active_connection = active_connection;
                            let _target_metric = target_metric;
                            let _limit_permit = limit_permit;
                            let _target_lease = target_lease;
                            let _drain_lease = drain_lease;
                            let tunnel = tunnel_upgraded(client_upgrade, upstream_upgrade);
                            match timeout_optional(tunnel_timeout, tunnel).await {
                                Ok(Ok(())) => {}
                                Ok(Err(e)) => {
                                    tracing::warn!(%e, "websocket tunnel closed with error");
                                }
                                Err(_) => {
                                    tracing::warn!("websocket tunnel timed out");
                                }
                            }
                        });
                    }
                    return Ok(record_proxy_outcome(
                        resp.map(Body::new),
                        metrics.as_deref(),
                        access_logger.as_deref(),
                        &proxy_context.access,
                        &proxy_context.metric_labels,
                        start,
                        &original_method,
                        &original_host,
                        &original_uri,
                        &target_base,
                        None,
                    ));
                }
                let response = guard_response(
                    resp.map(Body::new),
                    active_connection.take(),
                    target_metric.take(),
                    limit_permit.take(),
                    proxy_context.target_lease.take(),
                    proxy_context.drain_lease.take(),
                );
                Ok(record_proxy_outcome(
                    response,
                    metrics.as_deref(),
                    access_logger.as_deref(),
                    &proxy_context.access,
                    &proxy_context.metric_labels,
                    start,
                    &original_method,
                    &original_host,
                    &original_uri,
                    &target_base,
                    None,
                ))
            }
            Ok(Err(e)) => Ok(record_proxy_outcome(
                bad_gateway(),
                metrics.as_deref(),
                access_logger.as_deref(),
                &proxy_context.access,
                &proxy_context.metric_labels,
                start,
                &original_method,
                &original_host,
                &original_uri,
                &target_base,
                Some(e.to_string()),
            )),
            Err(_) => Ok(record_proxy_outcome(
                gateway_timeout(),
                metrics.as_deref(),
                access_logger.as_deref(),
                &proxy_context.access,
                &proxy_context.metric_labels,
                start,
                &original_method,
                &original_host,
                &original_uri,
                &target_base,
                Some("request timed out".to_string()),
            )),
        };
    }

    if retry_enabled {
        let (parts, body) = request.into_parts();
        let method = parts.method;
        let version = parts.version;
        let headers = parts.headers;
        let body_bytes = match collect_request_body(body, max_body_bytes).await {
            Ok(body) => body,
            Err(RequestBodyReadError::TooLarge) => {
                return Ok(record_proxy_outcome(
                    payload_too_large(),
                    metrics.as_deref(),
                    access_logger.as_deref(),
                    &proxy_context.access,
                    &proxy_context.metric_labels,
                    start,
                    &original_method,
                    &original_host,
                    &original_uri,
                    &target_base,
                    Some("request body too large".to_string()),
                ));
            }
            Err(RequestBodyReadError::Read(e)) => {
                return Ok(record_proxy_outcome(
                    bad_gateway(),
                    metrics.as_deref(),
                    access_logger.as_deref(),
                    &proxy_context.access,
                    &proxy_context.metric_labels,
                    start,
                    &original_method,
                    &original_host,
                    &original_uri,
                    &target_base,
                    Some(format!("failed to read request body: {e}")),
                ));
            }
        };

        let mut current_target_base = target_base;
        let mut current_lease = proxy_context.target_lease.take();
        let mut attempt_index = 0;

        loop {
            let target_uri = match build_target_uri(&current_target_base, &forward_uri) {
                Ok(uri) => uri,
                Err(e) => {
                    tracing::error!(target_base = %current_target_base, error = %e, "failed to build target URI");
                    return Ok(record_proxy_outcome(
                        bad_gateway(),
                        metrics.as_deref(),
                        access_logger.as_deref(),
                        &proxy_context.access,
                        &proxy_context.metric_labels,
                        start,
                        &original_method,
                        &original_host,
                        &original_uri,
                        &current_target_base,
                        Some(format!("failed to build target URI: {e}")),
                    ));
                }
            };
            let is_https = target_uri
                .scheme_str()
                .is_some_and(|s| s.eq_ignore_ascii_case("https"));
            let mut attempt_request = Request::new(Body::from(body_bytes.clone()));
            *attempt_request.method_mut() = method.clone();
            *attempt_request.version_mut() = version;
            *attempt_request.headers_mut() = headers.clone();
            sanitize_retry_request(attempt_request.headers_mut(), body_bytes.len());
            set_target_request_parts(&mut attempt_request, target_uri);

            let send_future =
                send_upstream_request(&clients, attempt_request, is_https, upstream_skip_ssl);
            let result = timeout_optional(server_timeout, send_future).await;

            match result {
                Ok(Ok(mut resp)) => {
                    let status = resp.status();
                    if should_retry(
                        &proxy_context.retry_policy,
                        attempt_index,
                        AttemptOutcome::Response(status),
                    ) {
                        if let Some(next_target) =
                            next_retry_target(&proxy_context, Some(current_target_base.as_str()))
                        {
                            record_upstream_retry(
                                metrics.as_deref(),
                                &proxy_context.metric_labels.upstream,
                                &current_target_base,
                                "status",
                            );
                            drop(current_lease.take());
                            current_target_base = next_target.url;
                            current_lease = Some(next_target.active_connection);
                            attempt_index += 1;
                            continue;
                        }
                    }
                    tracing::debug!(status = %resp.status(), "proxy response received");
                    if let Err(e) = headers::apply_response_headers(
                        resp.headers_mut(),
                        &proxy_context.header_policy,
                    ) {
                        tracing::error!(error = %e, "failed to apply response header policy");
                        return Ok(record_proxy_outcome(
                            bad_gateway(),
                            metrics.as_deref(),
                            access_logger.as_deref(),
                            &proxy_context.access,
                            &proxy_context.metric_labels,
                            start,
                            &original_method,
                            &original_host,
                            &original_uri,
                            &current_target_base,
                            Some(format!("failed to apply response header policy: {e}")),
                        ));
                    }
                    let response = guard_response(
                        resp.map(Body::new),
                        active_connection.take(),
                        target_metric.take(),
                        limit_permit.take(),
                        current_lease.take(),
                        proxy_context.drain_lease.take(),
                    );
                    return Ok(record_proxy_outcome(
                        response,
                        metrics.as_deref(),
                        access_logger.as_deref(),
                        &proxy_context.access,
                        &proxy_context.metric_labels,
                        start,
                        &original_method,
                        &original_host,
                        &original_uri,
                        &current_target_base,
                        None,
                    ));
                }
                Ok(Err(e)) => {
                    if should_retry(
                        &proxy_context.retry_policy,
                        attempt_index,
                        AttemptOutcome::ConnectError,
                    ) {
                        if let Some(next_target) =
                            next_retry_target(&proxy_context, Some(current_target_base.as_str()))
                        {
                            record_upstream_retry(
                                metrics.as_deref(),
                                &proxy_context.metric_labels.upstream,
                                &current_target_base,
                                "connect_error",
                            );
                            drop(current_lease.take());
                            current_target_base = next_target.url;
                            current_lease = Some(next_target.active_connection);
                            attempt_index += 1;
                            continue;
                        }
                    }
                    tracing::warn!(target = %current_target_base, %is_https, %e, "proxy request failed");
                    return Ok(record_proxy_outcome(
                        bad_gateway(),
                        metrics.as_deref(),
                        access_logger.as_deref(),
                        &proxy_context.access,
                        &proxy_context.metric_labels,
                        start,
                        &original_method,
                        &original_host,
                        &original_uri,
                        &current_target_base,
                        Some(e.to_string()),
                    ));
                }
                Err(_) => {
                    if should_retry(
                        &proxy_context.retry_policy,
                        attempt_index,
                        AttemptOutcome::Timeout,
                    ) {
                        if let Some(next_target) =
                            next_retry_target(&proxy_context, Some(current_target_base.as_str()))
                        {
                            record_upstream_retry(
                                metrics.as_deref(),
                                &proxy_context.metric_labels.upstream,
                                &current_target_base,
                                "timeout",
                            );
                            drop(current_lease.take());
                            current_target_base = next_target.url;
                            current_lease = Some(next_target.active_connection);
                            attempt_index += 1;
                            continue;
                        }
                    }
                    tracing::warn!("proxy request timed out");
                    return Ok(record_proxy_outcome(
                        gateway_timeout(),
                        metrics.as_deref(),
                        access_logger.as_deref(),
                        &proxy_context.access,
                        &proxy_context.metric_labels,
                        start,
                        &original_method,
                        &original_host,
                        &original_uri,
                        &current_target_base,
                        Some("request timed out".to_string()),
                    ));
                }
            }
        }
    }

    let (parts, body) = request.into_parts();
    let mut request = if let Some(limit) = max_body_bytes {
        let body_bytes = match collect_request_body(body, Some(limit)).await {
            Ok(body) => body,
            Err(RequestBodyReadError::TooLarge) => {
                return Ok(record_proxy_outcome(
                    payload_too_large(),
                    metrics.as_deref(),
                    access_logger.as_deref(),
                    &proxy_context.access,
                    &proxy_context.metric_labels,
                    start,
                    &original_method,
                    &original_host,
                    &original_uri,
                    &target_base,
                    Some("request body too large".to_string()),
                ));
            }
            Err(RequestBodyReadError::Read(e)) => {
                return Ok(record_proxy_outcome(
                    bad_gateway(),
                    metrics.as_deref(),
                    access_logger.as_deref(),
                    &proxy_context.access,
                    &proxy_context.metric_labels,
                    start,
                    &original_method,
                    &original_host,
                    &original_uri,
                    &target_base,
                    Some(format!("failed to read request body: {e}")),
                ));
            }
        };
        let body_len = body_bytes.len();
        let mut request = Request::from_parts(parts, Body::from(body_bytes));
        sanitize_retry_request(request.headers_mut(), body_len);
        request
    } else {
        Request::from_parts(parts, body)
    };

    let target_uri = match build_target_uri(&target_base, &forward_uri) {
        Ok(uri) => uri,
        Err(e) => {
            tracing::error!(target_base = %target_base, error = %e, "failed to build target URI");
            return Ok(record_proxy_outcome(
                bad_gateway(),
                metrics.as_deref(),
                access_logger.as_deref(),
                &proxy_context.access,
                &proxy_context.metric_labels,
                start,
                &original_method,
                &original_host,
                &original_uri,
                &target_base,
                Some(format!("failed to build target URI: {e}")),
            ));
        }
    };
    set_target_request_parts(&mut request, target_uri);
    let is_https = request
        .uri()
        .scheme_str()
        .is_some_and(|s| s.eq_ignore_ascii_case("https"));
    let send_future = send_upstream_request(&clients, request, is_https, upstream_skip_ssl);
    let result = timeout_optional(server_timeout, send_future).await;

    match result {
        Ok(Ok(mut resp)) => {
            tracing::debug!(status = %resp.status(), "proxy response received");
            if let Err(e) =
                headers::apply_response_headers(resp.headers_mut(), &proxy_context.header_policy)
            {
                tracing::error!(error = %e, "failed to apply response header policy");
                return Ok(record_proxy_outcome(
                    bad_gateway(),
                    metrics.as_deref(),
                    access_logger.as_deref(),
                    &proxy_context.access,
                    &proxy_context.metric_labels,
                    start,
                    &original_method,
                    &original_host,
                    &original_uri,
                    &target_base,
                    Some(format!("failed to apply response header policy: {e}")),
                ));
            }

            let response = guard_response(
                resp.map(Body::new),
                active_connection.take(),
                target_metric.take(),
                limit_permit.take(),
                proxy_context.target_lease.take(),
                proxy_context.drain_lease.take(),
            );
            Ok(record_proxy_outcome(
                response,
                metrics.as_deref(),
                access_logger.as_deref(),
                &proxy_context.access,
                &proxy_context.metric_labels,
                start,
                &original_method,
                &original_host,
                &original_uri,
                &target_base,
                None,
            ))
        }
        Ok(Err(e)) => {
            tracing::warn!(target = %target_base, %is_https, %e, "proxy request failed");
            Ok(record_proxy_outcome(
                bad_gateway(),
                metrics.as_deref(),
                access_logger.as_deref(),
                &proxy_context.access,
                &proxy_context.metric_labels,
                start,
                &original_method,
                &original_host,
                &original_uri,
                &target_base,
                Some(e.to_string()),
            ))
        }
        Err(_) => Ok(record_proxy_outcome(
            gateway_timeout(),
            metrics.as_deref(),
            access_logger.as_deref(),
            &proxy_context.access,
            &proxy_context.metric_labels,
            start,
            &original_method,
            &original_host,
            &original_uri,
            &target_base,
            Some("request timed out".to_string()),
        )),
    }
}

async fn send_upstream_request(
    clients: &ProxyClients,
    request: Request<Body>,
    is_https: bool,
    upstream_skip_ssl: bool,
) -> Result<Response<hyper::body::Incoming>, hyper_util::client::legacy::Error> {
    if is_https && upstream_skip_ssl {
        clients.https_insecure_client.request(request).await
    } else if is_https {
        clients.https_client.request(request).await
    } else {
        clients.http_client.request(request).await
    }
}

fn guard_response(
    response: Response<Body>,
    active_connection: Option<ActiveConnectionGuard>,
    target_metric: Option<TargetMetricGuard>,
    limit_permit: Option<LimitPermit>,
    target_lease: Option<TargetLease>,
    drain_lease: Option<DrainLease>,
) -> Response<Body> {
    response.map(|body| {
        Body::new(GuardedResponseBody {
            inner: Box::pin(body),
            _active_connection: active_connection,
            _target_metric: target_metric,
            _limit_permit: limit_permit,
            _target_lease: target_lease,
            _drain_lease: drain_lease,
        })
    })
}

fn set_target_request_parts(request: &mut Request<Body>, target_uri: Uri) {
    if let Some(host) = target_uri.host() {
        let host_value = if let Some(port) = target_uri.port_u16() {
            format!("{}:{}", host, port)
        } else {
            host.to_string()
        };
        if let Ok(value) = http::HeaderValue::from_str(&host_value) {
            request.headers_mut().insert(header::HOST, value);
        }
    }
    *request.uri_mut() = target_uri;
}

fn sanitize_retry_request(headers: &mut HeaderMap, body_len: usize) {
    strip_hop_by_hop_headers(headers);
    headers.remove(header::CONTENT_LENGTH);
    if let Ok(value) = HeaderValue::from_str(&body_len.to_string()) {
        headers.insert(header::CONTENT_LENGTH, value);
    }
}

fn strip_hop_by_hop_headers(headers: &mut HeaderMap) {
    let mut connection_header_names = Vec::new();
    for value in headers.get_all(header::CONNECTION).iter() {
        if let Ok(value) = value.to_str() {
            for token in value.split(',') {
                let token = token.trim();
                if let Ok(name) = HeaderName::from_bytes(token.as_bytes()) {
                    connection_header_names.push(name);
                }
            }
        }
    }

    for name in [
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
    ] {
        headers.remove(name);
    }
    for name in connection_header_names {
        headers.remove(name);
    }
}

async fn collect_request_body(
    mut body: Body,
    max_body_bytes: Option<u64>,
) -> Result<Bytes, RequestBodyReadError> {
    let mut collected = Vec::new();
    let mut total = 0u64;
    while let Some(frame) = body.frame().await {
        match frame {
            Ok(frame) => {
                if let Ok(data) = frame.into_data() {
                    let len = data.len() as u64;
                    if let Some(max) = max_body_bytes {
                        if total.saturating_add(len) > max {
                            return Err(RequestBodyReadError::TooLarge);
                        }
                    }
                    total = total.saturating_add(len);
                    collected.extend_from_slice(&data);
                }
            }
            Err(e) => return Err(RequestBodyReadError::Read(e)),
        }
    }

    Ok(Bytes::from(collected))
}

fn retry_policy_requires_buffering(policy: &RetryPolicy) -> bool {
    policy.attempts > 0
        && (!policy.retry_on_status.is_empty()
            || policy.retry_on_timeout
            || policy.retry_on_connect_error)
}

fn next_retry_target(
    proxy_context: &ProxyRequestContext,
    excluded_url: Option<&str>,
) -> Option<SelectedTarget> {
    let balancer = proxy_context.balancer.as_ref()?;
    balancer.select_excluding(
        &proxy_context.metric_labels.upstream,
        BalanceContext {
            client_ip: Some(proxy_context.balance_client_ip.as_str()),
            path: proxy_context.balance_path.as_str(),
        },
        excluded_url,
    )
}

async fn timeout_optional<F, T>(duration: Duration, future: F) -> Result<T, tokio::time::error::Elapsed>
where
    F: std::future::Future<Output = T>,
{
    if duration.is_zero() {
        Ok(future.await)
    } else {
        tokio::time::timeout(duration, future).await
    }
}

fn duration_millis_u64(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn record_upstream_retry(
    metrics: Option<&ProxyMetrics>,
    upstream: &str,
    target: &str,
    reason: &str,
) {
    if let Some(metrics) = metrics {
        metrics
            .upstream_retries
            .with_label_values(&[upstream, target, reason])
            .inc();
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

struct TargetMetricGuard {
    metrics: Arc<ProxyMetrics>,
    upstream: String,
    target: String,
}

impl Drop for TargetMetricGuard {
    fn drop(&mut self) {
        self.metrics
            .target_active_connections
            .with_label_values(&[self.upstream.as_str(), self.target.as_str()])
            .dec();
    }
}

#[cfg(test)]
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
                labels.listen.as_str(),
                labels.rule.as_str(),
                labels.upstream.as_str(),
                status.as_str(),
            ])
            .inc();
        metrics
            .request_duration
            .with_label_values(&[
                labels.listen.as_str(),
                labels.rule.as_str(),
                labels.upstream.as_str(),
            ])
            .observe(start.elapsed().as_secs_f64());
    }

    response
}

fn record_proxy_outcome(
    response: Response<Body>,
    metrics: Option<&ProxyMetrics>,
    access_logger: Option<&AccessLogger>,
    access_context: &ProxyAccessLogContext,
    labels: &ProxyMetricLabels,
    start: std::time::Instant,
    method: &str,
    host: &str,
    uri: &str,
    target: &str,
    error: Option<String>,
) -> Response<Body> {
    let elapsed = start.elapsed();
    if let Some(logger) = access_logger {
        logger.record(AccessLogEntry {
            source: access_context.source.clone(),
            method: method.to_string(),
            host: host.to_string(),
            uri: uri.to_string(),
            status: response.status().as_u16(),
            duration_ms: elapsed.as_millis(),
            rule: labels.rule.clone(),
            upstream: labels.upstream.clone(),
            target: target.to_string(),
            error,
        });
    }

    record_proxy_metrics_with_elapsed(response, metrics, labels, elapsed)
}

fn record_proxy_metrics_with_elapsed(
    response: Response<Body>,
    metrics: Option<&ProxyMetrics>,
    labels: &ProxyMetricLabels,
    elapsed: Duration,
) -> Response<Body> {
    if let Some(metrics) = metrics {
        let status = response.status().as_u16().to_string();
        metrics
            .requests_total
            .with_label_values(&[
                labels.listen.as_str(),
                labels.rule.as_str(),
                labels.upstream.as_str(),
                status.as_str(),
            ])
            .inc();
        metrics
            .request_duration
            .with_label_values(&[
                labels.listen.as_str(),
                labels.rule.as_str(),
                labels.upstream.as_str(),
            ])
            .observe(elapsed.as_secs_f64());
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

fn too_many_requests() -> Response<Body> {
    Response::builder()
        .status(StatusCode::TOO_MANY_REQUESTS)
        .body(Body::from("Too Many Requests"))
        .expect("static too many requests response is valid")
}

fn payload_too_large() -> Response<Body> {
    Response::builder()
        .status(StatusCode::PAYLOAD_TOO_LARGE)
        .body(Body::from("Payload Too Large"))
        .expect("static payload too large response is valid")
}

fn service_unavailable() -> Response<Body> {
    Response::builder()
        .status(StatusCode::SERVICE_UNAVAILABLE)
        .body(Body::from("Service Unavailable"))
        .expect("static service unavailable response is valid")
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
        build_target_uri, collect_request_body, guard_response, is_websocket_upgrade,
        not_found_page, record_proxy_metrics, retry_policy_requires_buffering,
        sanitize_retry_request, ProxyMetricLabels, RequestBodyReadError,
    };
    use crate::models::RetryPolicy;
    use crate::observability::metrics::ProxyMetrics;
    use crate::runtime::drain::DrainController;
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
    fn guarded_response_body_holds_drain_lease_until_body_drops() {
        let drain = DrainController::default();
        let lease = drain.try_acquire().expect("lease allowed");

        let response = guard_response(
            Response::new(Body::from("ok")),
            None,
            None,
            None,
            None,
            Some(lease),
        );

        assert_eq!(drain.active(), 1);
        drop(response);
        assert_eq!(drain.active(), 0);
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
            listen: "0.0.0.0:80".to_string(),
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
        assert!(output.contains("rustproxy_proxy_requests_total"));
        assert!(output.contains("rule=\"canary-header\""));
        assert!(output.contains("listen=\"0.0.0.0:80\""));
        assert!(output.contains("upstream=\"canary\""));
        assert!(output.contains("status=\"202\""));
        assert!(output.contains("rustproxy_proxy_request_duration_seconds"));
    }

    #[test]
    fn retry_request_headers_are_sanitized_for_replay() {
        let mut headers = HeaderMap::new();
        headers.insert("connection", "keep-alive, x-strip".parse().unwrap());
        headers.insert("x-strip", "remove-me".parse().unwrap());
        headers.insert("transfer-encoding", "chunked".parse().unwrap());
        headers.insert("content-length", "999".parse().unwrap());
        headers.insert("upgrade", "websocket".parse().unwrap());
        headers.insert("x-keep", "ok".parse().unwrap());

        sanitize_retry_request(&mut headers, 12);

        assert!(!headers.contains_key("connection"));
        assert!(!headers.contains_key("x-strip"));
        assert!(!headers.contains_key("transfer-encoding"));
        assert!(!headers.contains_key("upgrade"));
        assert_eq!(headers.get("content-length").unwrap(), "12");
        assert_eq!(headers.get("x-keep").unwrap(), "ok");
    }

    #[tokio::test]
    async fn collect_request_body_rejects_actual_bytes_above_limit() {
        let result = collect_request_body(Body::from("abcdef"), Some(5)).await;

        assert!(matches!(result, Err(RequestBodyReadError::TooLarge)));
    }

    #[tokio::test]
    async fn collect_request_body_accepts_bytes_at_limit() {
        let body = collect_request_body(Body::from("abcde"), Some(5))
            .await
            .unwrap();

        assert_eq!(body, "abcde");
    }

    #[test]
    fn retry_buffering_only_when_retry_can_fire() {
        assert!(!retry_policy_requires_buffering(&RetryPolicy {
            attempts: 1,
            ..Default::default()
        }));
        assert!(!retry_policy_requires_buffering(&RetryPolicy {
            retry_on_status: vec![502],
            ..Default::default()
        }));
        assert!(retry_policy_requires_buffering(&RetryPolicy {
            attempts: 1,
            retry_on_status: vec![502],
            ..Default::default()
        }));
    }
}
