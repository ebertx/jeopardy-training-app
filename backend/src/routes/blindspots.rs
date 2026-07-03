use axum::{extract::State, Json};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::blindspots::{generate_packs_for_user, needs_refresh, GenOutcome};
use crate::error::AppError;
use crate::AppState;

#[derive(sqlx::FromRow)]
struct PackRow {
    id: i32,
    theme: String,
    diagnosis: String,
    primer: String,
    search_query: String,
    match_count: i32,
    created_at: chrono::DateTime<chrono::Utc>,
}

async fn load_state(
    state: &Arc<AppState>,
    user_id: i32,
) -> Result<(Vec<PackRow>, Option<chrono::DateTime<chrono::Utc>>, i64, i64), AppError> {
    let packs: Vec<PackRow> = sqlx::query_as(
        "SELECT id, theme, diagnosis, primer, search_query, match_count, created_at
         FROM blindspot_packs
         WHERE user_id = $1 AND superseded = false
         ORDER BY id ASC",
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await?;
    let generated_at = packs.first().map(|p| p.created_at);

    let total_recent: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM question_attempts
         WHERE user_id = $1 AND correct = false AND answered_at >= now() - interval '30 days'",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;

    let new_since: i64 = match generated_at {
        Some(t) => sqlx::query_scalar(
            "SELECT COUNT(*) FROM question_attempts
             WHERE user_id = $1 AND correct = false AND answered_at >= $2",
        )
        .bind(user_id)
        .bind(t)
        .fetch_one(&state.pool)
        .await?,
        None => 0,
    };
    Ok((packs, generated_at, new_since, total_recent))
}

fn response_json(
    packs: &[PackRow],
    generated_at: Option<chrono::DateTime<chrono::Utc>>,
    stale: bool,
    insufficient: bool,
    configured: bool,
) -> Value {
    json!({
        "packs": packs.iter().map(|p| json!({
            "id": p.id,
            "theme": p.theme,
            "diagnosis": p.diagnosis,
            "primer": p.primer,
            "searchQuery": p.search_query,
            "matchCount": p.match_count,
        })).collect::<Vec<_>>(),
        "generatedAt": generated_at,
        "stale": stale,
        "insufficientData": insufficient,
        "configured": configured,
    })
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let configured = !state.config.openai_api_key.is_empty();
    let (packs, generated_at, new_since, total_recent) = load_state(&state, user_id).await?;
    let stale = needs_refresh(generated_at, new_since, total_recent, chrono::Utc::now());
    let insufficient = generated_at.is_none() && total_recent < crate::blindspots::MIN_MISSES_TO_GENERATE;

    // Background auto-refresh: fire once, guarded; the CURRENT set is returned.
    if stale && configured && !state.blindspot_inflight.swap(true, Ordering::SeqCst) {
        let st = state.clone();
        tokio::spawn(async move {
            if let Err(e) = generate_packs_for_user(&st, user_id).await {
                tracing::warn!("blindspot auto-refresh failed: {e:?}");
            }
            st.blindspot_inflight.store(false, Ordering::SeqCst);
        });
    }

    Ok(Json(response_json(&packs, generated_at, stale, insufficient, configured)))
}

pub async fn generate(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let configured = !state.config.openai_api_key.is_empty();
    if !configured {
        return Err(AppError::BadRequest(
            "Blind-spot analysis is disabled (no OPENAI_API_KEY configured).".to_string(),
        ));
    }
    let outcome = generate_packs_for_user(&state, user_id).await?;
    let (packs, generated_at, _new_since, _total) = load_state(&state, user_id).await?;
    let insufficient = matches!(outcome, GenOutcome::InsufficientData);
    Ok(Json(response_json(&packs, generated_at, false, insufficient, configured)))
}
