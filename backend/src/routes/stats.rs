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
struct CategoryStat {
    pub classifier_category: Option<String>,
    pub total: i64,
    pub correct: i64,
}

#[derive(Debug, FromRow)]
struct SessionStat {
    pub id: i32,
    pub started_at: chrono::NaiveDateTime,
    pub completed_at: Option<chrono::NaiveDateTime>,
    pub total: i64,
    pub correct: i64,
}

#[derive(Debug, FromRow)]
struct DailyStat {
    pub date: Option<chrono::NaiveDate>,
    pub avg_percentage: Option<f64>,
    pub session_count: i64,
}

pub async fn stats(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let include_reviewed = params
        .get("includeReviewed")
        .map(|s| s == "true")
        .unwrap_or(false);

    let review_filter = if include_reviewed {
        ""
    } else {
        " AND qs.is_review_session = false"
    };

    // 1. Overall stats
    let overall_sql = format!(
        "SELECT COUNT(*) as total,
          COALESCE(SUM(CASE WHEN qa.correct THEN 1 ELSE 0 END), 0) as correct
        FROM question_attempts qa
        JOIN quiz_sessions qs ON qa.session_id = qs.id
        WHERE qa.user_id = $1{}",
        review_filter
    );
    let overall: (i64, i64) = sqlx::query_as(&overall_sql)
        .bind(user_id)
        .fetch_one(&state.pool)
        .await?;

    let total = overall.0;
    let correct = overall.1;
    let accuracy = if total > 0 {
        (correct as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    // 2. Category breakdown
    let category_sql = format!(
        "SELECT jq.classifier_category,
          COUNT(*)::bigint as total,
          SUM(CASE WHEN qa.correct THEN 1 ELSE 0 END)::bigint as correct
        FROM question_attempts qa
        JOIN jeopardy_questions jq ON qa.question_id = jq.id
        JOIN quiz_sessions qs ON qa.session_id = qs.id
        WHERE qa.user_id = $1 AND jq.archived = false{}
        GROUP BY jq.classifier_category
        ORDER BY jq.classifier_category",
        review_filter
    );
    let categories: Vec<CategoryStat> = sqlx::query_as(&category_sql)
        .bind(user_id)
        .fetch_all(&state.pool)
        .await?;

    let category_breakdown: Vec<Value> = categories
        .into_iter()
        .map(|c| {
            let cat_accuracy = if c.total > 0 {
                (c.correct as f64 / c.total as f64) * 100.0
            } else {
                0.0
            };
            json!({
                "category": c.classifier_category,
                "total": c.total,
                "correct": c.correct,
                "accuracy": cat_accuracy,
            })
        })
        .collect();

    // 3. Recent sessions (last 10)
    let sessions_sql = format!(
        "SELECT qs.id, qs.started_at, qs.completed_at,
          COUNT(qa.id)::bigint as total,
          SUM(CASE WHEN qa.correct THEN 1 ELSE 0 END)::bigint as correct
        FROM quiz_sessions qs
        LEFT JOIN question_attempts qa ON qs.id = qa.session_id
        WHERE qs.user_id = $1{}
        GROUP BY qs.id, qs.started_at, qs.completed_at
        ORDER BY qs.started_at DESC
        LIMIT 10",
        review_filter
    );
    let sessions: Vec<SessionStat> = sqlx::query_as(&sessions_sql)
        .bind(user_id)
        .fetch_all(&state.pool)
        .await?;

    let recent_sessions: Vec<Value> = sessions
        .into_iter()
        .map(|s| {
            json!({
                "id": s.id,
                "started_at": s.started_at,
                "completed_at": s.completed_at,
                "total": s.total,
                "correct": s.correct,
            })
        })
        .collect();

    // 4. Daily stats
    let daily_sql = format!(
        "SELECT DATE(qs.completed_at) as date,
          (SUM(CASE WHEN qa.correct THEN 1 ELSE 0 END)::float / NULLIF(COUNT(qa.id), 0)) * 100 as avg_percentage,
          COUNT(DISTINCT qs.id)::bigint as session_count
        FROM quiz_sessions qs
        LEFT JOIN question_attempts qa ON qs.id = qa.session_id
        WHERE qs.user_id = $1 AND qs.completed_at IS NOT NULL{}
        GROUP BY DATE(qs.completed_at)
        ORDER BY date ASC",
        review_filter
    );
    let daily: Vec<DailyStat> = sqlx::query_as(&daily_sql)
        .bind(user_id)
        .fetch_all(&state.pool)
        .await?;

    let daily_stats: Vec<Value> = daily
        .into_iter()
        .map(|d| {
            json!({
                "date": d.date,
                "avgPercentage": d.avg_percentage,
                "sessionCount": d.session_count,
            })
        })
        .collect();

    Ok(Json(json!({
        "overall": {
            "total": total,
            "correct": correct,
            "accuracy": accuracy,
        },
        "categoryBreakdown": category_breakdown,
        "recentSessions": recent_sessions,
        "dailyStats": daily_stats,
    })))
}
