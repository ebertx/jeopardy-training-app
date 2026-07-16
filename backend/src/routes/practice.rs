use axum::{extract::State, Json};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::srs::{schedule, CardKind, Prev, Rating};
use crate::adaptive::{compute_weights, sample_category, CategoryStat};
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

    // Record the attempt. attempt_kind is decided server-side: first-ever attempt
    // at this question = 'new' (cold), anything later = 'review'.
    let prior: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM question_attempts WHERE user_id = $1 AND question_id = $2)",
    )
    .bind(user_id)
    .bind(body.question_id)
    .fetch_one(&state.pool)
    .await?;
    let kind = if prior { "review" } else { "new" };
    sqlx::query(
        "INSERT INTO question_attempts (session_id, question_id, user_id, correct, attempt_kind) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(session_id)
    .bind(body.question_id)
    .bind(user_id)
    .bind(rating.is_correct())
    .bind(kind)
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

use axum::extract::Query;
use std::collections::HashMap;

/// Start of "today" in the user's IANA timezone, as a UTC instant.
/// Pure (takes `now`) so it can be unit-tested. Falls back to UTC midnight when
/// tz is missing or unparseable.
pub fn day_start_utc(now: DateTime<Utc>, tz: Option<&str>) -> DateTime<Utc> {
    use chrono::TimeZone;
    let zone: chrono_tz::Tz = tz.and_then(|s| s.parse().ok()).unwrap_or(chrono_tz::UTC);
    let local_now = now.with_timezone(&zone);
    let local_midnight = local_now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("valid midnight");
    zone.from_local_datetime(&local_midnight)
        .earliest()
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or(now)
}

/// Interleave decision: with both new allowance and due reviews available, pick a
/// new card with probability new/(new+due) so new cards spread through the day
/// instead of queueing behind every review. Pure (takes `roll`) for testability.
pub fn serve_new(new_remaining: i64, due_count: i64, roll: f64) -> bool {
    if new_remaining <= 0 {
        return false;
    }
    if due_count <= 0 {
        return true;
    }
    roll < new_remaining as f64 / (new_remaining + due_count) as f64
}

#[derive(sqlx::FromRow)]
pub(crate) struct ClueRow {
    pub(crate) id: i32,
    question: Option<String>,
    answer: Option<String>,
    category: Option<String>,
    classifier_category: Option<String>,
    clue_value: Option<i32>,
    round: Option<i32>,
    air_date: Option<chrono::NaiveDate>,
    notes: Option<String>,
}

/// Fire-and-forget insight pregeneration: by the time the user reads,
/// reveals, and grades the card, the insight is cached. No-op when the
/// OpenAI key is unconfigured; failures are logged and swallowed.
pub(crate) fn pregenerate_insight(state: &Arc<AppState>, question_id: i32) {
    if state.config.openai_api_key.is_empty() {
        return;
    }
    let st = state.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::insights::ensure_insight(&st, question_id).await {
            tracing::warn!("insight pregeneration failed for {question_id}: {e:?}");
        }
    });
}

pub(crate) fn clue_json(row: ClueRow) -> Value {
    json!({
        "id": row.id,
        "question": row.question,
        "answer": row.answer,
        "category": row.category,
        "classifier_category": row.classifier_category,
        "clue_value": row.clue_value,
        "round": row.round,
        "air_date": row.air_date,
        "notes": row.notes,
    })
}

pub async fn next(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;

    // User prefs.
    let (new_per_day, tz, adaptive): (i32, Option<String>, bool) =
        sqlx::query_as("SELECT new_cards_per_day, timezone, adaptive_targeting FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&state.pool)
            .await?;

    // Due review count (unsuspended, due now).
    let due_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM srs_cards
         WHERE user_id = $1 AND suspended = false AND due <= now()",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;

    // New cards introduced since local midnight.
    let day_start = day_start_utc(Utc::now(), tz.as_deref());
    let new_today: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM srs_cards WHERE user_id = $1 AND created_at >= $2",
    )
    .bind(user_id)
    .bind(day_start)
    .fetch_one(&state.pool)
    .await?;
    let new_remaining = (new_per_day as i64 - new_today).max(0);

    // Interleave: decide new-vs-review first, then fall back to the other if the
    // chosen source comes up empty. Net behavior when only one source has items
    // is identical to the old strict priority.
    let want_new = {
        use rand::Rng;
        serve_new(new_remaining, due_count, rand::rng().random())
    };

    if want_new {
        if let Some(row) = pick_new_clue(&state, user_id, adaptive, &params).await? {
            pregenerate_insight(&state, row.id);
            return Ok(Json(json!({
                "done": false, "isNew": true, "card": clue_json(row),
                "dueCount": due_count, "newRemaining": new_remaining,
            })));
        }
    }

    if let Some(row) = fetch_review(&state, user_id).await? {
        pregenerate_insight(&state, row.id);
        return Ok(Json(json!({
            "done": false, "isNew": false, "card": clue_json(row),
            "dueCount": due_count, "newRemaining": new_remaining,
        })));
    }

    // Review pool empty (or roll chose review with none due) — try a new clue.
    if new_remaining > 0 {
        if let Some(row) = pick_new_clue(&state, user_id, adaptive, &params).await? {
            pregenerate_insight(&state, row.id);
            return Ok(Json(json!({
                "done": false, "isNew": true, "card": clue_json(row),
                "dueCount": due_count, "newRemaining": new_remaining,
            })));
        }
    }

    // 3) Nothing to do right now. Tell the client when work resumes so it can
    // be honest about learning-step cards landing in a few minutes.
    let next_due_at: Option<DateTime<Utc>> = sqlx::query_scalar(
        "SELECT min(due) FROM srs_cards WHERE user_id = $1 AND suspended = false AND due > now()",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
    let due_soon_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM srs_cards
         WHERE user_id = $1 AND suspended = false
           AND due > now() AND due <= now() + interval '60 minutes'",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(json!({
        "done": true, "dueCount": due_count, "newRemaining": new_remaining,
        "nextDueAt": next_due_at, "dueSoonCount": due_soon_count,
    })))
}

/// Highest-priority due review card (unsuspended, due now), if any.
async fn fetch_review(
    state: &Arc<AppState>,
    user_id: i32,
) -> Result<Option<ClueRow>, sqlx::Error> {
    sqlx::query_as::<_, ClueRow>(
        "SELECT jq.id, jq.question, jq.answer, jq.category, jq.classifier_category,
                jq.clue_value, jq.round, jq.air_date, jq.notes
         FROM srs_cards sc
         JOIN jeopardy_questions jq ON jq.id = sc.question_id
         WHERE sc.user_id = $1 AND sc.suspended = false AND sc.due <= now()
           AND jq.archived = false
         ORDER BY sc.due ASC
         LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
}

/// Per-category (attempts, correct) for the adaptive window: last 180 days,
/// falling back to all-time when the window holds < 200 attempts. The second
/// element names the window actually used ("180d" or "all") for display.
async fn adaptive_category_stats(
    state: &Arc<AppState>,
    user_id: i32,
) -> Result<(Vec<CategoryStat>, &'static str), AppError> {
    const WINDOWED_SQL: &str = "SELECT jq.classifier_category, COUNT(*)::bigint, \
             SUM((qa.correct)::int)::bigint \
         FROM question_attempts qa \
         JOIN jeopardy_questions jq ON jq.id = qa.question_id \
         WHERE qa.user_id = $1 AND jq.classifier_category IS NOT NULL \
           AND qa.answered_at >= now() - interval '180 days' \
         GROUP BY jq.classifier_category";
    const ALL_TIME_SQL: &str = "SELECT jq.classifier_category, COUNT(*)::bigint, \
             SUM((qa.correct)::int)::bigint \
         FROM question_attempts qa \
         JOIN jeopardy_questions jq ON jq.id = qa.question_id \
         WHERE qa.user_id = $1 AND jq.classifier_category IS NOT NULL \
         GROUP BY jq.classifier_category";

    let windowed: Vec<(String, i64, i64)> = sqlx::query_as(WINDOWED_SQL)
        .bind(user_id)
        .fetch_all(&state.pool)
        .await?;
    let windowed_total: i64 = windowed.iter().map(|r| r.1).sum();

    let (rows, window) = if windowed_total < 200 {
        let all_time: Vec<(String, i64, i64)> = sqlx::query_as(ALL_TIME_SQL)
            .bind(user_id)
            .fetch_all(&state.pool)
            .await?;
        (all_time, "all")
    } else {
        (windowed, "180d")
    };

    Ok((
        rows.into_iter()
            .map(|(category, attempts, correct)| CategoryStat { category, attempts, correct })
            .collect(),
        window,
    ))
}

async fn pick_with_filters(
    state: &Arc<AppState>,
    user_id: i32,
    category: &str, // "all" or a classifier category
    params: &HashMap<String, String>,
) -> Result<Option<ClueRow>, AppError> {
    let game_types_str = params.get("gameTypes").map(|s| s.as_str()).unwrap_or("");
    let game_types: Vec<&str> = game_types_str
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    let mut conditions = vec![
        "question IS NOT NULL".to_string(),
        "answer IS NOT NULL".to_string(),
        "classifier_category IS NOT NULL".to_string(),
        "air_date IS NOT NULL".to_string(),
        "archived = false".to_string(),
        // Exclude clues already in this user's SRS pool.
        "id NOT IN (SELECT question_id FROM srs_cards WHERE user_id = $1)".to_string(),
    ];

    let use_category = category != "all";
    if use_category {
        conditions.push("classifier_category = $2".to_string());
    }
    for gt in &game_types {
        match *gt {
            "kids" | "Kids" => conditions
                .push("NOT (notes ILIKE '%Kids%' OR notes ILIKE '%Kid''s%')".to_string()),
            "teen" | "Teen" => conditions.push("NOT (notes ILIKE '%Teen%')".to_string()),
            "college" | "College" => conditions.push("NOT (notes ILIKE '%College%')".to_string()),
            _ => {}
        }
    }
    let where_clause = conditions.join(" AND ");

    let count_sql = format!("SELECT COUNT(*) FROM jeopardy_questions WHERE {}", where_clause);
    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql).bind(user_id);
    if use_category {
        count_q = count_q.bind(category);
    }
    let total: i64 = count_q.fetch_one(&state.pool).await?;
    if total == 0 {
        return Ok(None);
    }

    // Same recency-biased exponential offset used by the legacy quiz picker.
    use rand::Rng;
    let r: f64 = rand::rng().random();
    let lambda = 3.5_f64;
    let normalized = (-(1.0 - r).ln() / lambda).min(1.0);
    let offset = (normalized * total as f64).floor() as i64;

    let sql = format!(
        "SELECT id, question, answer, category, classifier_category, clue_value, round, air_date, notes
         FROM jeopardy_questions WHERE {} ORDER BY air_date DESC LIMIT 1 OFFSET {}",
        where_clause, offset
    );
    let mut q = sqlx::query_as::<_, ClueRow>(&sql).bind(user_id);
    if use_category {
        q = q.bind(category);
    }
    Ok(q.fetch_optional(&state.pool).await?)
}

/// Strategy wrapper: 60% of pulls (when no manual filter and the user's toggle
/// is on) sample a category by weakness weight first; 40% — and all filtered or
/// toggled-off pulls — behave exactly as before. A weighted pick that finds no
/// eligible clue falls back to unconstrained.
async fn pick_new_clue(
    state: &Arc<AppState>,
    user_id: i32,
    adaptive: bool,
    params: &HashMap<String, String>,
) -> Result<Option<ClueRow>, AppError> {
    let manual_category = params.get("category").map(|s| s.as_str()).unwrap_or("all");

    if manual_category == "all" && adaptive {
        use rand::Rng;
        let roll: f64 = rand::rng().random();
        if roll >= 0.4 {
            let (stats, _) = adaptive_category_stats(state, user_id).await?;
            let weights = compute_weights(&stats);
            let r: f64 = rand::rng().random();
            if let Some(cat) = sample_category(&weights, r) {
                let cat = cat.to_string();
                if let Some(row) = pick_with_filters(state, user_id, &cat, params).await? {
                    return Ok(Some(row));
                }
                // Weighted category exhausted — fall through to unconstrained.
            }
        }
    }

    pick_with_filters(state, user_id, manual_category, params).await
}

pub async fn status(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;

    let (new_per_day, tz, adaptive): (i32, Option<String>, bool) =
        sqlx::query_as("SELECT new_cards_per_day, timezone, adaptive_targeting FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&state.pool)
            .await?;

    let due_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM srs_cards WHERE user_id = $1 AND suspended = false AND due <= now()",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;

    let day_start = day_start_utc(Utc::now(), tz.as_deref());
    let new_today: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM srs_cards WHERE user_id = $1 AND created_at >= $2",
    )
    .bind(user_id)
    .bind(day_start)
    .fetch_one(&state.pool)
    .await?;
    let new_remaining = (new_per_day as i64 - new_today).max(0);

    let reviewed_today: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM question_attempts WHERE user_id = $1 AND answered_at >= $2",
    )
    .bind(user_id)
    .bind(day_start)
    .fetch_one(&state.pool)
    .await?;

    // 14-day due forecast (calendar day in UTC; good enough for a bar chart).
    // Overdue cards fold into today's bucket so the chart never shows past dates.
    let forecast: Vec<(chrono::NaiveDate, i64)> = sqlx::query_as(
        "SELECT GREATEST((due AT TIME ZONE 'UTC')::date, (now() AT TIME ZONE 'UTC')::date) AS d, COUNT(*)
         FROM srs_cards
         WHERE user_id = $1 AND suspended = false
           AND due < now() + interval '14 days'
         GROUP BY d ORDER BY d",
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await?;
    let forecast_json: Vec<Value> = forecast
        .into_iter()
        .map(|(d, c)| json!({ "date": d, "count": c }))
        .collect();

    let (adaptive_weights, adaptive_window): (Vec<Value>, Option<&'static str>) = if adaptive {
        let (stats, window) = adaptive_category_stats(&state, user_id).await?;
        let weights = compute_weights(&stats)
            .into_iter()
            .map(|w| {
                json!({
                    "category": w.category,
                    "attempts": w.attempts,
                    "accuracy": w.accuracy,
                    "weight": w.weight,
                })
            })
            .collect();
        (weights, Some(window))
    } else {
        (vec![], None)
    };

    // Deck composition for the dashboard: four mutually exclusive buckets
    // (struggling wins so it stays actionable) that sum to the active deck.
    // Same definitions as /api/cards.
    let deck: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT \
           COUNT(*) FILTER (WHERE NOT (sc.suspended OR sc.lapses >= 4) AND sc.state IN ('learning','relearning')), \
           COUNT(*) FILTER (WHERE NOT (sc.suspended OR sc.lapses >= 4) AND sc.state = 'review' AND sc.interval_days < 21), \
           COUNT(*) FILTER (WHERE NOT (sc.suspended OR sc.lapses >= 4) AND sc.state = 'review' AND sc.interval_days >= 21), \
           COUNT(*) FILTER (WHERE sc.suspended OR sc.lapses >= 4) \
         FROM srs_cards sc \
         JOIN jeopardy_questions jq ON jq.id = sc.question_id \
         WHERE sc.user_id = $1 AND jq.archived = false",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
    let (learning, maturing, mastered, struggling) = deck;

    // Upsert today's snapshot (user-local date), then diff against a baseline:
    // newest snapshot at least a week old, else the oldest one before today.
    let zone: chrono_tz::Tz = tz
        .as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(chrono_tz::UTC);
    let today = Utc::now().with_timezone(&zone).date_naive();
    sqlx::query(
        "INSERT INTO srs_deck_snapshots (user_id, snap_date, learning, maturing, mastered, struggling) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (user_id, snap_date) DO UPDATE SET \
           learning = EXCLUDED.learning, maturing = EXCLUDED.maturing, \
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

    let baseline: Option<(chrono::NaiveDate, i32, i32, i32, i32)> = sqlx::query_as(
        "SELECT snap_date, learning, maturing, mastered, struggling FROM srs_deck_snapshots \
         WHERE user_id = $1 AND snap_date <= $2::date - 7 \
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
                "SELECT snap_date, learning, maturing, mastered, struggling FROM srs_deck_snapshots \
                 WHERE user_id = $1 AND snap_date < $2 \
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

    Ok(Json(json!({
        "dueCount": due_count,
        "newRemaining": new_remaining,
        "reviewedToday": reviewed_today,
        "forecast": forecast_json,
        "adaptiveWeights": adaptive_weights,
        "adaptiveWindow": adaptive_window,
        "deck": {
            "learning": learning,
            "maturing": maturing,
            "mastered": mastered,
            "struggling": struggling,
            "total": learning + maturing + mastered + struggling,
            "delta": delta,
        },
    })))
}

#[cfg(test)]
mod tests {
    use super::day_start_utc;
    use super::serve_new;
    use chrono::{TimeZone, Utc};

    #[test]
    fn chicago_day_start_is_local_midnight_in_utc() {
        // 2026-06-30 12:00 UTC; Chicago is UTC-5 (CDT) in summer → local midnight = 05:00 UTC.
        let now = Utc.with_ymd_and_hms(2026, 6, 30, 12, 0, 0).unwrap();
        let ds = day_start_utc(now, Some("America/Chicago"));
        assert_eq!(ds, Utc.with_ymd_and_hms(2026, 6, 30, 5, 0, 0).unwrap());
    }

    #[test]
    fn unknown_or_missing_tz_falls_back_to_utc_midnight() {
        let now = Utc.with_ymd_and_hms(2026, 6, 30, 12, 0, 0).unwrap();
        assert_eq!(
            day_start_utc(now, Some("Not/AZone")),
            Utc.with_ymd_and_hms(2026, 6, 30, 0, 0, 0).unwrap()
        );
        assert_eq!(
            day_start_utc(now, None),
            Utc.with_ymd_and_hms(2026, 6, 30, 0, 0, 0).unwrap()
        );
    }

    #[test]
    fn serve_new_boundaries() {
        assert!(!serve_new(0, 5, 0.0));           // no allowance → never new
        assert!(serve_new(3, 0, 0.99));           // no reviews due → always new
        assert!(!serve_new(0, 0, 0.5));           // nothing available → not new
    }

    #[test]
    fn serve_new_is_proportional() {
        // p(new) = 10/(10+30) = 0.25
        assert!(serve_new(10, 30, 0.24));
        assert!(!serve_new(10, 30, 0.26));
    }
}
