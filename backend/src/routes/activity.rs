use axum::{extract::State, Json};
use chrono::{NaiveDate, Utc};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::activity::summarize;
use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

const WINDOW_DAYS: i64 = 28;

pub async fn activity(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let tz: Option<String> = sqlx::query_scalar("SELECT timezone FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&state.pool)
        .await?;
    let zone: chrono_tz::Tz = tz
        .as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(chrono_tz::UTC);
    let today = Utc::now().with_timezone(&zone).date_naive();

    // One row per local day in the window; active when any attempt, Pavlov
    // review, or (pre-review-log history) Pavlov card creation/review landed
    // on that day.
    let rows: Vec<(NaiveDate, bool)> = sqlx::query_as(
        "WITH d AS (SELECT generate_series($2::date - ($4::int - 1), $2::date, '1 day')::date AS day),
         act AS (
           SELECT (answered_at AT TIME ZONE $3)::date AS day FROM question_attempts WHERE user_id = $1
           UNION SELECT (reviewed_at AT TIME ZONE $3)::date FROM pavlov_reviews WHERE user_id = $1
           UNION SELECT (created_at AT TIME ZONE $3)::date FROM pavlov_cards WHERE user_id = $1
           UNION SELECT (last_review AT TIME ZONE $3)::date FROM pavlov_cards WHERE user_id = $1 AND last_review IS NOT NULL)
         SELECT d.day, EXISTS (SELECT 1 FROM act WHERE act.day = d.day) AS active
         FROM d ORDER BY d.day",
    )
    .bind(user_id)
    .bind(today)
    .bind(zone.name())
    .bind(WINDOW_DAYS as i32)
    .fetch_all(&state.pool)
    .await?;

    let flags: Vec<bool> = rows.iter().map(|(_, a)| *a).collect();
    let s = summarize(&flags);
    let days: Vec<Value> = rows.into_iter().map(|(d, a)| json!({ "date": d, "active": a })).collect();
    Ok(Json(json!({ "streak": s.streak, "activeLast28": s.active, "days": days })))
}
