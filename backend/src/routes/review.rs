use axum::{
    extract::{Query, State},
    Json,
};
use serde_json::{json, Value};
use sqlx::FromRow;
use std::collections::HashMap;
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, FromRow)]
struct ReviewRow {
    pub id: i32,
    pub question: Option<String>,
    pub answer: Option<String>,
    pub category: Option<String>,
    pub classifier_category: Option<String>,
    pub clue_value: Option<i32>,
    pub round: Option<String>,
    pub air_date: Option<chrono::NaiveDate>,
    pub consecutive_correct: i32,
    pub mastered: bool,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let category = params.get("category").map(|s| s.as_str()).unwrap_or("all");
    let use_category = category != "all";

    let sql = if use_category {
        "SELECT DISTINCT ON (jq.id)
          jq.id, jq.question, jq.answer, jq.category, jq.classifier_category,
          jq.clue_value, jq.round, jq.air_date,
          COALESCE(qm.consecutive_correct, 0) as consecutive_correct,
          COALESCE(qm.mastered, false) as mastered
        FROM question_attempts qa
        JOIN jeopardy_questions jq ON qa.question_id = jq.id
        LEFT JOIN question_mastery qm ON qm.question_id = jq.id AND qm.user_id = qa.user_id
        WHERE qa.user_id = $1
          AND qa.correct = false
          AND jq.archived = false
          AND COALESCE(qm.mastered, false) = false
          AND jq.classifier_category = $2
        ORDER BY jq.id, COALESCE(qm.consecutive_correct, 0) DESC"
    } else {
        "SELECT DISTINCT ON (jq.id)
          jq.id, jq.question, jq.answer, jq.category, jq.classifier_category,
          jq.clue_value, jq.round, jq.air_date,
          COALESCE(qm.consecutive_correct, 0) as consecutive_correct,
          COALESCE(qm.mastered, false) as mastered
        FROM question_attempts qa
        JOIN jeopardy_questions jq ON qa.question_id = jq.id
        LEFT JOIN question_mastery qm ON qm.question_id = jq.id AND qm.user_id = qa.user_id
        WHERE qa.user_id = $1
          AND qa.correct = false
          AND jq.archived = false
          AND COALESCE(qm.mastered, false) = false
        ORDER BY jq.id, COALESCE(qm.consecutive_correct, 0) DESC"
    };

    let mut rows: Vec<ReviewRow> = if use_category {
        sqlx::query_as::<_, ReviewRow>(sql)
            .bind(user_id)
            .bind(category)
            .fetch_all(&state.pool)
            .await?
    } else {
        sqlx::query_as::<_, ReviewRow>(sql)
            .bind(user_id)
            .fetch_all(&state.pool)
            .await?
    };

    // Sort by consecutive_correct DESC (closest to mastery first)
    rows.sort_by(|a, b| b.consecutive_correct.cmp(&a.consecutive_correct));

    let result: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            json!({
                "question": {
                    "id": row.id,
                    "question": row.question,
                    "answer": row.answer,
                    "category": row.category,
                    "classifier_category": row.classifier_category,
                    "clue_value": row.clue_value,
                    "round": row.round,
                    "air_date": row.air_date,
                },
                "masteryProgress": {
                    "consecutive_correct": row.consecutive_correct,
                    "required": 3,
                }
            })
        })
        .collect();

    Ok(Json(json!(result)))
}
