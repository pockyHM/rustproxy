use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::time::Instant;

use crate::{
    api::{handlers::ApiResponse, routes::AppState},
    models::{Target, Upstream},
    proxy::health::HealthRegistry,
    runtime::state::{RuntimeSnapshot, TargetKey, TargetMode},
};

#[derive(Serialize)]
struct RuntimeApiError {
    success: bool,
    error: String,
}

#[derive(Serialize)]
pub struct RuntimeUpstream {
    name: String,
    targets: Vec<RuntimeTarget>,
}

#[derive(Serialize)]
pub struct RuntimeTarget {
    url: String,
    configured_weight: u32,
    effective_weight: u32,
    weight_override: Option<u32>,
    mode: &'static str,
    active_connections: u32,
    healthy: bool,
    last_error: Option<String>,
}

#[derive(Serialize)]
pub struct RuntimeStickEntry {
    upstream: String,
    key: String,
    target: String,
    expires_in_seconds: u64,
    request_count: u64,
    error_count: u64,
    bytes_in: u64,
    bytes_out: u64,
}

#[derive(Deserialize)]
pub struct TargetOperationRequest {
    target: String,
}

#[derive(Deserialize)]
pub struct TargetWeightRequest {
    target: String,
    weight: u32,
}

pub async fn list_upstreams(
    State(state): State<AppState>,
) -> Json<ApiResponse<Vec<RuntimeUpstream>>> {
    let config = state.config.read().await;
    let snapshot = state.runtime_state().snapshot();
    let mut upstreams = config.upstreams.values().collect::<Vec<_>>();
    upstreams.sort_by(|a, b| a.name.cmp(&b.name));

    Json(ApiResponse::success(
        upstreams
            .into_iter()
            .map(|upstream| RuntimeUpstream {
                name: upstream.name.clone(),
                targets: upstream
                    .targets
                    .iter()
                    .map(|target| runtime_target(&state, &snapshot, upstream, target))
                    .collect(),
            })
            .collect(),
    ))
}

pub async fn stick_table(
    State(state): State<AppState>,
) -> Json<ApiResponse<Vec<RuntimeStickEntry>>> {
    let now = Instant::now();
    let mut entries = state
        .stick_table_snapshot(now)
        .into_iter()
        .map(|entry| RuntimeStickEntry {
            upstream: entry.upstream,
            key: entry.key,
            target: entry.target,
            expires_in_seconds: entry.expires_at.saturating_duration_since(now).as_secs(),
            request_count: entry.request_count,
            error_count: entry.error_count,
            bytes_in: entry.bytes_in,
            bytes_out: entry.bytes_out,
        })
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| {
        a.upstream
            .cmp(&b.upstream)
            .then_with(|| a.key.cmp(&b.key))
            .then_with(|| a.target.cmp(&b.target))
    });
    Json(ApiResponse::success(entries))
}

pub async fn enable_target(
    State(state): State<AppState>,
    Path(upstream): Path<String>,
    Json(body): Json<TargetOperationRequest>,
) -> Result<Json<ApiResponse<RuntimeTarget>>, Response> {
    set_target_mode(state, upstream, body.target, TargetMode::Enabled).await
}

pub async fn disable_target(
    State(state): State<AppState>,
    Path(upstream): Path<String>,
    Json(body): Json<TargetOperationRequest>,
) -> Result<Json<ApiResponse<RuntimeTarget>>, Response> {
    set_target_mode(state, upstream, body.target, TargetMode::Disabled).await
}

pub async fn drain_target(
    State(state): State<AppState>,
    Path(upstream): Path<String>,
    Json(body): Json<TargetOperationRequest>,
) -> Result<Json<ApiResponse<RuntimeTarget>>, Response> {
    set_target_mode(state, upstream, body.target, TargetMode::Drain).await
}

pub async fn set_target_weight(
    State(state): State<AppState>,
    Path(upstream): Path<String>,
    Json(body): Json<TargetWeightRequest>,
) -> Result<Json<ApiResponse<RuntimeTarget>>, Response> {
    let (upstream, target) = find_target(&state, &upstream, &body.target).await?;
    let key = TargetKey::new(&upstream.name, &target.url);
    state
        .runtime_state()
        .set_target_weight(&key, Some(body.weight));

    let config = state.config.read().await.clone();
    state.rebuild_proxy_runtime(&config, &config);

    let snapshot = state.runtime_state().snapshot();
    Ok(Json(ApiResponse::success(runtime_target(
        &state, &snapshot, &upstream, &target,
    ))))
}

async fn set_target_mode(
    state: AppState,
    upstream: String,
    target_url: String,
    mode: TargetMode,
) -> Result<Json<ApiResponse<RuntimeTarget>>, Response> {
    let (upstream, target) = find_target(&state, &upstream, &target_url).await?;
    let key = TargetKey::new(&upstream.name, &target.url);
    state.runtime_state().set_target_mode(&key, mode);

    let snapshot = state.runtime_state().snapshot();
    Ok(Json(ApiResponse::success(runtime_target(
        &state, &snapshot, &upstream, &target,
    ))))
}

async fn find_target(
    state: &AppState,
    upstream_name: &str,
    target_url: &str,
) -> Result<(Upstream, Target), Response> {
    if target_url.trim().is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "target is required",
        ));
    }

    let config = state.config.read().await;
    let upstream = config.upstreams.get(upstream_name).ok_or_else(|| {
        error_response(
            StatusCode::NOT_FOUND,
            format!("upstream '{upstream_name}' not found"),
        )
    })?;
    let target = upstream
        .targets
        .iter()
        .find(|target| target.url == target_url)
        .ok_or_else(|| {
            error_response(
                StatusCode::NOT_FOUND,
                format!("target '{target_url}' not found in upstream '{upstream_name}'"),
            )
        })?;

    Ok((upstream.clone(), target.clone()))
}

fn runtime_target(
    state: &AppState,
    snapshot: &RuntimeSnapshot,
    upstream: &Upstream,
    target: &Target,
) -> RuntimeTarget {
    let key = TargetKey::new(&upstream.name, &target.url);
    let runtime = snapshot.targets.get(&key).cloned().unwrap_or_default();
    let healthy = if upstream.health_check.enabled {
        state
            .health
            .is_healthy(&HealthRegistry::target_key(&upstream.name, &target.url))
    } else {
        true
    };

    RuntimeTarget {
        url: target.url.clone(),
        configured_weight: target.weight,
        effective_weight: runtime.weight_override.unwrap_or(target.weight),
        weight_override: runtime.weight_override,
        mode: mode_name(runtime.mode),
        active_connections: runtime.active_connections,
        healthy,
        last_error: runtime.last_error,
    }
}

fn mode_name(mode: TargetMode) -> &'static str {
    match mode {
        TargetMode::Enabled => "enabled",
        TargetMode::Disabled => "disabled",
        TargetMode::Drain => "drain",
    }
}

fn error_response(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        Json(RuntimeApiError {
            success: false,
            error: message.into(),
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{to_bytes, Body},
        http::{header, Method, Request, StatusCode},
    };
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use tower::ServiceExt;

    use crate::{
        api::routes::{routes, AppState},
        auth::jwt,
        config::yaml::{AppConfig, Fallback},
        models::{BalanceAlgorithm, Target, Upstream},
        runtime::state::{TargetKey, TargetMode},
    };

    fn target(url: &str, weight: u32) -> Target {
        Target {
            url: url.to_string(),
            weight,
            timeouts: Default::default(),
        }
    }

    fn app_config() -> AppConfig {
        let upstream = Upstream {
            name: "backend".to_string(),
            skip_ssl: false,
            websocket: false,
            targets: vec![
                target("http://127.0.0.1:8080/api", 100),
                target("http://127.0.0.1:8081", 25),
            ],
            health_check: Default::default(),
            balance: BalanceAlgorithm::WeightedRoundRobin,
            retry: Default::default(),
            timeouts: Default::default(),
            sticky: Default::default(),
        };
        let mut upstreams = HashMap::new();
        upstreams.insert(upstream.name.clone(), upstream);
        AppConfig {
            listen: "127.0.0.1:3000".to_string(),
            proxy_listen: "127.0.0.1:8080".to_string(),
            timeouts: Default::default(),
            limits: Default::default(),
            connect_timeout: 10,
            request_timeout: 60,
            pool_max_idle_per_host: 32,
            pool_idle_timeout: 90,
            tcp_keepalive: 60,
            certificate_dir: "/tmp".to_string(),
            access_log: Default::default(),
            monitoring: Default::default(),
            certificates: Vec::new(),
            tls_listeners: Vec::new(),
            tcp_listeners: Vec::new(),
            match_sets: Vec::new(),
            rules: Vec::new(),
            upstreams,
            fallback: Fallback {
                url: "http://127.0.0.1:9000".to_string(),
            },
        }
    }

    fn auth_header() -> String {
        format!(
            "Bearer {}",
            jwt::create_token("admin", AppState::TEST_JWT_SECRET).unwrap()
        )
    }

    async fn request(state: AppState, method: Method, uri: &str, body: Option<Value>) -> Value {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, auth_header());
        if body.is_some() {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
        }
        let request = builder
            .body(match body {
                Some(body) => Body::from(body.to_string()),
                None => Body::empty(),
            })
            .unwrap();
        let response = routes(state).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn runtime_api_requires_authentication() {
        let state = AppState::for_test(app_config());

        let response = routes(state)
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/runtime/upstreams")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn runtime_api_lists_and_mutates_target_state() {
        let state = AppState::for_test(app_config());
        let target_url = "http://127.0.0.1:8080/api";
        let key = TargetKey::new("backend", target_url);
        let lease = state.runtime_state_for_test().acquire_target(&key).unwrap();

        let listed = request(state.clone(), Method::GET, "/api/runtime/upstreams", None).await;
        assert_eq!(listed["success"], true);
        assert_eq!(listed["data"][0]["name"], "backend");
        assert_eq!(listed["data"][0]["targets"][0]["url"], target_url);
        assert_eq!(listed["data"][0]["targets"][0]["configured_weight"], 100);
        assert_eq!(listed["data"][0]["targets"][0]["effective_weight"], 100);
        assert_eq!(listed["data"][0]["targets"][0]["mode"], "enabled");
        assert_eq!(listed["data"][0]["targets"][0]["active_connections"], 1);

        request(
            state.clone(),
            Method::POST,
            "/api/runtime/upstreams/backend/targets/disable",
            Some(json!({ "target": target_url })),
        )
        .await;
        assert_eq!(
            state.runtime_state_for_test().snapshot().targets[&key].mode,
            TargetMode::Disabled
        );

        request(
            state.clone(),
            Method::POST,
            "/api/runtime/upstreams/backend/targets/drain",
            Some(json!({ "target": target_url })),
        )
        .await;
        assert_eq!(
            state.runtime_state_for_test().snapshot().targets[&key].mode,
            TargetMode::Drain
        );

        request(
            state.clone(),
            Method::POST,
            "/api/runtime/upstreams/backend/targets/enable",
            Some(json!({ "target": target_url })),
        )
        .await;
        assert_eq!(
            state.runtime_state_for_test().snapshot().targets[&key].mode,
            TargetMode::Enabled
        );

        request(
            state.clone(),
            Method::POST,
            "/api/runtime/upstreams/backend/targets/weight",
            Some(json!({ "target": target_url, "weight": 50 })),
        )
        .await;
        let listed = request(state.clone(), Method::GET, "/api/runtime/upstreams", None).await;
        assert_eq!(listed["data"][0]["targets"][0]["weight_override"], 50);
        assert_eq!(listed["data"][0]["targets"][0]["effective_weight"], 50);

        drop(lease);
    }

    #[tokio::test]
    async fn runtime_api_lists_stick_table_snapshot() {
        let state = AppState::for_test(app_config());

        let listed = request(state, Method::GET, "/api/runtime/stick-table", None).await;

        assert_eq!(listed["success"], true);
        assert!(listed["data"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn runtime_api_rejects_unknown_targets() {
        let state = AppState::for_test(app_config());
        let response = routes(state)
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/runtime/upstreams/backend/targets/disable")
                    .header(header::AUTHORIZATION, auth_header())
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({ "target": "http://127.0.0.1:9999" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
