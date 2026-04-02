use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::FromRow;
use std::collections::HashMap;
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, FromRow)]
struct MasteredRow {
    pub id: i32,
    pub question: Option<String>,
    pub answer: Option<String>,
    pub category: Option<String>,
    pub classifier_category: Option<String>,
    pub clue_value: Option<i32>,
    pub round: Option<String>,
    pub air_date: Option<chrono::NaiveDate>,
    pub mastered_at: Option<chrono::NaiveDateTime>,
}

pub async fn random_mastered(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let category = params.get("category").map(|s| s.as_str()).unwrap_or("all");
    let use_category = category != "all";

    let rows: Vec<MasteredRow> = if use_category {
        sqlx::query_as::<_, MasteredRow>(
            "SELECT jq.id, jq.question, jq.answer, jq.category, jq.classifier_category,
              jq.clue_value, jq.round, jq.air_date, qm.mastered_at
            FROM question_mastery qm
            JOIN jeopardy_questions jq ON qm.question_id = jq.id
            WHERE qm.user_id = $1 AND qm.mastered = true AND jq.archived = false
              AND jq.classifier_category = $2",
        )
        .bind(user_id)
        .bind(category)
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_as::<_, MasteredRow>(
            "SELECT jq.id, jq.question, jq.answer, jq.category, jq.classifier_category,
              jq.clue_value, jq.round, jq.air_date, qm.mastered_at
            FROM question_mastery qm
            JOIN jeopardy_questions jq ON qm.question_id = jq.id
            WHERE qm.user_id = $1 AND qm.mastered = true AND jq.archived = false",
        )
        .bind(user_id)
        .fetch_all(&state.pool)
        .await?
    };

    let total_mastered = rows.len();
    if total_mastered == 0 {
        return Err(AppError::NotFound("No mastered questions found".to_string()));
    }

    use rand::Rng;
    let idx = rand::rng().random_range(0..total_mastered);
    let row = &rows[idx];

    Ok(Json(json!({
        "id": row.id,
        "question": row.question,
        "answer": row.answer,
        "category": row.category,
        "classifier_category": row.classifier_category,
        "clue_value": row.clue_value,
        "round": row.round,
        "air_date": row.air_date,
        "mastered_at": row.mastered_at,
        "total_mastered": total_mastered,
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetBody {
    pub question_id: i32,
}

pub async fn reset(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<ResetBody>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;

    sqlx::query(
        "UPDATE question_mastery SET consecutive_correct = 0, mastered = false, mastered_at = NULL
         WHERE user_id = $1 AND question_id = $2",
    )
    .bind(user_id)
    .bind(body.question_id)
    .execute(&state.pool)
    .await?;

    Ok(Json(json!({ "success": true })))
}
