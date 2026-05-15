use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
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
    pub success: bool,
    pub data: T,
}

impl<T> ApiResponse<T>
where
    T: Serialize,
{
    pub fn success(data: T) -> Self {
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
    (
        status,
        Json(ApiError {
            success: false,
            error: message.into(),
        }),
    )
        .into_response()
}

// ── Auth ──

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginData {
    token: String,
}

#[derive(Serialize)]
pub struct SetupStatus {
    pub users_exist: bool,
}

pub async fn setup_status(State(state): State<AppState>) -> Json<ApiResponse<SetupStatus>> {
    let users = state.db.list_users().unwrap_or_default();
    Json(ApiResponse::success(SetupStatus {
        users_exist: !users.is_empty(),
    }))
}

#[derive(Deserialize)]
pub struct SetupRequest {
    pub username: String,
    pub password: String,
}

pub async fn setup(
    State(state): State<AppState>,
    Json(body): Json<SetupRequest>,
) -> Result<Json<ApiResponse<Value>>, Response> {
    let users = state
        .db
        .list_users()
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if !users.is_empty() {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            "admin user already exists",
        ));
    }

    if body.username.trim().is_empty() || body.password.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "username and password are required",
        ));
    }

    let hash = crate::auth::hash_password(&body.password)
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    state
        .db
        .create_user(&body.username, &hash)
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(ApiResponse::success(
        json!({ "username": body.username }),
    )))
}

pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<ApiResponse<LoginData>>, Response> {
    let hash = state
        .db
        .get_user_password_hash(&body.username)
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let hash = match hash {
        Some(h) => h,
        None => {
            return Err(error_response(
                StatusCode::UNAUTHORIZED,
                "invalid credentials",
            ))
        }
    };

    let valid = crate::auth::verify_password(&body.password, &hash)
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if !valid {
        return Err(error_response(
            StatusCode::UNAUTHORIZED,
            "invalid credentials",
        ));
    }

    let token = crate::auth::jwt::create_token(&body.username, &state.jwt_secret)
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(ApiResponse::success(LoginData { token })))
}

// ── Config ──

pub async fn get_config(State(state): State<AppState>) -> Json<ApiResponse<AppConfig>> {
    let config = state.config.read().await.clone();
    Json(ApiResponse::success(config))
}

pub async fn put_config(
    State(state): State<AppState>,
    Json(new_config): Json<AppConfig>,
) -> Result<Json<ApiResponse<AppConfig>>, Response> {
    super::routes::validate_tls_config(&new_config)
        .map_err(|e| error_response(StatusCode::BAD_REQUEST, e.to_string()))?;

    let old_config = state.config.read().await.clone();
    state
        .sync_proxy_listeners(&new_config)
        .await
        .map_err(|e| error_response(StatusCode::BAD_REQUEST, e.to_string()))?;

    if let Err(error) = state.db.save_full_config(&new_config) {
        rollback_listeners(&state, &old_config, "config save failure").await;
        return Err(error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            error.to_string(),
        ));
    }

    {
        let mut config = state.config.write().await;
        *config = new_config.clone();
    }

    state.rebuild_proxy_runtime(&old_config, &new_config).await;

    state.metrics.config_reloads.inc();
    Ok(Json(ApiResponse::success(new_config)))
}

// ── Rules ──

pub async fn list_rules(State(state): State<AppState>) -> Json<ApiResponse<Vec<Rule>>> {
    let rules = state.config.read().await.rules.clone();
    Json(ApiResponse::success(rules))
}

pub async fn create_rule(
    State(state): State<AppState>,
    Json(mut rule): Json<Rule>,
) -> Result<(StatusCode, Json<ApiResponse<Rule>>), Response> {
    if rule.id.trim().is_empty() {
        rule.id = format!("rule-{}", uuid::Uuid::new_v4().simple());
    }

    {
        let config = state.config.read().await;
        if config.rules.iter().any(|existing| existing.id == rule.id) {
            return Err(error_response(
                StatusCode::CONFLICT,
                format!("rule '{}' already exists", rule.id),
            ));
        }
    }

    {
        let mut next = state.config.read().await.clone();
        next.rules.push(rule.clone());
        super::routes::validate_tls_config(&next)
            .map_err(|e| error_response(StatusCode::BAD_REQUEST, e.to_string()))?;

        state
            .sync_proxy_listeners(&next)
            .await
            .map_err(|e| error_response(StatusCode::BAD_REQUEST, e.to_string()))?;
    }

    let old_config = state.config.read().await.clone();
    if let Err(error) = state.db.create_rule(&rule) {
        rollback_listeners(&state, &old_config, "rule create failure").await;
        return Err(error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            error.to_string(),
        ));
    }

    let new_config = {
        let mut config = state.config.write().await;
        config.rules.push(rule.clone());
        config.clone()
    };
    state.rebuild_proxy_runtime(&old_config, &new_config).await;

    Ok((StatusCode::CREATED, Json(ApiResponse::success(rule))))
}

pub async fn update_rule(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(mut rule): Json<Rule>,
) -> Result<Json<ApiResponse<Rule>>, Response> {
    rule.id = id.clone();

    {
        let mut next = state.config.read().await.clone();
        let Some(existing_rule) = next.rules.iter_mut().find(|existing| existing.id == id) else {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                format!("rule '{id}' not found"),
            ));
        };
        *existing_rule = rule.clone();
        super::routes::validate_tls_config(&next)
            .map_err(|e| error_response(StatusCode::BAD_REQUEST, e.to_string()))?;

        state
            .sync_proxy_listeners(&next)
            .await
            .map_err(|e| error_response(StatusCode::BAD_REQUEST, e.to_string()))?;
    }

    let old_config = state.config.read().await.clone();
    if let Err(error) = state.db.update_rule(&rule) {
        rollback_listeners(&state, &old_config, "rule update failure").await;
        return Err(error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            error.to_string(),
        ));
    }

    let new_config = {
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
    state.rebuild_proxy_runtime(&old_config, &new_config).await;

    Ok(Json(ApiResponse::success(rule)))
}

pub async fn delete_rule(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<Value>>, Response> {
    let old_config = state.config.read().await.clone();
    if !old_config.rules.iter().any(|rule| rule.id == id) {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            format!("rule '{id}' not found"),
        ));
    }

    let mut next_config = old_config.clone();
    next_config.rules.retain(|rule| rule.id != id);
    super::routes::validate_tls_config(&next_config)
        .map_err(|e| error_response(StatusCode::BAD_REQUEST, e.to_string()))?;
    state
        .sync_proxy_listeners(&next_config)
        .await
        .map_err(|e| error_response(StatusCode::BAD_REQUEST, e.to_string()))?;

    let deleted = match state.db.delete_rule(&id) {
        Ok(deleted) => deleted,
        Err(error) => {
            rollback_listeners(&state, &old_config, "rule delete failure").await;
            return Err(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                error.to_string(),
            ));
        }
    };

    if !deleted {
        if let Err(error) = state.sync_proxy_listeners(&old_config).await {
            tracing::error!(%error, "failed to rollback listeners after missing rule delete");
        }
        return Err(error_response(
            StatusCode::NOT_FOUND,
            format!("rule '{id}' not found"),
        ));
    }

    {
        let mut config = state.config.write().await;
        *config = next_config.clone();
    }
    state.rebuild_proxy_runtime(&old_config, &next_config).await;

    Ok(Json(ApiResponse::success(json!({ "id": id }))))
}

async fn rollback_listeners(state: &AppState, old_config: &AppConfig, reason: &str) {
    if let Err(error) = state.sync_proxy_listeners(old_config).await {
        tracing::error!(%error, %reason, "failed to rollback proxy listeners");
    }
}

// ── Upstreams ──

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
    {
        let config = state.config.read().await;
        if config.upstreams.contains_key(&upstream.name) {
            return Err(error_response(
                StatusCode::CONFLICT,
                format!("upstream '{}' already exists", upstream.name),
            ));
        }
    }

    state
        .db
        .create_upstream(&upstream)
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let (old_config, new_config) = {
        let mut config = state.config.write().await;
        let old_config = config.clone();
        config
            .upstreams
            .insert(upstream.name.clone(), upstream.clone());
        let new_config = config.clone();
        (old_config, new_config)
    };
    state.rebuild_proxy_runtime(&old_config, &new_config).await;

    Ok((StatusCode::CREATED, Json(ApiResponse::success(upstream))))
}

pub async fn update_upstream(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(mut upstream): Json<Upstream>,
) -> Result<Json<ApiResponse<Upstream>>, Response> {
    upstream.name = id.clone();

    state
        .db
        .update_upstream(&upstream)
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let (old_config, new_config) = {
        let mut config = state.config.write().await;
        if !config.upstreams.contains_key(&id) {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                format!("upstream '{id}' not found"),
            ));
        }
        let old_config = config.clone();
        config.upstreams.insert(id.clone(), upstream.clone());
        let new_config = config.clone();
        (old_config, new_config)
    };
    state.rebuild_proxy_runtime(&old_config, &new_config).await;

    Ok(Json(ApiResponse::success(upstream)))
}

pub async fn delete_upstream(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<Value>>, Response> {
    let deleted = state
        .db
        .delete_upstream(&id)
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if !deleted {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            format!("upstream '{id}' not found"),
        ));
    }

    let (old_config, new_config) = {
        let mut config = state.config.write().await;
        let old_config = config.clone();
        config.upstreams.remove(&id);
        let new_config = config.clone();
        (old_config, new_config)
    };
    state.rebuild_proxy_runtime(&old_config, &new_config).await;

    Ok(Json(ApiResponse::success(json!({ "id": id }))))
}

// ── Health & Metrics ──

pub async fn health() -> Json<ApiResponse<Value>> {
    Json(ApiResponse::success(json!({ "status": "ok" })))
}

pub async fn metrics(State(state): State<AppState>) -> Response {
    match state.metrics.gather() {
        Ok(metrics_text) => (
            [(
                header::CONTENT_TYPE,
                "text/plain; version=0.0.4; charset=utf-8",
            )],
            metrics_text,
        )
            .into_response(),
        Err(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to gather metrics: {error}"),
        ),
    }
}
