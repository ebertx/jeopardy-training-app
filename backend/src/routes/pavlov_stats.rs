//! GET /api/pavlov/stats — Pavlov's counterpart to /api/stats + /api/practice/status,
//! shaped identically where the concept exists so the dashboard shares types.

use axum::{extract::State, Json};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde_json::{json, Value};
use sqlx::FromRow;
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::pavlov_stats::compute_progress;
use crate::routes::practice::day_start_utc;
use crate::AppState;

#[derive(FromRow)]
struct SplitRow {
    total: i64,
    correct: i64,
    cold_total: i64,
    cold_correct: i64,
    review_total: i64,
    review_correct: i64,
}

#[derive(FromRow)]
struct CategoryRow {
    category: String,
    total: i64,
    correct: i64,
    cold_total: i64,
    cold_correct: i64,
    review_total: i64,
    review_correct: i64,
}

#[derive(FromRow)]
struct DailyRow {
    date: NaiveDate,
    total: i64,
    correct: i64,
    cold_total: i64,
    cold_correct: i64,
    review_total: i64,
    review_correct: i64,
}

fn pct(correct: i64, total: i64) -> f64 {
    if total > 0 {
        correct as f64 / total as f64 * 100.0
    } else {
        0.0
    }
}

fn pack(total: i64, correct: i64) -> Value {
    json!({ "total": total, "correct": correct, "accuracy": pct(correct, total) })
}

/// The six aggregate columns every split query shares. `$1` is user_id.
const SPLIT_COLS: &str = "COUNT(*)::bigint AS total,
    COUNT(*) FILTER (WHERE rating <> 'wrong')::bigint AS correct,
    COUNT(*) FILTER (WHERE first_grade)::bigint AS cold_total,
    COUNT(*) FILTER (WHERE first_grade AND rating <> 'wrong')::bigint AS cold_correct,
    COUNT(*) FILTER (WHERE NOT first_grade)::bigint AS review_total,
    COUNT(*) FILTER (WHERE NOT first_grade AND rating <> 'wrong')::bigint AS review_correct";

pub async fn stats(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;

    let (new_per_day, tz, target_date): (i32, Option<String>, NaiveDate) = sqlx::query_as(
        "SELECT pavlov_new_per_day, timezone, pavlov_target_date FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
    let zone: chrono_tz::Tz = tz
        .as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(chrono_tz::UTC);
    let now = Utc::now();
    let day_start = day_start_utc(now, tz.as_deref());
    let today = now.with_timezone(&zone).date_naive();

    // --- accuracy from the review log ---------------------------------------
    let all: SplitRow = sqlx::query_as(&format!(
        "SELECT {SPLIT_COLS} FROM pavlov_reviews WHERE user_id = $1"
    ))
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;

    let (c30_t, c30_c): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*)::bigint, COUNT(*) FILTER (WHERE rating <> 'wrong')::bigint
         FROM pavlov_reviews
         WHERE user_id = $1 AND first_grade AND reviewed_at >= now() - interval '30 days'",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;

    let history_since: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT MIN(reviewed_at) FROM pavlov_reviews WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&state.pool)
            .await?;

    let reviewed_today: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pavlov_reviews WHERE user_id = $1 AND reviewed_at >= $2",
    )
    .bind(user_id)
    .bind(day_start)
    .fetch_one(&state.pool)
    .await?;

    let daily_rows: Vec<DailyRow> = sqlx::query_as(&format!(
        "SELECT (reviewed_at AT TIME ZONE $2)::date AS date, {SPLIT_COLS}
         FROM pavlov_reviews
         WHERE user_id = $1 AND reviewed_at >= now() - interval '30 days'
         GROUP BY 1 ORDER BY 1"
    ))
    .bind(user_id)
    .bind(zone.name())
    .fetch_all(&state.pool)
    .await?;
    let daily_accuracy: Vec<Value> = daily_rows
        .into_iter()
        .map(|d| {
            json!({
                "date": d.date,
                "total": d.total, "correct": d.correct, "accuracy": pct(d.correct, d.total),
                "coldTotal": d.cold_total, "coldCorrect": d.cold_correct,
                "coldAccuracy": pct(d.cold_correct, d.cold_total),
                "reviewTotal": d.review_total, "reviewCorrect": d.review_correct,
                "reviewAccuracy": pct(d.review_correct, d.review_total),
            })
        })
        .collect();

    let category_rows: Vec<CategoryRow> = sqlx::query_as(&format!(
        "SELECT pa.meta_category AS category, {SPLIT_COLS}
         FROM pavlov_reviews pr
         JOIN pavlov_answers pa ON pa.id = pr.answer_id
         WHERE pr.user_id = $1
         GROUP BY 1 ORDER BY 1"
    ))
    .bind(user_id)
    .fetch_all(&state.pool)
    .await?;
    let category_breakdown: Vec<Value> = category_rows
        .into_iter()
        .map(|c| {
            json!({
                "category": c.category,
                "total": c.total, "correct": c.correct, "accuracy": pct(c.correct, c.total),
                "coldTotal": c.cold_total, "coldCorrect": c.cold_correct,
                "coldAccuracy": pct(c.cold_correct, c.cold_total),
                "reviewTotal": c.review_total, "reviewCorrect": c.review_correct,
                "reviewAccuracy": pct(c.review_correct, c.review_total),
            })
        })
        .collect();

    // --- queue state from card rows -----------------------------------------
    let due_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pavlov_cards
         WHERE user_id = $1 AND suspended = false AND due <= now()",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
    let new_today: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pavlov_cards ca
         WHERE ca.user_id = $1 AND ca.created_at >= $2 AND ca.last_review IS NOT NULL",
    )
    .bind(user_id)
    .bind(day_start)
    .fetch_one(&state.pool)
    .await?;
    let new_remaining = (new_per_day as i64 - new_today).max(0);

    // 14-day due forecast, user-local days, overdue folded into today
    // (same shape as /api/practice/status).
    let forecast: Vec<(NaiveDate, i64)> = sqlx::query_as(
        "SELECT GREATEST((due AT TIME ZONE $2)::date, (now() AT TIME ZONE $2)::date) AS d, COUNT(*)
         FROM pavlov_cards
         WHERE user_id = $1 AND suspended = false
           AND (due AT TIME ZONE $2)::date <= (now() AT TIME ZONE $2)::date + 13
         GROUP BY d ORDER BY d",
    )
    .bind(user_id)
    .bind(zone.name())
    .fetch_all(&state.pool)
    .await?;
    let forecast_json: Vec<Value> = forecast
        .into_iter()
        .map(|(d, c)| json!({ "date": d, "count": c }))
        .collect();

    // --- deck composition (spec §2 bucket table) ----------------------------
    let (learning, maturing, mastered, struggling, banished): (i64, i64, i64, i64, i64) =
        sqlx::query_as(
            "SELECT
               COUNT(*) FILTER (WHERE NOT suspended AND lapses < 4 AND state <> 'review'),
               COUNT(*) FILTER (WHERE NOT suspended AND lapses < 4 AND state = 'review' AND interval_days < 21),
               COUNT(*) FILTER (WHERE NOT suspended AND lapses < 4 AND state = 'review' AND interval_days >= 21),
               COUNT(*) FILTER (WHERE NOT suspended AND lapses >= 4),
               COUNT(*) FILTER (WHERE suspended)
             FROM pavlov_cards WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_one(&state.pool)
        .await?;
    let deck_json_total = learning + maturing + mastered + struggling + banished;
    let touched: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pavlov_cards ca
         WHERE ca.user_id = $1",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;

    // Keep in sync with routes/practice.rs (same upsert + baseline rule).
    // Snapshot today (user-local date) and diff against a baseline: newest
    // snapshot at least a week old, else the oldest one before today.
    sqlx::query(
        "INSERT INTO pavlov_deck_snapshots (user_id, snap_date, learning, maturing, mastered, struggling)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (user_id, snap_date) DO UPDATE SET
           learning = EXCLUDED.learning, maturing = EXCLUDED.maturing,
           mastered = EXCLUDED.mastered, struggling = EXCLUDED.struggling",
    )
    .bind(user_id)
    .bind(today)
    .bind(learning as i32)
    .bind(maturing as i32)
    .bind(mastered as i32)
    .bind(struggling as i32)
    .execute(&state.pool)
    .await?;

    let baseline: Option<(NaiveDate, i32, i32, i32, i32)> = sqlx::query_as(
        "SELECT snap_date, learning, maturing, mastered, struggling FROM pavlov_deck_snapshots
         WHERE user_id = $1 AND snap_date <= $2::date - 7
         ORDER BY snap_date DESC LIMIT 1",
    )
    .bind(user_id)
    .bind(today)
    .fetch_optional(&state.pool)
    .await?;
    let baseline = match baseline {
        Some(b) => Some(b),
        None => {
            sqlx::query_as(
                "SELECT snap_date, learning, maturing, mastered, struggling FROM pavlov_deck_snapshots
                 WHERE user_id = $1 AND snap_date < $2
                 ORDER BY snap_date ASC LIMIT 1",
            )
            .bind(user_id)
            .bind(today)
            .fetch_optional(&state.pool)
            .await?
        }
    };
    let delta = baseline.map(|(since, l, y, m, s)| {
        json!({
            "since": since,
            "learning": learning - l as i64,
            "maturing": maturing - y as i64,
            "mastered": mastered - m as i64,
            "struggling": struggling - s as i64,
        })
    });

    // --- progress toward the target date ------------------------------------
    let deck_total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pavlov_answers")
        .fetch_one(&state.pool)
        .await?;
    let window_start = day_start - Duration::days(13); // today + 13 prior local days
    let created_14d: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pavlov_cards ca JOIN pavlov_answers pa ON pa.id = ca.answer_id
         WHERE ca.user_id = $1 AND ca.created_at >= $2",
    )
    .bind(user_id)
    .bind(window_start)
    .fetch_one(&state.pool)
    .await?;
    let (hooks_seen, hooks_total): (i64, i64) = sqlx::query_as(
        "SELECT
           (SELECT count(DISTINCT pr.hook_id) FROM pavlov_reviews pr
             JOIN pavlov_hooks h ON h.id = pr.hook_id
             JOIN pavlov_cards ca ON ca.answer_id = h.answer_id
               AND ca.user_id = $1 AND ca.last_review IS NOT NULL
             WHERE pr.user_id = $1 AND h.status = 'active' AND h.cue IS NOT NULL),
           (SELECT count(*) FROM pavlov_hooks h
             JOIN pavlov_cards ca ON ca.answer_id = h.answer_id
             WHERE ca.user_id = $1 AND ca.last_review IS NOT NULL
               AND h.status = 'active' AND h.cue IS NOT NULL)",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
    let progress = compute_progress(deck_total, touched, created_14d, today, target_date, hooks_seen, hooks_total);

    Ok(Json(json!({
        "overall": pack(all.total, all.correct),
        "cold": pack(all.cold_total, all.cold_correct),
        "review": pack(all.review_total, all.review_correct),
        "cold30d": pack(c30_t, c30_c),
        "historySince": history_since,
        "dailyAccuracy": daily_accuracy,
        "categoryBreakdown": category_breakdown,
        "reviewedToday": reviewed_today,
        "dueCount": due_count,
        "newRemaining": new_remaining,
        "forecast": forecast_json,
        "deck": {
            "learning": learning,
            "maturing": maturing,
            "mastered": mastered,
            "struggling": struggling,
            "banished": banished,
            "total": deck_json_total,
            "delta": delta,
        },
        "progress": progress,
    })))
}
