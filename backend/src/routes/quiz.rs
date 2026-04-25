use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::models::mastery::QuestionMastery;
use crate::models::question::Question;
use crate::AppState;

pub async fn random(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let _ = auth; // authenticated endpoint
    let category = params.get("category").map(|s| s.as_str()).unwrap_or("all");
    let game_types_str = params.get("gameTypes").map(|s| s.as_str()).unwrap_or("");

    let game_types: Vec<&str> = if game_types_str.is_empty() {
        vec![]
    } else {
        game_types_str.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect()
    };

    // Build WHERE clause with bind parameter for category
    let mut conditions = vec![
        "question IS NOT NULL".to_string(),
        "answer IS NOT NULL".to_string(),
        "classifier_category IS NOT NULL".to_string(),
        "air_date IS NOT NULL".to_string(),
        "archived = false".to_string(),
    ];

    // Track bind parameter index (starts at $1)
    let mut bind_idx = 1;
    let use_category_filter = category != "all";
    if use_category_filter {
        conditions.push(format!("classifier_category = ${}", bind_idx));
        bind_idx += 1;
    }
    let _ = bind_idx; // suppress unused warning

    // Add game type exclusion filters (hardcoded strings, safe from injection)
    for gt in &game_types {
        match *gt {
            "kids" => conditions.push(
                "NOT (notes ILIKE '%Kids%' OR notes ILIKE '%Kid''s%')".to_string(),
            ),
            "teen" => conditions.push("NOT (notes ILIKE '%Teen%')".to_string()),
            "college" => conditions.push("NOT (notes ILIKE '%College%')".to_string()),
            _ => {}
        }
    }

    let where_clause = conditions.join(" AND ");

    // Count matching questions
    let count_sql = format!("SELECT COUNT(*) FROM jeopardy_questions WHERE {}", where_clause);
    let mut count_query = sqlx::query_scalar::<_, i64>(&count_sql);
    if use_category_filter {
        count_query = count_query.bind(category);
    }
    let total_count: i64 = count_query
        .fetch_one(&state.pool)
        .await?;

    if total_count == 0 {
        return Err(AppError::NotFound("No questions found matching criteria".to_string()));
    }

    // Exponential distribution for biasing toward recent air dates
    use rand::Rng;
    let random_value: f64 = rand::rng().random();
    let lambda: f64 = 3.5;
    let exponential_random = -(1.0_f64 - random_value).ln() / lambda;
    let normalized_offset = exponential_random.min(1.0);
    let offset = (normalized_offset * total_count as f64).floor() as i64;

    // Fetch question at that offset ordered by air_date DESC
    let question_sql = format!(
        "SELECT id, question, answer, category, classifier_category, clue_value, round, air_date, notes \
         FROM jeopardy_questions WHERE {} ORDER BY air_date DESC LIMIT 1 OFFSET {}",
        where_clause, offset
    );

    let mut question_query = sqlx::query_as::<_, Question>(&question_sql);
    if use_category_filter {
        question_query = question_query.bind(category);
    }
    let question = question_query
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("No question found at offset".to_string()))?;

    Ok(Json(json!({
        "id": question.id,
        "question": question.question,
        "answer": question.answer,
        "category": question.category,
        "classifier_category": question.classifier_category,
        "clue_value": question.clue_value,
        "round": question.round,
        "air_date": question.air_date,
        "notes": question.notes,
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitBody {
    pub question_id: i32,
    pub correct: bool,
    pub session_id: Option<i32>,
    pub is_review_session: Option<bool>,
}

pub async fn submit(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<SubmitBody>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let is_review = body.is_review_session.unwrap_or(false);

    // Get or create session
    let session_id = match body.session_id {
        Some(id) => id,
        None => {
            let row: (i32,) = sqlx::query_as(
                "INSERT INTO quiz_sessions (user_id, is_review_session) VALUES ($1, $2) RETURNING id",
            )
            .bind(user_id)
            .bind(is_review)
            .fetch_one(&state.pool)
            .await?;
            row.0
        }
    };

    // Insert question attempt
    let attempt_row: (i32,) = sqlx::query_as(
        "INSERT INTO question_attempts (session_id, question_id, user_id, correct) VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(session_id)
    .bind(body.question_id)
    .bind(user_id)
    .bind(body.correct)
    .fetch_one(&state.pool)
    .await?;
    let attempt_id = attempt_row.0;

    // Get existing mastery record
    let existing_mastery: Option<QuestionMastery> = sqlx::query_as(
        "SELECT * FROM question_mastery WHERE user_id = $1 AND question_id = $2",
    )
    .bind(user_id)
    .bind(body.question_id)
    .fetch_optional(&state.pool)
    .await?;

    // Calculate new streak and mastery state
    let (new_streak, new_mastered) = if body.correct {
        let prev_streak = existing_mastery.as_ref().map(|m| m.consecutive_correct).unwrap_or(0);
        let streak = prev_streak + 1;
        let mastered = streak >= 3;
        (streak, mastered)
    } else {
        (0, false)
    };

    // Determine mastered_at:
    // - If newly mastered: NOW()
    // - If unmastered (was mastered, now not): NULL
    // - If already mastered and staying mastered: keep existing value
    // - If never mastered and still not: NULL
    let was_mastered = existing_mastery.as_ref().map(|m| m.mastered).unwrap_or(false);
    let newly_mastered = new_mastered && !was_mastered;
    let still_mastered = new_mastered && was_mastered;

    if newly_mastered {
        // Set mastered_at to NOW()
        sqlx::query(
            "INSERT INTO question_mastery (user_id, question_id, consecutive_correct, mastered, mastered_at, last_attempt_at)
             VALUES ($1, $2, $3, $4, NOW(), NOW())
             ON CONFLICT (user_id, question_id) DO UPDATE SET
               consecutive_correct = EXCLUDED.consecutive_correct,
               mastered = EXCLUDED.mastered,
               mastered_at = NOW(),
               last_attempt_at = NOW()",
        )
        .bind(user_id)
        .bind(body.question_id)
        .bind(new_streak)
        .bind(new_mastered)
        .execute(&state.pool)
        .await?;
    } else if still_mastered {
        // Keep existing mastered_at
        sqlx::query(
            "INSERT INTO question_mastery (user_id, question_id, consecutive_correct, mastered, mastered_at, last_attempt_at)
             VALUES ($1, $2, $3, $4, $5, NOW())
             ON CONFLICT (user_id, question_id) DO UPDATE SET
               consecutive_correct = EXCLUDED.consecutive_correct,
               mastered = EXCLUDED.mastered,
               mastered_at = question_mastery.mastered_at,
               last_attempt_at = NOW()",
        )
        .bind(user_id)
        .bind(body.question_id)
        .bind(new_streak)
        .bind(new_mastered)
        .bind(existing_mastery.as_ref().and_then(|m| m.mastered_at))
        .execute(&state.pool)
        .await?;
    } else {
        // mastered_at = NULL (unmastered or never mastered)
        sqlx::query(
            "INSERT INTO question_mastery (user_id, question_id, consecutive_correct, mastered, mastered_at, last_attempt_at)
             VALUES ($1, $2, $3, $4, NULL, NOW())
             ON CONFLICT (user_id, question_id) DO UPDATE SET
               consecutive_correct = EXCLUDED.consecutive_correct,
               mastered = EXCLUDED.mastered,
               mastered_at = NULL,
               last_attempt_at = NOW()",
        )
        .bind(user_id)
        .bind(body.question_id)
        .bind(new_streak)
        .bind(new_mastered)
        .execute(&state.pool)
        .await?;
    }

    Ok(Json(json!({
        "success": true,
        "attemptId": attempt_id,
        "sessionId": session_id,
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompleteBody {
    pub session_id: i32,
}

pub async fn complete(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<CompleteBody>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;

    // Mark session as completed
    sqlx::query(
        "UPDATE quiz_sessions SET completed_at = NOW() WHERE id = $1 AND user_id = $2",
    )
    .bind(body.session_id)
    .bind(user_id)
    .execute(&state.pool)
    .await?;

    // Query attempt stats
    let stats: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*) as total, COALESCE(SUM(CASE WHEN correct THEN 1 ELSE 0 END), 0) as correct \
         FROM question_attempts WHERE session_id = $1",
    )
    .bind(body.session_id)
    .fetch_one(&state.pool)
    .await?;

    let total = stats.0;
    let correct = stats.1;
    let accuracy = if total > 0 {
        correct as f64 / total as f64
    } else {
        0.0
    };

    // Get session timestamps
    let session = sqlx::query_as::<_, crate::models::session::QuizSession>(
        "SELECT * FROM quiz_sessions WHERE id = $1 AND user_id = $2",
    )
    .bind(body.session_id)
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("Session not found".to_string()))?;

    Ok(Json(json!({
        "success": true,
        "summary": {
            "total": total,
            "correct": correct,
            "accuracy": accuracy,
            "startedAt": session.started_at,
            "completedAt": session.completed_at,
        }
    })))
}
