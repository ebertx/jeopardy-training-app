#![allow(dead_code)]

//! Async jobs for Jeopardy objects (spec §5): entity resolution and hook
//! mining. All pure logic lives in `entity.rs` / `hooks.rs`; this module is
//! SQL and orchestration. Every job is idempotent and resumable.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::entity::{self, Entity, Form};
use crate::error::AppError;
use crate::AppState;

const UPDATE_CHUNK: usize = 5000;

/// Resolve the whole non-archived corpus into entities (pure, in memory).
pub async fn resolved_entities(state: &Arc<AppState>) -> Result<Vec<Entity>, AppError> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT question, count(*) FROM jeopardy_questions
         WHERE archived = false AND question IS NOT NULL GROUP BY 1",
    )
    .fetch_all(&state.pool)
    .await?;
    let forms: Vec<Form> = rows.into_iter().map(|(raw, count)| Form { raw, count }).collect();
    let entities = entity::resolve(&forms);
    tracing::info!("objects: {} response forms -> {} entities", forms.len(), entities.len());
    Ok(entities)
}

/// Job 1 (spec §5): write `entity_norm` on every clue, merge deck rows whose
/// old string norms belong to one entity, re-key cues and n-grams, refresh
/// forms / display / frequency. A rerun finds nothing left to merge.
pub async fn run_resolve(state: &Arc<AppState>) -> Result<(), AppError> {
    let entities = resolved_entities(state).await?;

    // 1. entity_norm per clue (chunked; only rows whose key changes are written)
    let pairs: Vec<(String, String)> = entities
        .iter()
        .flat_map(|e| e.forms.iter().map(move |f| (f.clone(), e.key.clone())))
        .collect();
    for chunk in pairs.chunks(UPDATE_CHUNK) {
        let (fs, ks): (Vec<String>, Vec<String>) = chunk.iter().cloned().unzip();
        sqlx::query(
            "UPDATE jeopardy_questions jq SET entity_norm = m.key
             FROM unnest($1::text[], $2::text[]) AS m(form, key)
             WHERE jq.question = m.form AND jq.archived = false
               AND jq.entity_norm IS DISTINCT FROM m.key",
        )
        .bind(&fs)
        .bind(&ks)
        .execute(&state.pool)
        .await?;
    }
    tracing::info!("objects resolve: entity_norm written");

    // 2. merge deck rows: the old (0008) norms of an entity's forms may map to
    //    several pavlov_answers rows — keep one, move cards/reviews, re-key cues.
    let mut merged = 0usize;
    for e in &entities {
        let old_norms: Vec<String> = e
            .forms
            .iter()
            .map(|f| entity::norm_response(f))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let deck: Vec<(i32, String)> = sqlx::query_as(
            "SELECT id, answer_norm FROM pavlov_answers WHERE answer_norm = ANY($1)
             ORDER BY answer_freq DESC, id",
        )
        .bind(&old_norms)
        .fetch_all(&state.pool)
        .await?;
        if deck.is_empty() || (deck.len() == 1 && deck[0].1 == e.key) {
            continue;
        }
        let survivor = deck.iter().find(|(_, n)| *n == e.key).map(|(id, _)| *id).unwrap_or(deck[0].0);
        let losers: Vec<i32> = deck.iter().map(|(id, _)| *id).filter(|id| *id != survivor).collect();

        let mut tx = state.pool.begin().await?;
        for loser in &losers {
            merge_cards(&mut tx, survivor, *loser).await?;
            sqlx::query("UPDATE pavlov_reviews SET answer_id = $1 WHERE answer_id = $2")
                .bind(survivor)
                .bind(loser)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM pavlov_answers WHERE id = $1").bind(loser).execute(&mut *tx).await?;
        }
        // cues: a loser cue with the same stem as a survivor cue would collide on
        // UNIQUE (answer_norm, cue_stem) — drop it first, then re-key the rest.
        sqlx::query(
            "DELETE FROM pavlov_cues c USING pavlov_cues k
             WHERE c.answer_norm = ANY($1) AND c.answer_norm <> $2
               AND k.answer_norm = $2 AND k.cue_stem = c.cue_stem",
        )
        .bind(&old_norms)
        .bind(&e.key)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE pavlov_cues SET answer_norm = $2, answer = $3
             WHERE answer_norm = ANY($1) AND answer_norm <> $2",
        )
        .bind(&old_norms)
        .bind(&e.key)
        .bind(&e.display)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE pavlov_answers SET answer_norm = $2 WHERE id = $1")
            .bind(survivor)
            .bind(&e.key)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        merged += 1;
    }
    tracing::info!("objects resolve: {merged} entities merged in the deck");

    // 3. n-grams: re-key every old norm that differs from its entity key
    let rekey: Vec<(String, String)> = entities
        .iter()
        .flat_map(|e| {
            e.forms
                .iter()
                .map(|f| (entity::norm_response(f), e.key.clone()))
                .filter(|(o, k)| o != k)
                .collect::<BTreeSet<_>>()
        })
        .collect();
    for chunk in rekey.chunks(UPDATE_CHUNK) {
        let (os, ks): (Vec<String>, Vec<String>) = chunk.iter().cloned().unzip();
        sqlx::query(
            "UPDATE pavlov_clue_ngrams g SET answer_norm = m.key
             FROM unnest($1::text[], $2::text[]) AS m(old, key)
             WHERE g.answer_norm = m.old",
        )
        .bind(&os)
        .bind(&ks)
        .execute(&state.pool)
        .await?;
    }
    tracing::info!("objects resolve: {} n-gram keys rewritten", rekey.len());

    refresh_entity_rows(state, &entities).await
}

/// For one user holding cards on both rows keep the one with more reps
/// (ties: the survivor's); re-point the loser's card for users who only
/// have that one.
async fn merge_cards(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    survivor: i32,
    loser: i32,
) -> Result<(), AppError> {
    sqlx::query(
        "DELETE FROM pavlov_cards s USING pavlov_cards l
         WHERE s.answer_id = $1 AND l.answer_id = $2 AND s.user_id = l.user_id AND l.reps > s.reps",
    )
    .bind(survivor)
    .bind(loser)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "DELETE FROM pavlov_cards l USING pavlov_cards s
         WHERE l.answer_id = $2 AND s.answer_id = $1 AND s.user_id = l.user_id",
    )
    .bind(survivor)
    .bind(loser)
    .execute(&mut **tx)
    .await?;
    sqlx::query("UPDATE pavlov_cards SET answer_id = $1 WHERE answer_id = $2")
        .bind(survivor)
        .bind(loser)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Refresh forms / display / answer_freq on every deck row from the resolved
/// corpus. Rerunnable; the end of resolve and the start of hooks both call it
/// (assemble_stage creates rows with a mode() display and no forms).
pub async fn refresh_entity_rows(state: &Arc<AppState>, entities: &[Entity]) -> Result<(), AppError> {
    let keys: Vec<String> = sqlx::query_scalar("SELECT answer_norm FROM pavlov_answers")
        .fetch_all(&state.pool)
        .await?;
    let in_deck: std::collections::HashSet<&str> = keys.iter().map(|s| s.as_str()).collect();
    let mut n = 0usize;
    for e in entities.iter().filter(|e| in_deck.contains(e.key.as_str())) {
        sqlx::query(
            "UPDATE pavlov_answers SET forms = $2, answer = $3, answer_freq = $4 WHERE answer_norm = $1",
        )
        .bind(&e.key)
        .bind(&e.forms)
        .bind(&e.display)
        .bind(e.freq as i32)
        .execute(&state.pool)
        .await?;
        n += 1;
    }
    tracing::info!("objects: refreshed {n} entity rows");
    Ok(())
}
