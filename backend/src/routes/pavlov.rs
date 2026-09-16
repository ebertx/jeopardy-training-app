use axum::{extract::State, Json};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

/// Admin-only, single-flight (shared `pavlov_inflight` flag for generate /
/// resolve / hooks), fire-and-forget. `needs_key` gates jobs that call OpenAI.
async fn spawn_admin_job<F, Fut>(
    state: Arc<AppState>,
    auth: &AuthUser,
    name: &'static str,
    needs_key: bool,
    job: F,
) -> Result<Json<Value>, AppError>
where
    F: FnOnce(Arc<AppState>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<(), AppError>> + Send + 'static,
{
    if auth.role != "admin" {
        return Err(AppError::Forbidden("Admin access required".into()));
    }
    if needs_key && state.config.openai_api_key.is_empty() {
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
        if let Err(e) = job(st.clone()).await {
            tracing::error!("pavlov {name} failed (resumable — rerun to continue): {e:?}");
        }
        st.pavlov_inflight.store(false, Ordering::SeqCst);
    });
    Ok(Json(json!({ "started": true })))
}

pub async fn generate(State(state): State<Arc<AppState>>, auth: AuthUser) -> Result<Json<Value>, AppError> {
    spawn_admin_job(state, &auth, "generation", true, |st| async move {
        crate::pavlov::run_generation(&st).await
    })
    .await
}

pub async fn resolve(State(state): State<Arc<AppState>>, auth: AuthUser) -> Result<Json<Value>, AppError> {
    spawn_admin_job(state, &auth, "resolve", false, |st| async move {
        crate::objects::run_resolve(&st).await
    })
    .await
}

pub async fn hooks(State(state): State<Arc<AppState>>, auth: AuthUser) -> Result<Json<Value>, AppError> {
    spawn_admin_job(state, &auth, "hooks", false, |st| async move {
        crate::objects::run_hooks(&st).await
    })
    .await
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

    let (total, labeled, vetted): (i64, i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE status = 'active'),
                count(*) FILTER (WHERE status = 'active' AND cue IS NOT NULL),
                count(*) FILTER (WHERE status = 'active' AND source IN ('vetted', 'both'))
         FROM pavlov_hooks",
    )
    .fetch_one(&state.pool)
    .await?;
    let entities_pending: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pavlov_answers WHERE hooks_built_at IS NULL")
            .fetch_one(&state.pool)
            .await?;

    Ok(Json(json!({
        "running": state.pavlov_inflight.load(Ordering::SeqCst),
        "pending": get("pending"),
        "active": get("active"),
        "dropped": get("dropped"),
        "hooks": {
            "total": total,
            "labeled": labeled,
            "unlabeled": total - labeled,
            "vetted": vetted,
            "entitiesPending": entities_pending,
        },
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
    forms: Vec<String>,
    vetted: bool,
}

pub async fn answers(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let mut rows: Vec<AnswerListRow> = sqlx::query_as(
        "SELECT pa.id, pa.answer, pa.answer_norm, pa.meta_category, pa.phrases,
                pa.phrase_tiers, pa.score, pa.forms, pa.vetted,
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

    #[derive(sqlx::FromRow)]
    struct HookListRow { id: i32, answer_id: i32, rank: i32, cue: Option<String>, key_gram: String, support: i32, source: String, status: String }
    let hook_rows: Vec<HookListRow> = sqlx::query_as(
        "SELECT id, answer_id, rank, cue, key_gram, support, source, status FROM pavlov_hooks ORDER BY answer_id, rank, id",
    )
    .fetch_all(&state.pool)
    .await?;
    let mut hooks_by_answer: HashMap<i32, Vec<Value>> = HashMap::new();
    for h in hook_rows {
        hooks_by_answer.entry(h.answer_id).or_default().push(json!({
            "id": h.id, "rank": h.rank, "cue": h.cue, "keyGram": h.key_gram,
            "support": h.support, "source": h.source, "status": h.status,
        }));
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
                "forms": r.forms, "vetted": r.vetted,
                "hooks": hooks_by_answer.remove(&r.id).unwrap_or_default(),
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

async fn set_hook_status(state: &Arc<AppState>, id: i32, status: &str) -> Result<Json<Value>, AppError> {
    let n = sqlx::query("UPDATE pavlov_hooks SET status = $2 WHERE id = $1")
        .bind(id)
        .bind(status)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound("No such hook".into()));
    }
    Ok(Json(json!({ "status": status })))
}

pub async fn hook_drop(State(state): State<Arc<AppState>>, _auth: AuthUser, Path(id): Path<i32>) -> Result<Json<Value>, AppError> {
    set_hook_status(&state, id, "dropped").await
}

pub async fn hook_restore(State(state): State<Arc<AppState>>, _auth: AuthUser, Path(id): Path<i32>) -> Result<Json<Value>, AppError> {
    set_hook_status(&state, id, "active").await
}

/// Up to three example clues for one hook (list page expansion).
pub async fn hook_detail(State(state): State<Arc<AppState>>, _auth: AuthUser, Path(id): Path<i32>) -> Result<Json<Value>, AppError> {
    let row: Option<(Option<String>, String, Vec<String>, Vec<i32>)> =
        sqlx::query_as("SELECT cue, key_gram, grams, clue_ids FROM pavlov_hooks WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;
    let (cue, key_gram, grams, clue_ids) = row.ok_or_else(|| AppError::NotFound("No such hook".into()))?;
    let examples: Vec<(String, Option<String>, Option<chrono::NaiveDate>)> = sqlx::query_as(
        "SELECT coalesce(answer, ''), category, air_date FROM jeopardy_questions
         WHERE id = ANY($1) ORDER BY air_date DESC NULLS LAST LIMIT 3",
    )
    .bind(&clue_ids)
    .fetch_all(&state.pool)
    .await?;
    let examples: Vec<Value> = examples
        .into_iter()
        .map(|(clue, category, air_date)| json!({ "clue": clue, "category": category, "airDate": air_date }))
        .collect();
    Ok(Json(json!({ "id": id, "cue": cue, "keyGram": key_gram, "grams": grams, "examples": examples })))
}

/// The hook map for the deck entity a clue resolves to (practice pause).
pub async fn entity_by_question(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(question_id): Path<i32>,
) -> Result<axum::response::Response, AppError> {
    use axum::response::IntoResponse;
    let answer_id: Option<i32> = sqlx::query_scalar(
        "SELECT pa.id FROM jeopardy_questions jq JOIN pavlov_answers pa ON pa.answer_norm = jq.entity_norm
         WHERE jq.id = $1",
    )
    .bind(question_id)
    .fetch_optional(&state.pool)
    .await?;
    let Some(answer_id) = answer_id else {
        return Ok(axum::http::StatusCode::NO_CONTENT.into_response());
    };
    let (answer, forms, hooks) = hook_map(&state, auth.user_id, answer_id).await?;
    Ok(Json(json!({ "answerId": answer_id, "answer": answer, "forms": forms, "hooks": hooks })).into_response())
}

use crate::hooks::{cap_for_coverage, pick_hook, HookExposure};

/// An entity can be served when it has a labeled active hook or (transitional
/// fallback, spec §5) legacy cue phrases. Alias `pa` = pavlov_answers.
pub(crate) const DRILLABLE_SQL: &str = "(cardinality(pa.phrases) > 0 OR EXISTS (
    SELECT 1 FROM pavlov_hooks h WHERE h.answer_id = pa.id AND h.status = 'active' AND h.cue IS NOT NULL))";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ServedHook {
    pub id: i32,
    pub rank: i32,
    pub cue: String,
}

#[derive(sqlx::FromRow)]
struct DrillAnswerRow {
    id: i32,
    answer_norm: String,
    phrases: Vec<String>,
    phrase_tiers: Vec<String>,
    meta_category: String,
}

fn drill_card_json(r: &DrillAnswerRow, hook: Option<ServedHook>) -> Value {
    let phrases: Vec<Value> = r
        .phrases
        .iter()
        .zip(r.phrase_tiers.iter())
        .map(|(text, tier)| json!({ "text": text, "tier": tier }))
        .collect();
    json!({
        "answerId": r.id, "answerNorm": r.answer_norm, "category": r.meta_category,
        "phrases": phrases,
        "hookId": hook.as_ref().map(|h| h.id),
        "hookRank": hook.as_ref().map(|h| h.rank),
        "cue": hook.as_ref().map(|h| h.cue.clone()),
    })
}

#[derive(sqlx::FromRow)]
struct HookPickRow {
    id: i32,
    rank: i32,
    cue: String,
    seen: i64,
    last_wrong: bool,
}

/// Spec §3 hook selection: fewest exposures → last wrong → rank.
async fn served_hook(state: &Arc<AppState>, user_id: i32, answer_id: i32) -> Result<Option<ServedHook>, AppError> {
    let rows: Vec<HookPickRow> = sqlx::query_as(
        "SELECT h.id, h.rank, h.cue, COALESCE(e.n, 0) AS seen,
                COALESCE(e.last_rating = 'wrong', false) AS last_wrong
         FROM pavlov_hooks h
         LEFT JOIN (
           SELECT hook_id, count(*) AS n,
                  (array_agg(rating ORDER BY reviewed_at DESC))[1] AS last_rating
           FROM pavlov_reviews
           WHERE user_id = $1 AND answer_id = $2 AND hook_id IS NOT NULL
           GROUP BY hook_id
         ) e ON e.hook_id = h.id
         WHERE h.answer_id = $2 AND h.status = 'active' AND h.cue IS NOT NULL",
    )
    .bind(user_id)
    .bind(answer_id)
    .fetch_all(&state.pool)
    .await?;
    let exposures: Vec<HookExposure> = rows
        .iter()
        .map(|r| HookExposure { id: r.id, rank: r.rank, seen: r.seen, last_wrong: r.last_wrong })
        .collect();
    Ok(pick_hook(&exposures)
        .and_then(|id| rows.into_iter().find(|r| r.id == id))
        .map(|r| ServedHook { id: r.id, rank: r.rank, cue: r.cue }))
}

/// Serve one card: pick its hook, shape the response.
async fn serve_card(
    state: &Arc<AppState>,
    user_id: i32,
    row: DrillAnswerRow,
    is_new: bool,
    due_count: i64,
    new_remaining: i64,
) -> Result<Json<Value>, AppError> {
    let hook = served_hook(state, user_id, row.id).await?;
    Ok(Json(json!({
        "done": false, "isNew": is_new, "card": drill_card_json(&row, hook),
        "dueCount": due_count, "newRemaining": new_remaining,
    })))
}

#[derive(sqlx::FromRow)]
struct HookMapRow {
    id: i32,
    rank: i32,
    cue: String,
    support: i32,
    source: String,
    seen: i64,
    last_wrong_at: Option<DateTime<Utc>>,
}

/// The entity's display name, merged forms, and labeled active hooks with
/// this user's exposure marks. Shared by drill_check and entity_by_question.
pub(crate) async fn hook_map(
    state: &Arc<AppState>,
    user_id: i32,
    answer_id: i32,
) -> Result<(String, Vec<String>, Vec<Value>), AppError> {
    let (answer, forms): (String, Vec<String>) =
        sqlx::query_as("SELECT answer, forms FROM pavlov_answers WHERE id = $1")
            .bind(answer_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| AppError::NotFound("No such card".into()))?;
    let rows: Vec<HookMapRow> = sqlx::query_as(
        "SELECT h.id, h.rank, h.cue, h.support, h.source,
                COALESCE(e.n, 0) AS seen, e.last_wrong_at
         FROM pavlov_hooks h
         LEFT JOIN (
           SELECT hook_id, count(*) AS n,
                  max(reviewed_at) FILTER (WHERE rating = 'wrong') AS last_wrong_at
           FROM pavlov_reviews
           WHERE user_id = $1 AND answer_id = $2 AND hook_id IS NOT NULL
           GROUP BY hook_id
         ) e ON e.hook_id = h.id
         WHERE h.answer_id = $2 AND h.status = 'active' AND h.cue IS NOT NULL
         ORDER BY h.rank, h.id",
    )
    .bind(user_id)
    .bind(answer_id)
    .fetch_all(&state.pool)
    .await?;
    let hooks = rows
        .into_iter()
        .map(|r| {
            json!({
                "id": r.id, "rank": r.rank, "cue": r.cue, "support": r.support, "source": r.source,
                "seen": r.seen, "lastWrongAt": r.last_wrong_at,
            })
        })
        .collect();
    Ok((answer, forms, hooks))
}

/// The SELECT list every drill query shares.
fn drill_cols() -> &'static str {
    "pa.id, pa.answer_norm, pa.phrases, pa.phrase_tiers, pa.meta_category"
}

/// Category-weighted new-card pick: sample a meta-category by Anytime Test
/// share (restricted to categories that still have unseen cards for this
/// user, renormalized by the race), then the evidence race within it. Falls
/// back to the unfiltered race when the sampled category comes up empty.
async fn pick_new_card(
    state: &Arc<AppState>,
    user_id: i32,
) -> Result<Option<DrillAnswerRow>, AppError> {
    let available: Vec<(String,)> = sqlx::query_as(&format!(
        "SELECT DISTINCT pa.meta_category FROM pavlov_answers pa
         WHERE {DRILLABLE_SQL}
           AND pa.id NOT IN (SELECT answer_id FROM pavlov_cards WHERE user_id = $1)"
    ))
    .bind(user_id)
    .fetch_all(&state.pool)
    .await?;
    let avail: Vec<String> = available.into_iter().map(|(c,)| c).collect();
    let weights = crate::blend::target_weights(&avail);
    let total: i64 = weights.iter().map(|(_, w)| w).sum();
    let picked_cat = if total > 0 {
        use rand::Rng;
        let mut roll = rand::rng().random_range(0..total);
        weights.iter().find(|(_, w)| { if roll < *w { true } else { roll -= w; false } })
            .map(|(c, _)| c.clone())
    } else {
        None
    };

    // Within the sampled category: vetted (JBoard-named) entities first, then
    // the most frequently recurring unseen entity (corpus count from migration
    // 0014), cue strength breaks ties, id makes it deterministic.
    // Measured on the 2026-09-15 deck: the first 1,100 draws under this order
    // carry ~66% of the remaining frequency mass (uniform: 30%, proportional
    // race: 52%). Variety still comes from the category sampling above.
    let cols = drill_cols();
    let pick_in_cat = format!(
        "SELECT {cols} FROM pavlov_answers pa
         WHERE pa.meta_category = $2 AND {DRILLABLE_SQL}
           AND pa.id NOT IN (SELECT answer_id FROM pavlov_cards WHERE user_id = $1)
         ORDER BY pa.vetted DESC, pa.answer_freq DESC, pa.score DESC, pa.id LIMIT 1"
    );
    let pick_any = format!(
        "SELECT {cols} FROM pavlov_answers pa
         WHERE {DRILLABLE_SQL}
           AND pa.id NOT IN (SELECT answer_id FROM pavlov_cards WHERE user_id = $1)
         ORDER BY pa.vetted DESC, pa.answer_freq DESC, pa.score DESC, pa.id LIMIT 1"
    );

    if let Some(cat) = picked_cat {
        if let Some(row) = sqlx::query_as::<_, DrillAnswerRow>(&pick_in_cat)
            .bind(user_id)
            .bind(&cat)
            .fetch_optional(&state.pool)
            .await?
        {
            return Ok(Some(row));
        }
    }
    Ok(sqlx::query_as::<_, DrillAnswerRow>(&pick_any)
        .bind(user_id)
        .fetch_optional(&state.pool)
        .await?)
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
        sqlx::query_as("SELECT pavlov_new_per_day, timezone FROM users WHERE id = $1")
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
        "SELECT COUNT(*) FROM pavlov_cards ca JOIN pavlov_answers pa ON pa.id = ca.answer_id
         WHERE ca.user_id = $1 AND ca.created_at >= $2 AND ca.last_review IS NOT NULL",
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

    let cols = drill_cols();
    let fetch_due = format!(
        "SELECT {cols} FROM pavlov_cards ca
         JOIN pavlov_answers pa ON pa.id = ca.answer_id
         WHERE ca.user_id = $1 AND ca.suspended = false AND ca.due <= now()
         ORDER BY ca.due ASC LIMIT 1"
    );

    if want_new {
        if let Some(row) = pick_new_card(&state, user_id).await? {
            return serve_card(&state, user_id, row, true, due_count, new_remaining).await;
        }
    }
    if let Some(row) = sqlx::query_as::<_, DrillAnswerRow>(&fetch_due)
        .bind(user_id)
        .fetch_optional(&state.pool)
        .await?
    {
        return serve_card(&state, user_id, row, false, due_count, new_remaining).await;
    }
    if new_remaining > 0 || extra {
        if let Some(row) = pick_new_card(&state, user_id).await? {
            return serve_card(&state, user_id, row, true, due_count, new_remaining).await;
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
    let more_new_available: bool = sqlx::query_scalar(&format!(
        "SELECT EXISTS (SELECT 1 FROM pavlov_answers pa
         WHERE {DRILLABLE_SQL}
           AND pa.id NOT IN (SELECT answer_id FROM pavlov_cards WHERE user_id = $1))"
    ))
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(json!({
        "done": true, "dueCount": due_count, "newRemaining": new_remaining,
        "nextDueAt": next_due_at, "dueSoonCount": due_soon_count,
        "moreNewAvailable": more_new_available,
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckBody {
    pub answer_id: i32,
    pub hook_id: Option<i32>,
    /// Optional: honesty-mode reveal sends no typed answer.
    pub typed: Option<String>,
}

/// Reveal the answer (optionally grading a typed attempt) — no SRS state
/// change (that's `grade`).
pub async fn drill_check(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<CheckBody>,
) -> Result<Json<Value>, AppError> {
    let row: Option<(String, Vec<i32>, String)> = sqlx::query_as(
        "SELECT pa.answer, pa.example_clue_ids, pa.answer_norm FROM pavlov_answers pa WHERE pa.id = $1",
    )
    .bind(body.answer_id)
    .fetch_optional(&state.pool)
    .await?;
    let (answer, example_ids, answer_norm) = row.ok_or_else(|| AppError::NotFound("No such cue".into()))?;
    let correct = body.typed.as_deref().map(|t| answer_match::is_correct(t, &answer));

    let (_, forms, hooks) = hook_map(&state, auth.user_id, body.answer_id).await?;
    let example_clue: Option<(String, Option<String>, Option<chrono::NaiveDate>)> = match body.hook_id {
        Some(hid) => {
            sqlx::query_as(
                "SELECT coalesce(jq.answer, ''), jq.category, jq.air_date
                 FROM pavlov_hooks h JOIN jeopardy_questions jq ON jq.id = ANY(h.clue_ids)
                 WHERE h.id = $1 AND h.answer_id = $2
                 ORDER BY jq.air_date DESC NULLS LAST LIMIT 1",
            )
            .bind(hid)
            .bind(body.answer_id)
            .fetch_optional(&state.pool)
            .await?
        }
        None => None,
    };

    let examples: Vec<(String, Option<String>, Option<chrono::NaiveDate>)> = sqlx::query_as(
        "SELECT coalesce(answer, ''), category, air_date FROM jeopardy_questions
         WHERE id = ANY($1) ORDER BY air_date DESC",
    )
    .bind(&example_ids[..])
    .fetch_all(&state.pool)
    .await?;
    let ex_json = |(clue, category, air_date): (String, Option<String>, Option<chrono::NaiveDate>)| {
        json!({ "clue": clue, "category": category, "airDate": air_date })
    };
    Ok(Json(json!({
        "correct": correct, "answer": answer, "answerNorm": answer_norm, "forms": forms,
        "hooks": hooks, "servedHookId": body.hook_id,
        "exampleClue": example_clue.map(ex_json),
        "examples": examples.into_iter().map(ex_json).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrillGradeBody {
    pub answer_id: i32,
    pub rating: String,
    pub hook_id: Option<i32>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct PavlovCardRow {
    pub(crate) state: String,
    pub(crate) interval_days: f64,
    pub(crate) ease: f64,
    pub(crate) reps: i32,
    pub(crate) lapses: i32,
    pub(crate) step_index: i16,
    pub(crate) last_review: Option<DateTime<Utc>>,
}

/// A grade is the card's first when no card row exists yet, or when the row
/// was created by a suspend/banish and has never been graded (last_review is
/// NULL). Mirrors the `last_review IS NOT NULL` rule used for "new today".
pub(crate) fn is_first_grade(existing: Option<&PavlovCardRow>) -> bool {
    existing.map_or(true, |r| r.last_review.is_none())
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

    // A served hook must belong to the graded entity.
    if let Some(hid) = body.hook_id {
        let ok: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM pavlov_hooks WHERE id = $1 AND answer_id = $2)",
        )
        .bind(hid)
        .bind(body.answer_id)
        .fetch_one(&state.pool)
        .await?;
        if !ok {
            return Err(AppError::BadRequest("hookId does not belong to answerId".into()));
        }
    }

    let mut tx = state.pool.begin().await?;

    let existing: Option<PavlovCardRow> = sqlx::query_as(
        "SELECT state, interval_days, ease, reps, lapses, step_index, last_review
         FROM pavlov_cards WHERE user_id = $1 AND answer_id = $2",
    )
    .bind(user_id)
    .bind(body.answer_id)
    .fetch_optional(&mut *tx)
    .await?;
    let first_grade = is_first_grade(existing.as_ref());
    let prev = existing.map(|r| Prev {
        state: CardKind::from_str(&r.state),
        interval_days: r.interval_days,
        ease: r.ease,
        reps: r.reps,
        lapses: r.lapses,
        step_index: r.step_index,
    });

    let out = schedule(prev, rating);

    // Coverage guard (spec §3): while a labeled hook other than the one just
    // served is still unseen by this user, cap the interval.
    let has_unseen: bool = sqlx::query_scalar(
        "SELECT EXISTS (
           SELECT 1 FROM pavlov_hooks h
           WHERE h.answer_id = $2 AND h.status = 'active' AND h.cue IS NOT NULL
             AND h.id <> COALESCE($3, -1)
             AND NOT EXISTS (SELECT 1 FROM pavlov_reviews r WHERE r.user_id = $1 AND r.hook_id = h.id))",
    )
    .bind(user_id)
    .bind(body.answer_id)
    .bind(body.hook_id)
    .fetch_one(&mut *tx)
    .await?;
    let (interval_days, interval_secs) = cap_for_coverage(out.interval_days, out.interval_secs, has_unseen);
    let now: DateTime<Utc> = Utc::now();
    let due = now + Duration::seconds(interval_secs);
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
    .bind(interval_days)
    .bind(out.ease)
    .bind(due)
    .bind(now)
    .bind(out.reps)
    .bind(out.lapses)
    .bind(out.step_index)
    .bind(suspended)
    .execute(&mut *tx)
    .await?;

    // Per-grade log — the dashboard's only source for accuracy over time.
    // `body.rating` was validated by Rating::from_wire above.
    sqlx::query(
        "INSERT INTO pavlov_reviews (user_id, answer_id, rating, first_grade, reviewed_at, hook_id)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(user_id)
    .bind(body.answer_id)
    .bind(&body.rating)
    .bind(first_grade)
    .bind(now)
    .bind(body.hook_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Json(json!({
        "state": out.state.as_str(),
        "due": due,
        "intervalDays": interval_days,
        "requeueInSession": out.requeue_in_session,
    })))
}

#[cfg(test)]
mod tests {
    use super::{drill_card_json, is_first_grade, DrillAnswerRow, PavlovCardRow, ServedHook};
    use chrono::Utc;

    fn row(last_review: Option<chrono::DateTime<Utc>>) -> PavlovCardRow {
        PavlovCardRow {
            state: "learning".into(),
            interval_days: 0.0,
            ease: 2.5,
            reps: 0,
            lapses: 0,
            step_index: 0,
            last_review,
        }
    }

    #[test]
    fn no_card_row_is_first_grade() {
        assert!(is_first_grade(None));
    }

    #[test]
    fn banish_created_card_never_graded_is_first_grade() {
        // suspend() inserts a card row with last_review NULL; un-banishing then
        // grading must still count as the card's first grade.
        assert!(is_first_grade(Some(&row(None))));
    }

    #[test]
    fn previously_graded_card_is_not_first_grade() {
        assert!(!is_first_grade(Some(&row(Some(Utc::now())))));
    }

    #[test]
    fn card_json_carries_hook_or_falls_back_to_phrases() {
        let row = DrillAnswerRow {
            id: 7, answer_norm: "visigoths".into(),
            phrases: vec!["the Goths split".into()], phrase_tiers: vec!["standard".into()],
            meta_category: "History & Politics".into(),
        };
        let with = drill_card_json(&row, Some(ServedHook { id: 3, rank: 2, cue: "ruled Spain until 711".into() }));
        assert_eq!(with["hookId"], 3);
        assert_eq!(with["cue"], "ruled Spain until 711");
        let without = drill_card_json(&row, None);
        assert!(without["hookId"].is_null());
        assert_eq!(without["phrases"][0]["text"], "the Goths split");
    }
}
