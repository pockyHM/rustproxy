pub mod balancer;
pub mod conditions;
pub mod matcher;
pub mod upstream;

use std::{convert::Infallible, sync::Arc, time::Duration};

use axum::body::Body;
use http::{Request, Response, StatusCode, Uri};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use rustls_native_certs::load_native_certs;

use crate::{config::yaml::AppConfig, proxy::balancer::Balancer, proxy::matcher::Matcher};

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

pub async fn handle_proxy(
    mut request: Request<Body>,
    config: Arc<AppConfig>,
    matcher: Arc<Matcher>,
    balancer: Arc<Balancer>,
) -> Result<Response<Body>, Infallible> {
    let match_request = request_for_matching(&request);
    let target_base = matcher
        .match_request(&match_request)
        .and_then(|rule| balancer.select(&rule.upstream))
        .unwrap_or_else(|| config.fallback.url.clone());

    let target_uri = match build_target_uri(&target_base, request.uri()) {
        Ok(uri) => uri,
        Err(_) => return Ok(bad_gateway()),
    };

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

    let connect_timeout = if config.connect_timeout > 0 {
        Some(Duration::from_secs(config.connect_timeout))
    } else {
        None
    };

    let request_timeout = if config.request_timeout > 0 {
        Some(Duration::from_secs(config.request_timeout))
    } else {
        None
    };

    let send_future = if is_https {
        send_https(request, config.skip_ssl, connect_timeout)
    } else {
        send_http(request, connect_timeout)
    };

    let result = match request_timeout {
        Some(timeout) => tokio::time::timeout(timeout, send_future).await,
        None => Ok(send_future.await),
    };

    match result {
        Ok(Ok(resp)) => {
            tracing::debug!(status = %resp.status(), "proxy response received");
            Ok(resp.map(Body::new))
        }
        Ok(Err(e)) => {
            tracing::warn!(%e, "proxy request failed");
            Ok(bad_gateway())
        }
        Err(_) => {
            tracing::warn!("proxy request timed out");
            Ok(gateway_timeout())
        }
    }
}

fn send_http(
    request: Request<Body>,
    connect_timeout: Option<Duration>,
) -> hyper_util::client::legacy::ResponseFuture {
    let mut connector = hyper_util::client::legacy::connect::HttpConnector::new();
    if let Some(timeout) = connect_timeout {
        connector.set_connect_timeout(Some(timeout));
    }
    let client: Client<hyper_util::client::legacy::connect::HttpConnector, Body> =
        Client::builder(TokioExecutor::new()).build(connector);
    client.request(request)
}

fn send_https(
    request: Request<Body>,
    skip_ssl: bool,
    connect_timeout: Option<Duration>,
) -> hyper_util::client::legacy::ResponseFuture {
    let tls_config = if skip_ssl {
        rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoCertificateVerification))
            .with_no_client_auth()
    } else {
        let mut root_certs = rustls::RootCertStore::empty();
        for cert in load_native_certs().expect("failed to load native certs") {
            root_certs.add(cert).ok();
        }
        rustls::ClientConfig::builder()
            .with_root_certificates(root_certs)
            .with_no_client_auth()
    };

    let mut http_connector = hyper_util::client::legacy::connect::HttpConnector::new();
    if let Some(timeout) = connect_timeout {
        http_connector.set_connect_timeout(Some(timeout));
    }

    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(tls_config)
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .wrap_connector(http_connector);
    let client = Client::builder(TokioExecutor::new()).build(https);
    client.request(request)
}

fn request_for_matching(request: &Request<Body>) -> Request<()> {
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

fn bad_gateway() -> Response<Body> {
    Response::builder()
        .status(StatusCode::BAD_GATEWAY)
        .body(Body::from("Bad Gateway"))
        .expect("static bad gateway response is valid")
}

fn gateway_timeout() -> Response<Body> {
    Response::builder()
        .status(StatusCode::GATEWAY_TIMEOUT)
        .body(Body::from("Gateway Timeout"))
        .expect("static gateway timeout response is valid")
}

#[cfg(test)]
mod tests {
    use super::build_target_uri;
    use http::Uri;

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
    fn avoids_double_slashes_between_target_and_path() {
        let original_uri: Uri = "/api/users".parse().unwrap();
        let target_uri = build_target_uri("http://backend.internal/", &original_uri).unwrap();

        assert_eq!(target_uri, "http://backend.internal/api/users");
    }
}
