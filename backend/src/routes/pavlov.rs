use axum::{extract::State, Json};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

pub async fn generate(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    if auth.role != "admin" {
        return Err(AppError::Forbidden("Admin access required".into()));
    }
    if state.config.openai_api_key.is_empty() {
        return Err(AppError::BadRequest("OPENAI_API_KEY not configured".into()));
    }
    if state
        .pavlov_inflight
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(Json(json!({ "started": false, "running": true })));
    }
    let st = state.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::pavlov::run_generation(&st).await {
            tracing::error!("pavlov generation failed (resumable — rerun to continue): {e:?}");
        }
        st.pavlov_inflight.store(false, Ordering::SeqCst);
    });
    Ok(Json(json!({ "started": true })))
}

pub async fn status(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    if auth.role != "admin" {
        return Err(AppError::Forbidden("Admin access required".into()));
    }
    let counts: Vec<(String, i64)> =
        sqlx::query_as("SELECT status, count(*) FROM pavlov_cues GROUP BY status")
            .fetch_all(&state.pool)
            .await?;
    let get = |k: &str| counts.iter().find(|(s, _)| s == k).map(|(_, n)| *n).unwrap_or(0);
    Ok(Json(json!({
        "running": state.pavlov_inflight.load(Ordering::SeqCst),
        "pending": get("pending"),
        "active": get("active"),
        "dropped": get("dropped"),
    })))
}

use axum::extract::Path;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;

use crate::answer_match;
use crate::blend::TARGET_WEIGHTS;
use crate::routes::practice::{day_start_utc, serve_new};
use crate::srs::{schedule, CardKind, Prev, Rating};

const LEECH_LAPSES: i32 = 8; // same threshold as practice.rs

fn category_rank(cat: &str) -> usize {
    TARGET_WEIGHTS
        .iter()
        .position(|(c, _)| *c == cat)
        .unwrap_or(TARGET_WEIGHTS.len())
}

#[derive(sqlx::FromRow)]
struct AnswerListRow {
    id: i32,
    answer: String,
    answer_norm: String,
    meta_category: String,
    phrases: Vec<String>,
    phrase_tiers: Vec<String>,
    score: f32,
    suspended: bool,
}

pub async fn answers(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let mut rows: Vec<AnswerListRow> = sqlx::query_as(
        "SELECT pa.id, pa.answer, pa.answer_norm, pa.meta_category, pa.phrases,
                pa.phrase_tiers, pa.score,
                COALESCE(ca.suspended, false) AS suspended
         FROM pavlov_answers pa
         LEFT JOIN pavlov_cards ca ON ca.answer_id = pa.id AND ca.user_id = $1",
    )
    .bind(auth.user_id)
    .fetch_all(&state.pool)
    .await?;
    rows.sort_by(|a, b| {
        category_rank(&a.meta_category)
            .cmp(&category_rank(&b.meta_category))
            .then(b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal))
    });

    // Per-phrase evidence for the listing (one query, mapped client-side).
    let ev: Vec<(String, String, String, i32, i32, f32)> = sqlx::query_as(
        "SELECT answer_norm, cue_display, tier, support, total, prec
         FROM pavlov_cues WHERE status = 'active'",
    )
    .fetch_all(&state.pool)
    .await?;
    use std::collections::HashMap;
    let mut ev_map: HashMap<(String, String), (String, i32, i32, f32)> = HashMap::new();
    for (norm, display, tier, support, total, prec) in ev {
        ev_map.insert((norm, display), (tier, support, total, prec));
    }

    let answers: Vec<Value> = rows
        .into_iter()
        .map(|r| {
            let phrases: Vec<Value> = r
                .phrases
                .iter()
                .zip(r.phrase_tiers.iter())
                .map(|(text, tier)| {
                    let key = (r.answer_norm.clone(), text.clone());
                    match ev_map.get(&key) {
                        Some((_, support, total, prec)) => json!({
                            "text": text, "tier": tier,
                            "support": support, "total": total, "precision": prec,
                        }),
                        None => json!({ "text": text, "tier": tier }),
                    }
                })
                .collect();
            json!({
                "id": r.id, "answer": r.answer, "category": r.meta_category,
                "phrases": phrases, "suspended": r.suspended,
            })
        })
        .collect();
    Ok(Json(json!({ "answers": answers })))
}

#[derive(Deserialize)]
pub struct SuspendBody {
    pub suspended: bool,
}

pub async fn suspend(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(answer_id): Path<i32>,
    Json(body): Json<SuspendBody>,
) -> Result<Json<Value>, AppError> {
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pavlov_answers WHERE id = $1)")
            .bind(answer_id)
            .fetch_one(&state.pool)
            .await?;
    if !exists {
        return Err(AppError::NotFound("No such card".into()));
    }
    sqlx::query(
        "INSERT INTO pavlov_cards (user_id, answer_id, suspended) VALUES ($1, $2, $3)
         ON CONFLICT (user_id, answer_id) DO UPDATE SET suspended = EXCLUDED.suspended",
    )
    .bind(auth.user_id)
    .bind(answer_id)
    .bind(body.suspended)
    .execute(&state.pool)
    .await?;
    Ok(Json(json!({ "suspended": body.suspended })))
}

#[derive(sqlx::FromRow)]
struct DrillAnswerRow {
    id: i32,
    phrases: Vec<String>,
    phrase_tiers: Vec<String>,
    meta_category: String,
}

fn drill_card_json(r: DrillAnswerRow) -> Value {
    let phrases: Vec<Value> = r
        .phrases
        .iter()
        .zip(r.phrase_tiers.iter())
        .map(|(text, tier)| json!({ "text": text, "tier": tier }))
        .collect();
    json!({ "answerId": r.id, "phrases": phrases, "category": r.meta_category })
}

pub async fn drill_next(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    // extra=true: user chose to keep drilling past the daily new-card
    // allowance (spec amendment 2026-07-23). Due reviews still serve first;
    // newRemaining keeps reporting the true value.
    let extra = params.get("extra").map(|v| v == "true").unwrap_or(false);
    let (new_per_day, tz): (i32, Option<String>) =
        sqlx::query_as("SELECT new_cards_per_day, timezone FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&state.pool)
            .await?;

    let due_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pavlov_cards ca
         JOIN pavlov_answers pa ON pa.id = ca.answer_id
         WHERE ca.user_id = $1 AND ca.suspended = false AND ca.due <= now()",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;

    let day_start = day_start_utc(Utc::now(), tz.as_deref());
    // `suspend` can create a pavlov_cards row (to persist the suspended flag)
    // for a cue the user never actually drilled — that row's last_review stays
    // NULL. Only rows created via `grade` (which always sets last_review) count
    // as introduced new cards, so exclude last_review IS NULL rows here.
    let new_today: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pavlov_cards WHERE user_id = $1 AND created_at >= $2 AND last_review IS NOT NULL",
    )
    .bind(user_id)
    .bind(day_start)
    .fetch_one(&state.pool)
    .await?;
    let new_remaining = (new_per_day as i64 - new_today).max(0);

    let want_new = {
        use rand::Rng;
        serve_new(new_remaining, due_count, rand::rng().random())
    };

    let pick_new = "SELECT id, phrases, phrase_tiers, meta_category FROM pavlov_answers
         WHERE id NOT IN (SELECT answer_id FROM pavlov_cards WHERE user_id = $1)
         ORDER BY -ln(random()) / ln(1 + score) LIMIT 1";
    let fetch_due = "SELECT pa.id, pa.phrases, pa.phrase_tiers, pa.meta_category
         FROM pavlov_cards ca
         JOIN pavlov_answers pa ON pa.id = ca.answer_id
         WHERE ca.user_id = $1 AND ca.suspended = false AND ca.due <= now()
         ORDER BY ca.due ASC LIMIT 1";

    if want_new {
        if let Some(row) = sqlx::query_as::<_, DrillAnswerRow>(pick_new)
            .bind(user_id)
            .fetch_optional(&state.pool)
            .await?
        {
            return Ok(Json(json!({
                "done": false, "isNew": true, "card": drill_card_json(row),
                "dueCount": due_count, "newRemaining": new_remaining,
            })));
        }
    }
    if let Some(row) = sqlx::query_as::<_, DrillAnswerRow>(fetch_due)
        .bind(user_id)
        .fetch_optional(&state.pool)
        .await?
    {
        return Ok(Json(json!({
            "done": false, "isNew": false, "card": drill_card_json(row),
            "dueCount": due_count, "newRemaining": new_remaining,
        })));
    }
    if new_remaining > 0 || extra {
        if let Some(row) = sqlx::query_as::<_, DrillAnswerRow>(pick_new)
            .bind(user_id)
            .fetch_optional(&state.pool)
            .await?
        {
            return Ok(Json(json!({
                "done": false, "isNew": true, "card": drill_card_json(row),
                "dueCount": due_count, "newRemaining": new_remaining,
            })));
        }
    }

    let next_due_at: Option<DateTime<Utc>> = sqlx::query_scalar(
        "SELECT min(due) FROM pavlov_cards WHERE user_id = $1 AND suspended = false AND due > now()",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
    let due_soon_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pavlov_cards
         WHERE user_id = $1 AND suspended = false
           AND due > now() AND due <= now() + interval '60 minutes'",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
    // Unseen cards still exist → the frontend can offer "Keep going".
    let more_new_available: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pavlov_answers pa
         WHERE pa.id NOT IN (SELECT answer_id FROM pavlov_cards WHERE user_id = $1))",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(json!({
        "done": true, "dueCount": due_count, "newRemaining": new_remaining,
        "nextDueAt": next_due_at, "dueSoonCount": due_soon_count,
        "moreNewAvailable": more_new_available,
    })))
}

/// Lightweight status for the dashboard tile: due/new counts + deck size.
pub async fn status_user(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let (new_per_day, tz): (i32, Option<String>) =
        sqlx::query_as("SELECT new_cards_per_day, timezone FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&state.pool)
            .await?;
    let due_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pavlov_cards ca
         JOIN pavlov_answers pa ON pa.id = ca.answer_id
         WHERE ca.user_id = $1 AND ca.suspended = false AND ca.due <= now()",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
    let day_start = day_start_utc(Utc::now(), tz.as_deref());
    let new_today: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pavlov_cards WHERE user_id = $1 AND created_at >= $2 AND last_review IS NOT NULL",
    )
    .bind(user_id)
    .bind(day_start)
    .fetch_one(&state.pool)
    .await?;
    let total_cards: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pavlov_answers")
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(json!({
        "dueCount": due_count,
        "newRemaining": (new_per_day as i64 - new_today).max(0),
        "totalCards": total_cards,
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckBody {
    pub answer_id: i32,
    /// Optional: honesty-mode reveal sends no typed answer.
    pub typed: Option<String>,
}

/// Reveal the answer (optionally grading a typed attempt) — no SRS state
/// change (that's `grade`).
pub async fn drill_check(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(body): Json<CheckBody>,
) -> Result<Json<Value>, AppError> {
    let row: Option<(String, Vec<i32>)> = sqlx::query_as(
        "SELECT answer, example_clue_ids FROM pavlov_answers WHERE id = $1",
    )
    .bind(body.answer_id)
    .fetch_optional(&state.pool)
    .await?;
    let (answer, example_ids) = row.ok_or_else(|| AppError::NotFound("No such cue".into()))?;
    let correct = body.typed.as_deref().map(|t| answer_match::is_correct(t, &answer));

    let examples: Vec<(String, Option<String>, Option<chrono::NaiveDate>)> = sqlx::query_as(
        "SELECT coalesce(answer, ''), category, air_date FROM jeopardy_questions
         WHERE id = ANY($1) ORDER BY air_date DESC",
    )
    .bind(&example_ids[..])
    .fetch_all(&state.pool)
    .await?;
    let examples: Vec<Value> = examples
        .into_iter()
        .map(|(clue, category, air_date)| {
            json!({ "clue": clue, "category": category, "airDate": air_date })
        })
        .collect();
    Ok(Json(json!({ "correct": correct, "answer": answer, "examples": examples })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrillGradeBody {
    pub answer_id: i32,
    pub rating: String,
}

#[derive(sqlx::FromRow)]
struct PavlovCardRow {
    state: String,
    interval_days: f64,
    ease: f64,
    reps: i32,
    lapses: i32,
    step_index: i16,
}

/// SM-2 schedule for a cue card. Deliberately does NOT touch question_attempts
/// or quiz_sessions — cue reps are not clue attempts (spec §3).
pub async fn drill_grade(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<DrillGradeBody>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let rating = Rating::from_wire(&body.rating)
        .ok_or_else(|| AppError::BadRequest("rating must be wrong|got_it|too_easy".into()))?;

    let existing: Option<PavlovCardRow> = sqlx::query_as(
        "SELECT state, interval_days, ease, reps, lapses, step_index
         FROM pavlov_cards WHERE user_id = $1 AND answer_id = $2",
    )
    .bind(user_id)
    .bind(body.answer_id)
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
        "INSERT INTO pavlov_cards
           (user_id, answer_id, state, interval_days, ease, due, last_review, reps, lapses, step_index, suspended)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
         ON CONFLICT (user_id, answer_id) DO UPDATE SET
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
    .bind(body.answer_id)
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
        "state": out.state.as_str(),
        "due": due,
        "intervalDays": out.interval_days,
        "requeueInSession": out.requeue_in_session,
    })))
}
