use axum::{extract::State, Json};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::srs::{schedule, CardKind, Prev, Rating};
use crate::AppState;

const LEECH_LAPSES: i32 = 8;

#[derive(sqlx::FromRow)]
struct CardRow {
    state: String,
    interval_days: f64,
    ease: f64,
    reps: i32,
    lapses: i32,
    step_index: i16,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GradeBody {
    pub question_id: i32,
    pub rating: String,
    pub session_id: Option<i32>,
}

pub async fn grade(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<GradeBody>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let rating = Rating::from_wire(&body.rating)
        .ok_or_else(|| AppError::BadRequest("rating must be wrong|got_it|too_easy".into()))?;

    // Ensure a session row exists (mirrors quiz::submit), for question_attempts stats.
    let session_id = match body.session_id {
        Some(id) => id,
        None => {
            let row: (i32,) = sqlx::query_as(
                "INSERT INTO quiz_sessions (user_id, is_review_session) VALUES ($1, false) RETURNING id",
            )
            .bind(user_id)
            .fetch_one(&state.pool)
            .await?;
            row.0
        }
    };

    // Record the attempt for existing stats/analytics.
    sqlx::query(
        "INSERT INTO question_attempts (session_id, question_id, user_id, correct) VALUES ($1, $2, $3, $4)",
    )
    .bind(session_id)
    .bind(body.question_id)
    .bind(user_id)
    .bind(rating.is_correct())
    .execute(&state.pool)
    .await?;

    // Load prior SRS state, if any.
    let existing: Option<CardRow> = sqlx::query_as(
        "SELECT state, interval_days, ease, reps, lapses, step_index
         FROM srs_cards WHERE user_id = $1 AND question_id = $2",
    )
    .bind(user_id)
    .bind(body.question_id)
    .fetch_optional(&state.pool)
    .await?;

    let prev = existing.map(|r| Prev {
        state: CardKind::from_str(&r.state),
        interval_days: r.interval_days,
        ease: r.ease,
        reps: r.reps,
        lapses: r.lapses,
        step_index: r.step_index,
    });

    let out = schedule(prev, rating);
    let now: DateTime<Utc> = Utc::now();
    let due = now + Duration::seconds(out.interval_secs);
    let suspended = out.lapses >= LEECH_LAPSES;

    sqlx::query(
        "INSERT INTO srs_cards
           (user_id, question_id, state, interval_days, ease, due, last_review, reps, lapses, step_index, suspended)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
         ON CONFLICT (user_id, question_id) DO UPDATE SET
           state = EXCLUDED.state,
           interval_days = EXCLUDED.interval_days,
           ease = EXCLUDED.ease,
           due = EXCLUDED.due,
           last_review = EXCLUDED.last_review,
           reps = EXCLUDED.reps,
           lapses = EXCLUDED.lapses,
           step_index = EXCLUDED.step_index,
           suspended = EXCLUDED.suspended",
    )
    .bind(user_id)
    .bind(body.question_id)
    .bind(out.state.as_str())
    .bind(out.interval_days)
    .bind(out.ease)
    .bind(due)
    .bind(now)
    .bind(out.reps)
    .bind(out.lapses)
    .bind(out.step_index)
    .bind(suspended)
    .execute(&state.pool)
    .await?;

    Ok(Json(json!({
        "sessionId": session_id,
        "state": out.state.as_str(),
        "due": due,
        "intervalDays": out.interval_days,
        "requeueInSession": out.requeue_in_session,
    })))
}
