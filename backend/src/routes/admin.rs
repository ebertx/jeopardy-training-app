use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};
use std::sync::Arc;
use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::models::user::User;
use crate::AppState;

pub async fn list_users(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    if auth.role != "admin" {
        return Err(AppError::Forbidden("Admin access required".into()));
    }

    let users = sqlx::query_as::<_, User>(
        "SELECT * FROM users ORDER BY created_at DESC"
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(json!({ "users": users })))
}

pub async fn approve(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<Value>,
) -> Result<Json<Value>, AppError> {
    if auth.role != "admin" {
        return Err(AppError::Forbidden("Admin access required".into()));
    }

    let user_id = req["userId"]
        .as_i64()
        .ok_or_else(|| AppError::BadRequest("userId required".into()))? as i32;

    sqlx::query("UPDATE users SET approved = true, approved_at = NOW() WHERE id = $1")
        .bind(user_id)
        .execute(&state.pool)
        .await?;

    Ok(Json(json!({ "success": true })))
}
