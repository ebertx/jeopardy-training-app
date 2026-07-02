use axum::{
    extract::{Query, State},
    Json,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

#[derive(sqlx::FromRow)]
struct CardListRow {
    id: i32,
    question: Option<String>,
    answer: Option<String>,
    category: Option<String>,
    classifier_category: Option<String>,
    clue_value: Option<i32>,
    round: Option<i32>,
    air_date: Option<chrono::NaiveDate>,
    state: String,
    interval_days: f64,
    due: chrono::DateTime<chrono::Utc>,
    lapses: i32,
    suspended: bool,
}

/// Browse the user's SRS deck by state. The state predicate comes from a
/// fixed whitelist (never user text); `category` is always bound.
pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;

    let state_filter = params.get("state").map(|s| s.as_str()).unwrap_or("learning");
    let predicate = match state_filter {
        "learning" => "sc.state IN ('learning','relearning')",
        "due" => "sc.suspended = false AND sc.due <= now() + interval '24 hours'",
        "mastered" => "sc.state = 'review' AND sc.interval_days >= 21",
        "struggling" => "(sc.suspended = true OR sc.lapses >= 4)",
        _ => {
            return Err(AppError::BadRequest(
                "state must be learning|due|mastered|struggling".into(),
            ))
        }
    };

    let category = params.get("category").map(|s| s.as_str()).unwrap_or("all");
    let use_category = category != "all";
    let cat_clause = if use_category {
        " AND jq.classifier_category = $2"
    } else {
        ""
    };

    let count_sql = format!(
        "SELECT COUNT(*) FROM srs_cards sc \
         JOIN jeopardy_questions jq ON jq.id = sc.question_id \
         WHERE sc.user_id = $1 AND jq.archived = false AND {predicate}{cat_clause}"
    );
    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql).bind(user_id);
    if use_category {
        count_q = count_q.bind(category);
    }
    let total: i64 = count_q.fetch_one(&state.pool).await?;

    let list_sql = format!(
        "SELECT jq.id, jq.question, jq.answer, jq.category, jq.classifier_category, \
                jq.clue_value, jq.round, jq.air_date, \
                sc.state, sc.interval_days, sc.due, sc.lapses, sc.suspended \
         FROM srs_cards sc \
         JOIN jeopardy_questions jq ON jq.id = sc.question_id \
         WHERE sc.user_id = $1 AND jq.archived = false AND {predicate}{cat_clause} \
         ORDER BY sc.due ASC \
         LIMIT 200"
    );
    let mut list_q = sqlx::query_as::<_, CardListRow>(&list_sql).bind(user_id);
    if use_category {
        list_q = list_q.bind(category);
    }
    let rows: Vec<CardListRow> = list_q.fetch_all(&state.pool).await?;

    let cards: Vec<Value> = rows
        .into_iter()
        .map(|r| {
            json!({
                "id": r.id,
                "question": r.question,
                "answer": r.answer,
                "category": r.category,
                "classifier_category": r.classifier_category,
                "clue_value": r.clue_value,
                "round": r.round,
                "air_date": r.air_date,
                "state": r.state,
                "interval_days": r.interval_days,
                "due": r.due,
                "lapses": r.lapses,
                "suspended": r.suspended,
            })
        })
        .collect();

    Ok(Json(json!({ "cards": cards, "total": total })))
}
