use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::models::question::Question;
use crate::AppState;

#[derive(Deserialize)]
pub struct ArchiveBody {
    pub reason: String,
}

pub async fn get_question(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(id): Path<i32>,
) -> Result<Json<Value>, AppError> {
    let question = sqlx::query_as::<_, Question>(
        "SELECT id, question, answer, category, classifier_category, clue_value, round, air_date, notes
         FROM jeopardy_questions WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;

    match question {
        Some(q) => Ok(Json(serde_json::to_value(q).unwrap())),
        None => Err(AppError::NotFound(format!("Question {} not found", id))),
    }
}

pub async fn archive(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(id): Path<i32>,
    Json(body): Json<ArchiveBody>,
) -> Result<Json<Value>, AppError> {
    sqlx::query(
        "UPDATE jeopardy_questions SET archived = true, archived_reason = $1, archived_at = NOW() WHERE id = $2",
    )
    .bind(&body.reason)
    .bind(id)
    .execute(&state.pool)
    .await?;

    Ok(Json(json!({ "success": true })))
}

pub async fn unarchive(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(id): Path<i32>,
) -> Result<Json<Value>, AppError> {
    sqlx::query(
        "UPDATE jeopardy_questions SET archived = false, archived_reason = NULL, archived_at = NULL WHERE id = $1",
    )
    .bind(id)
    .execute(&state.pool)
    .await?;

    Ok(Json(json!({ "success": true })))
}
