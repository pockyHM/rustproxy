use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use serde_json::{json, Value};

use crate::{
    config::yaml::AppConfig,
    models::{Rule, Upstream},
};

use super::routes::AppState;

#[derive(Serialize)]
pub struct ApiResponse<T>
where
    T: Serialize,
{
    success: bool,
    data: T,
}

impl<T> ApiResponse<T>
where
    T: Serialize,
{
    fn success(data: T) -> Self {
        Self {
            success: true,
            data,
        }
    }
}

#[derive(Serialize)]
struct ApiError {
    success: bool,
    error: String,
}

fn error_response(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(ApiError {
        success: false,
        error: message.into(),
    }))
        .into_response()
}

async fn persist_config(state: &AppState, config: &AppConfig) -> Result<(), String> {
    let yaml = serde_yaml::to_string(config)
        .map_err(|error| format!("failed to serialize config: {error}"))?;
    tokio::fs::write(state.config_path.as_str(), yaml)
        .await
        .map_err(|error| format!("failed to write config: {error}"))?;
    state.metrics.config_reloads.inc();
    Ok(())
}

pub async fn get_config(State(state): State<AppState>) -> Json<ApiResponse<AppConfig>> {
    let config = state.config.read().await.clone();
    Json(ApiResponse::success(config))
}

pub async fn put_config(
    State(state): State<AppState>,
    Json(new_config): Json<AppConfig>,
) -> Result<Json<ApiResponse<AppConfig>>, Response> {
    persist_config(&state, &new_config)
        .await
        .map_err(|error| error_response(StatusCode::INTERNAL_SERVER_ERROR, error))?;

    let mut config = state.config.write().await;
    *config = new_config.clone();

    Ok(Json(ApiResponse::success(new_config)))
}

pub async fn list_rules(State(state): State<AppState>) -> Json<ApiResponse<Vec<Rule>>> {
    let rules = state.config.read().await.rules.clone();
    Json(ApiResponse::success(rules))
}

pub async fn create_rule(
    State(state): State<AppState>,
    Json(rule): Json<Rule>,
) -> Result<(StatusCode, Json<ApiResponse<Rule>>), Response> {
    let updated_config = {
        let mut config = state.config.write().await;
        if config.rules.iter().any(|existing| existing.id == rule.id) {
            return Err(error_response(
                StatusCode::CONFLICT,
                format!("rule '{}' already exists", rule.id),
            ));
        }

        config.rules.push(rule.clone());
        config.clone()
    };

    if let Err(error) = persist_config(&state, &updated_config).await {
        let mut config = state.config.write().await;
        config.rules.retain(|existing| existing.id != rule.id);
        return Err(error_response(StatusCode::INTERNAL_SERVER_ERROR, error));
    }

    Ok((StatusCode::CREATED, Json(ApiResponse::success(rule))))
}

pub async fn update_rule(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(mut rule): Json<Rule>,
) -> Result<Json<ApiResponse<Rule>>, Response> {
    rule.id = id.clone();

    let updated_config = {
        let mut config = state.config.write().await;
        let Some(existing_rule) = config.rules.iter_mut().find(|existing| existing.id == id) else {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                format!("rule '{id}' not found"),
            ));
        };

        *existing_rule = rule.clone();
        config.clone()
    };

    if let Err(error) = persist_config(&state, &updated_config).await {
        return Err(error_response(StatusCode::INTERNAL_SERVER_ERROR, error));
    }

    Ok(Json(ApiResponse::success(rule)))
}

pub async fn delete_rule(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<Value>>, Response> {
    let updated_config = {
        let mut config = state.config.write().await;
        let original_len = config.rules.len();
        config.rules.retain(|rule| rule.id != id);

        if config.rules.len() == original_len {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                format!("rule '{id}' not found"),
            ));
        }

        config.clone()
    };

    if let Err(error) = persist_config(&state, &updated_config).await {
        return Err(error_response(StatusCode::INTERNAL_SERVER_ERROR, error));
    }

    Ok(Json(ApiResponse::success(json!({ "id": id }))))
}

pub async fn list_upstreams(State(state): State<AppState>) -> Json<ApiResponse<Vec<Upstream>>> {
    let upstreams = state
        .config
        .read()
        .await
        .upstreams
        .values()
        .cloned()
        .collect();
    Json(ApiResponse::success(upstreams))
}

pub async fn create_upstream(
    State(state): State<AppState>,
    Json(upstream): Json<Upstream>,
) -> Result<(StatusCode, Json<ApiResponse<Upstream>>), Response> {
    let updated_config = {
        let mut config = state.config.write().await;
        if config.upstreams.contains_key(&upstream.name) {
            return Err(error_response(
                StatusCode::CONFLICT,
                format!("upstream '{}' already exists", upstream.name),
            ));
        }

        config
            .upstreams
            .insert(upstream.name.clone(), upstream.clone());
        config.clone()
    };

    if let Err(error) = persist_config(&state, &updated_config).await {
        let mut config = state.config.write().await;
        config.upstreams.remove(&upstream.name);
        return Err(error_response(StatusCode::INTERNAL_SERVER_ERROR, error));
    }

    Ok((StatusCode::CREATED, Json(ApiResponse::success(upstream))))
}

pub async fn update_upstream(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(mut upstream): Json<Upstream>,
) -> Result<Json<ApiResponse<Upstream>>, Response> {
    upstream.name = id.clone();

    let updated_config = {
        let mut config = state.config.write().await;
        if !config.upstreams.contains_key(&id) {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                format!("upstream '{id}' not found"),
            ));
        }

        config.upstreams.insert(id.clone(), upstream.clone());
        config.clone()
    };

    if let Err(error) = persist_config(&state, &updated_config).await {
        return Err(error_response(StatusCode::INTERNAL_SERVER_ERROR, error));
    }

    Ok(Json(ApiResponse::success(upstream)))
}

pub async fn delete_upstream(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<Value>>, Response> {
    let updated_config = {
        let mut config = state.config.write().await;
        if config.upstreams.remove(&id).is_none() {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                format!("upstream '{id}' not found"),
            ));
        }

        config.clone()
    };

    if let Err(error) = persist_config(&state, &updated_config).await {
        return Err(error_response(StatusCode::INTERNAL_SERVER_ERROR, error));
    }

    Ok(Json(ApiResponse::success(json!({ "id": id }))))
}

pub async fn health() -> Json<ApiResponse<Value>> {
    Json(ApiResponse::success(json!({ "status": "ok" })))
}

pub async fn metrics(State(state): State<AppState>) -> Response {
    match state.metrics.gather() {
        Ok(metrics) => ([
            (header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8"),
        ], metrics)
            .into_response(),
        Err(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to gather metrics: {error}"),
        ),
    }
}
