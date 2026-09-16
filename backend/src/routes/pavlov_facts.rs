//! Fact cards: the four facts of an answer sheet become four extra Pavlov
//! cards (kind='fact') under the parent answer, so the topic is drilled in
//! depth. Idempotent: re-adding is a no-op.
use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::sheets::{fact_norm, parse_sheet};
use crate::AppState;

#[derive(sqlx::FromRow)]
struct ParentRow {
    meta_category: String,
    score: f32,
    answer_freq: i32,
    example_clue_ids: Vec<i32>,
}

/// Upsert the four fact rows and give the user a due-now card for each one
/// they lack. Returns Ok(None) when no sheet is cached for this answer.
pub async fn add_fact_cards(
    state: &Arc<AppState>,
    user_id: i32,
    answer_norm: &str,
) -> Result<Option<i64>, AppError> {
    let sheet: Option<(Value,)> =
        sqlx::query_as("SELECT content FROM answer_sheets WHERE answer_norm = $1")
            .bind(answer_norm)
            .fetch_optional(&state.pool)
            .await?;
    let Some((content,)) = sheet else { return Ok(None) };
    let sheet = match parse_sheet(&content, answer_norm) {
        Ok(s) => s,
        Err(_) => return Ok(None),
    };

    // Parent metadata: the deck row when present, else derived from the corpus
    // (practice misses can be answers outside the Pavlov deck).
    let parent: Option<ParentRow> = sqlx::query_as(
        "SELECT meta_category, score, answer_freq, example_clue_ids FROM pavlov_answers
         WHERE answer_norm = $1 AND kind = 'answer'",
    )
    .bind(answer_norm)
    .fetch_optional(&state.pool)
    .await?;
    let parent = match parent {
        Some(p) => p,
        None => {
            let (cat, freq, ids): (Option<String>, i32, Vec<i32>) = sqlx::query_as(
                "SELECT max(classifier_category), COALESCE(max(answer_freq), 1),
                        COALESCE(array_agg(id ORDER BY air_date DESC), '{}')
                 FROM (SELECT classifier_category, answer_freq, id, air_date FROM jeopardy_questions
                       WHERE lower(trim(regexp_replace(question, '^(the|a|an) ', '', 'i'))) = $1
                         AND archived = false LIMIT 5) q",
            )
            .bind(answer_norm)
            .fetch_one(&state.pool)
            .await?;
            ParentRow {
                meta_category: cat.unwrap_or_else(|| "Miscellaneous".to_string()),
                score: 0.0,
                answer_freq: freq,
                example_clue_ids: ids,
            }
        }
    };

    let mut added = 0i64;
    for (i, fact) in sheet.facts.iter().enumerate() {
        let norm = fact_norm(answer_norm, i + 1);
        let answer_id: i32 = sqlx::query_scalar(
            "INSERT INTO pavlov_answers
               (answer_norm, answer, meta_category, phrases, phrase_tiers, score,
                example_clue_ids, answer_freq, kind, parent_norm)
             VALUES ($1, $2, $3, $4, '{standard}', $5, $6, $7, 'fact', $8)
             ON CONFLICT (answer_norm) DO UPDATE SET
               answer = EXCLUDED.answer, phrases = EXCLUDED.phrases,
               meta_category = EXCLUDED.meta_category
             RETURNING id",
        )
        .bind(&norm)
        .bind(&fact.response)
        .bind(&parent.meta_category)
        .bind(vec![fact.prompt.clone()])
        .bind(parent.score)
        .bind(&parent.example_clue_ids)
        .bind(parent.answer_freq)
        .bind(answer_norm)
        .fetch_one(&state.pool)
        .await?;
        let res = sqlx::query(
            "INSERT INTO pavlov_cards (user_id, answer_id) VALUES ($1, $2)
             ON CONFLICT (user_id, answer_id) DO NOTHING",
        )
        .bind(user_id)
        .bind(answer_id)
        .execute(&state.pool)
        .await?;
        added += res.rows_affected() as i64;
    }
    Ok(Some(added))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactsBody {
    pub answer_norm: String,
}

pub async fn add_facts(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<FactsBody>,
) -> Result<Json<Value>, AppError> {
    match add_fact_cards(&state, auth.user_id, &body.answer_norm).await? {
        Some(added) => Ok(Json(json!({ "added": added }))),
        None => Err(AppError::NotFound("No sheet for that answer yet".into())),
    }
}
