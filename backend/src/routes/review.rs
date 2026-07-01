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
    id: i32,
    question: Option<String>,
    answer: Option<String>,
    category: Option<String>,
    classifier_category: Option<String>,
    clue_value: Option<i32>,
    round: Option<i32>,
    air_date: Option<chrono::NaiveDate>,
    reps: i32,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let category = params.get("category").map(|s| s.as_str()).unwrap_or("all");
    let use_category = category != "all";

    // "Review" = SRS cards you're still learning (not yet at the mastered interval),
    // soonest-due first.
    let base = "SELECT jq.id, jq.question, jq.answer, jq.category, jq.classifier_category,
                       jq.clue_value, jq.round, jq.air_date, sc.reps
                FROM srs_cards sc
                JOIN jeopardy_questions jq ON jq.id = sc.question_id
                WHERE sc.user_id = $1 AND sc.suspended = false AND jq.archived = false
                  AND NOT (sc.state = 'review' AND sc.interval_days >= 21)";

    let rows: Vec<ReviewRow> = if use_category {
        let sql = format!("{base} AND jq.classifier_category = $2 ORDER BY sc.due ASC LIMIT 200");
        sqlx::query_as::<_, ReviewRow>(&sql)
            .bind(user_id)
            .bind(category)
            .fetch_all(&state.pool)
            .await?
    } else {
        let sql = format!("{base} ORDER BY sc.due ASC LIMIT 200");
        sqlx::query_as::<_, ReviewRow>(&sql)
            .bind(user_id)
            .fetch_all(&state.pool)
            .await?
    };

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
                    "consecutive_correct": row.reps,
                    "required": 3,
                }
            })
        })
        .collect();

    Ok(Json(json!(result)))
}
