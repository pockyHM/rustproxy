pub mod balancer;
pub mod conditions;
pub mod matcher;
pub mod upstream;

use std::{convert::Infallible, sync::Arc};

use axum::body::Body;
use http::{Request, Response, StatusCode, Uri};
use hyper_util::{
    client::legacy::{connect::HttpConnector, Client},
    rt::TokioExecutor,
};

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

    let client: Client<HttpConnector, Body> = Client::builder(TokioExecutor::new()).build_http();

    match client.request(request).await {
        Ok(response) => Ok(response.map(Body::new)),
        Err(_) => Ok(bad_gateway()),
    }
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
