use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::Serialize;

use crate::api::routes::AppState;

use super::jwt;

#[derive(Clone)]
pub struct AuthUser {
    pub username: String,
}

#[derive(Serialize)]
struct AuthError {
    success: bool,
    error: String,
}

fn unauthorized(msg: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        axum::Json(AuthError {
            success: false,
            error: msg.to_string(),
        }),
    )
        .into_response()
}

pub async fn require_auth(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let auth_header = request
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok());

    let token = match auth_header {
        Some(h) if h.starts_with("Bearer ") => &h[7..],
        _ => return unauthorized("missing or invalid authorization header"),
    };

    match jwt::validate_token(token, &state.jwt_secret) {
        Ok(claims) => {
            request.extensions_mut().insert(AuthUser {
                username: claims.sub,
            });
            next.run(request).await
        }
        Err(_) => unauthorized("invalid or expired token"),
    }
}
