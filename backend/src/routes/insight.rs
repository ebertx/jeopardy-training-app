use axum::{
    extract::{Path, State},
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

pub async fn get_insight(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(question_id): Path<i32>,
) -> Result<Json<Value>, AppError> {
    let _ = auth; // authenticated endpoint
    match crate::insights::ensure_insight(&state, question_id).await? {
        Some(c) => Ok(Json(json!({ "insight": c.insight, "hook": c.hook }))),
        None => Err(AppError::NotFound("No insight available".to_string())),
    }
}
