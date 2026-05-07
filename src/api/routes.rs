use std::sync::Arc;

use anyhow::Context;
use axum::{
    body::Body,
    extract::State,
    http::{Request, Response, StatusCode},
    response::IntoResponse,
    routing::{any, get, put},
    Router,
};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::{
    config::yaml::AppConfig,
    observability::metrics::ProxyMetrics,
    proxy::{balancer::Balancer, handle_proxy, matcher::Matcher},
};

use super::handlers;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<AppConfig>>,
    pub config_path: Arc<String>,
    pub metrics: Arc<ProxyMetrics>,
}

pub fn routes(state: AppState) -> Router {
    Router::new()
        .route("/api/config", get(handlers::get_config).put(handlers::put_config))
        .route("/api/rules", get(handlers::list_rules).post(handlers::create_rule))
        .route(
            "/api/rules/{id}",
            put(handlers::update_rule).delete(handlers::delete_rule),
        )
        .route(
            "/api/upstreams",
            get(handlers::list_upstreams).post(handlers::create_upstream),
        )
        .route(
            "/api/upstreams/{id}",
            put(handlers::update_upstream).delete(handlers::delete_upstream),
        )
        .route("/api/health", get(handlers::health))
        .route("/metrics", get(handlers::metrics))
        .fallback(any(proxy_fallback))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn proxy_fallback(
    State(state): State<AppState>,
    request: Request<Body>,
) -> Result<Response<Body>, std::convert::Infallible> {
    if request.uri().path().starts_with("/api/") {
        return Ok((StatusCode::NOT_FOUND, "API endpoint not found").into_response());
    }

    let config = state.config.read().await.clone();
    let matcher = Matcher::new(config.rules.clone());
    let balancer = Balancer::new(config.upstreams.clone());

    handle_proxy(
        request,
        Arc::new(config),
        Arc::new(matcher),
        Arc::new(balancer),
    )
    .await
}

pub async fn run(config: AppConfig, config_path: String) -> anyhow::Result<()> {
    let listen = config.listen.clone();
    let state = AppState {
        config: Arc::new(RwLock::new(config)),
        config_path: Arc::new(config_path),
        metrics: Arc::new(ProxyMetrics::new().context("failed to initialize metrics")?),
    };
    let app = routes(state);
    let listener = TcpListener::bind(&listen)
        .await
        .with_context(|| format!("failed to bind API server to {listen}"))?;

    tracing::info!(%listen, "REST API server listening");
    axum::serve(listener, app)
        .await
        .context("REST API server failed")?;

    Ok(())
}
