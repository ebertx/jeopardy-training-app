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
    pub cold_total: i64,
    pub cold_correct: i64,
    pub review_total: i64,
    pub review_correct: i64,
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

#[derive(Debug, FromRow)]
struct DailyAccuracyStat {
    pub date: Option<chrono::NaiveDate>,
    pub total: i64,
    pub correct: i64,
    pub cold_total: i64,
    pub cold_correct: i64,
    pub review_total: i64,
    pub review_correct: i64,
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
        WHERE qa.user_id = $1 AND qa.attempt_kind <> 'mock'{}",
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

    // Cold vs review (all-time) and cold last-30d — the test-relevant metrics.
    let kind_split: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT attempt_kind, COUNT(*)::bigint,
                COALESCE(SUM(CASE WHEN correct THEN 1 ELSE 0 END), 0)::bigint
         FROM question_attempts
         WHERE user_id = $1 AND attempt_kind IN ('new', 'review')
         GROUP BY attempt_kind",
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await?;
    let pack = |t: i64, c: i64| {
        json!({ "total": t, "correct": c,
                "accuracy": if t > 0 { c as f64 / t as f64 * 100.0 } else { 0.0 } })
    };
    let find = |k: &str| kind_split.iter().find(|(kind, _, _)| kind == k)
        .map(|(_, t, c)| (*t, *c)).unwrap_or((0, 0));
    let (cold_t, cold_c) = find("new");
    let (rev_t, rev_c) = find("review");

    let (c30_t, c30_c): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*)::bigint, COALESCE(SUM(CASE WHEN correct THEN 1 ELSE 0 END), 0)::bigint
         FROM question_attempts
         WHERE user_id = $1 AND attempt_kind = 'new' AND answered_at >= now() - interval '30 days'",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;

    let mock_rows: Vec<(i32, Option<chrono::DateTime<chrono::Utc>>, Option<i32>)> = sqlx::query_as(
        "SELECT id, completed_at, score FROM mock_tests
         WHERE user_id = $1 AND completed_at IS NOT NULL ORDER BY completed_at ASC",
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await?;
    let mock_best = mock_rows.iter().filter_map(|(_, _, s)| *s).max();
    let mock_latest = mock_rows.last().and_then(|(_, _, s)| *s);
    let mock_tests: Vec<Value> = mock_rows.into_iter()
        .map(|(id, at, s)| json!({ "id": id, "completedAt": at, "score": s }))
        .collect();

    // 2. Category breakdown
    let category_sql = format!(
        "SELECT jq.classifier_category,
          COUNT(*)::bigint as total,
          SUM(CASE WHEN qa.correct THEN 1 ELSE 0 END)::bigint as correct,
          COUNT(*) FILTER (WHERE qa.attempt_kind = 'new')::bigint as cold_total,
          COUNT(*) FILTER (WHERE qa.attempt_kind = 'new' AND qa.correct)::bigint as cold_correct,
          COUNT(*) FILTER (WHERE qa.attempt_kind = 'review')::bigint as review_total,
          COUNT(*) FILTER (WHERE qa.attempt_kind = 'review' AND qa.correct)::bigint as review_correct
        FROM question_attempts qa
        JOIN jeopardy_questions jq ON qa.question_id = jq.id
        JOIN quiz_sessions qs ON qa.session_id = qs.id
        WHERE qa.user_id = $1 AND jq.archived = false AND qa.attempt_kind <> 'mock'{}
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
            let cold_accuracy = if c.cold_total > 0 {
                (c.cold_correct as f64 / c.cold_total as f64) * 100.0
            } else {
                0.0
            };
            let review_accuracy = if c.review_total > 0 {
                (c.review_correct as f64 / c.review_total as f64) * 100.0
            } else {
                0.0
            };
            json!({
                "category": c.classifier_category,
                "total": c.total,
                "correct": c.correct,
                "accuracy": cat_accuracy,
                "coldTotal": c.cold_total,
                "coldCorrect": c.cold_correct,
                "coldAccuracy": cold_accuracy,
                "reviewTotal": c.review_total,
                "reviewCorrect": c.review_correct,
                "reviewAccuracy": review_accuracy,
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

    // 5. Daily accuracy, last 30 days, straight from attempts (independent of
    // session completion — Done-button exits never complete a session).
    let daily_accuracy_rows: Vec<DailyAccuracyStat> = sqlx::query_as(
        "SELECT DATE(qa.answered_at) as date,
          COUNT(*)::bigint as total,
          SUM(CASE WHEN qa.correct THEN 1 ELSE 0 END)::bigint as correct,
          COUNT(*) FILTER (WHERE qa.attempt_kind = 'new')::bigint as cold_total,
          COUNT(*) FILTER (WHERE qa.attempt_kind = 'new' AND qa.correct)::bigint as cold_correct,
          COUNT(*) FILTER (WHERE qa.attempt_kind = 'review')::bigint as review_total,
          COUNT(*) FILTER (WHERE qa.attempt_kind = 'review' AND qa.correct)::bigint as review_correct
        FROM question_attempts qa
        WHERE qa.user_id = $1 AND qa.answered_at >= now() - interval '30 days' AND qa.attempt_kind <> 'mock'
        GROUP BY DATE(qa.answered_at)
        ORDER BY date ASC",
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await?;

    let daily_accuracy: Vec<Value> = daily_accuracy_rows
        .into_iter()
        .map(|d| {
            let pct = if d.total > 0 {
                (d.correct as f64 / d.total as f64) * 100.0
            } else {
                0.0
            };
            let cold_pct = if d.cold_total > 0 {
                (d.cold_correct as f64 / d.cold_total as f64) * 100.0
            } else {
                0.0
            };
            let review_pct = if d.review_total > 0 {
                (d.review_correct as f64 / d.review_total as f64) * 100.0
            } else {
                0.0
            };
            json!({
                "date": d.date,
                "total": d.total,
                "correct": d.correct,
                "accuracy": pct,
                "coldTotal": d.cold_total,
                "coldCorrect": d.cold_correct,
                "coldAccuracy": cold_pct,
                "reviewTotal": d.review_total,
                "reviewCorrect": d.review_correct,
                "reviewAccuracy": review_pct,
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
        "dailyAccuracy": daily_accuracy,
        "cold": pack(cold_t, cold_c),
        "review": pack(rev_t, rev_c),
        "cold30d": pack(c30_t, c30_c),
        "mockReadiness": {
            "tests": mock_tests, "best": mock_best, "latest": mock_latest,
            "passLine": crate::routes::mock_test::PASS_LINE,
        },
    })))
}
