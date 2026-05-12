pub mod balancer;
pub mod conditions;
pub mod matcher;
pub mod upstream;

use std::{convert::Infallible, sync::Arc};

use axum::body::Body;
use http::{Request, Response, StatusCode, Uri};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use rustls_native_certs::load_native_certs;

use crate::{config::yaml::AppConfig, proxy::balancer::Balancer, proxy::matcher::Matcher};

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
    *request.uri_mut() = target_uri;

    let is_https = request
        .uri()
        .scheme_str()
        .is_some_and(|s| s.eq_ignore_ascii_case("https"));

    let response = if is_https {
        send_https(request).await
    } else {
        send_http(request).await
    };

    match response {
        Ok(resp) => Ok(resp.map(Body::new)),
        Err(e) => {
            tracing::warn!(%e, "proxy request failed");
            Ok(bad_gateway())
        }
    }
}

fn send_http(request: Request<Body>) -> hyper_util::client::legacy::ResponseFuture {
    let client: Client<hyper_util::client::legacy::connect::HttpConnector, Body> =
        Client::builder(TokioExecutor::new()).build_http();
    client.request(request)
}

fn send_https(request: Request<Body>) -> hyper_util::client::legacy::ResponseFuture {
    let client: Client<hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>, Body>;

    let mut root_certs = rustls::RootCertStore::empty();
    for cert in load_native_certs().expect("failed to load native certs") {
        root_certs.add(cert).ok();
    }
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(root_certs)
        .with_no_client_auth();
    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(config)
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .build();
    client = Client::builder(TokioExecutor::new()).build(https);
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
