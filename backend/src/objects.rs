//! Async jobs for Jeopardy objects (spec §5): entity resolution and hook
//! mining. All pure logic lives in `entity.rs` / `hooks.rs`; this module is
//! SQL and orchestration. Every job is idempotent and resumable.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::entity::{self, Entity, Form};
use crate::error::AppError;
use crate::hooks::{
    cluster_hooks, hook_label_prompts, parse_hook_labels, parse_vetted_tsv, vetted_matches, GramStat,
    HookLabelInput, HOOK_LABEL_BATCH, HOOK_LABEL_MODEL, HOOK_MIN_SUPPORT,
};
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
    let mut renamed = 0usize;
    for e in &entities {
        let mut old_norms: Vec<String> = e
            .forms
            .iter()
            .map(|f| entity::norm_response(f))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        // The entity key itself may not appear among the corpus forms' norms
        // (e.g. the bare form fell out of the non-archived corpus), yet a
        // pre-existing deck row can already sit at that key. Without it in
        // the search set, that row is invisible here and the later
        // `UPDATE ... SET answer_norm = e.key` collides with it.
        if !old_norms.contains(&e.key) {
            old_norms.push(e.key.clone());
        }
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
        // cues: UNIQUE (answer_norm, cue_stem) — among every cue row that will
        // be re-keyed to e.key, keep one per stem (prefer the row already at
        // the key, then higher support, then lowest id) and drop the rest.
        sqlx::query(
            "DELETE FROM pavlov_cues c USING (
               SELECT id, row_number() OVER (
                        PARTITION BY cue_stem
                        ORDER BY (answer_norm = $2) DESC, support DESC, id) AS rn
               FROM pavlov_cues WHERE answer_norm = ANY($1)
             ) r
             WHERE c.id = r.id AND r.rn > 1",
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
        if losers.is_empty() {
            renamed += 1;
        } else {
            merged += 1;
        }
    }
    tracing::info!("objects resolve: {merged} entities merged, {renamed} rows re-keyed in the deck");

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

const VETTED_TSV: &str = include_str!("../data/pavlovs-jboard.tsv");
const HOOK_SAMPLE_CLUES: i64 = 5;
const MINE_BATCH: i64 = 200;

/// Job 2 (spec §5): refresh entity rows, import the vetted pairs, mine hooks
/// for every entity, label them (vetted → cue → model), rerank.
pub async fn run_hooks(state: &Arc<AppState>) -> Result<(), AppError> {
    let entities = resolved_entities(state).await?;
    let vetted = import_vetted(state).await?;
    refresh_entity_rows(state, &entities).await?;

    // A completed previous run leaves no entity pending → this is a
    // regeneration: re-mine everything. Otherwise resume where it stopped.
    let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM pavlov_answers WHERE hooks_built_at IS NULL")
        .fetch_one(&state.pool)
        .await?;
    if pending == 0 {
        sqlx::query("UPDATE pavlov_answers SET hooks_built_at = NULL").execute(&state.pool).await?;
        // A regeneration re-mines every entity's grams from scratch, so the
        // stale document-frequency table must go too — otherwise every
        // re-mined gram is absent from it, COALESCE(d.df, 0) scores them all
        // as maximally distinctive, and they win seeding/key-gram selection.
        tracing::info!("objects hooks: regeneration run — truncating pavlov_gram_df for rebuild");
        sqlx::query("TRUNCATE pavlov_gram_df").execute(&state.pool).await?;
    }
    ensure_gram_df(state).await?;
    // Unlabeled hooks from earlier runs get another try at a label.
    sqlx::query("UPDATE pavlov_hooks SET label_attempted_at = NULL WHERE cue IS NULL AND status = 'active'")
        .execute(&state.pool)
        .await?;

    let corpus_clues: i64 =
        sqlx::query_scalar("SELECT count(*) FROM jeopardy_questions WHERE archived = false AND question IS NOT NULL")
            .fetch_one(&state.pool)
            .await?;
    mine_hooks(state, corpus_clues).await?;
    label_vetted(state, &vetted).await?;
    label_from_cues(state).await?;
    label_with_model(state).await?;
    rerank(state).await
}

async fn ensure_gram_df(state: &Arc<AppState>) -> Result<(), AppError> {
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM pavlov_gram_df").fetch_one(&state.pool).await?;
    if n > 0 {
        return Ok(());
    }
    tracing::info!("objects hooks: building pavlov_gram_df (one-time)");
    sqlx::query(
        "INSERT INTO pavlov_gram_df (gram, df)
         SELECT gram, count(DISTINCT clue_id) FROM pavlov_clue_ngrams GROUP BY 1",
    )
    .execute(&state.pool)
    .await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct GramRow {
    gram: String,
    n: i16,
    support: i64,
    df: i32,
    clue_ids: Vec<i32>,
}

/// Cluster every pending entity's grams and upsert its hooks.
async fn mine_hooks(state: &Arc<AppState>, corpus_clues: i64) -> Result<(), AppError> {
    loop {
        let batch: Vec<(i32, String)> = sqlx::query_as(
            "SELECT id, answer_norm FROM pavlov_answers WHERE hooks_built_at IS NULL ORDER BY id LIMIT $1",
        )
        .bind(MINE_BATCH)
        .fetch_all(&state.pool)
        .await?;
        if batch.is_empty() {
            return Ok(());
        }
        for (answer_id, norm) in batch {
            let rows: Vec<GramRow> = sqlx::query_as(
                "SELECT g.gram, g.n, count(DISTINCT g.clue_id) AS support,
                        COALESCE(d.df, 0) AS df,
                        array_agg(DISTINCT g.clue_id) AS clue_ids
                 FROM pavlov_clue_ngrams g LEFT JOIN pavlov_gram_df d ON d.gram = g.gram
                 WHERE g.answer_norm = $1
                 GROUP BY g.gram, g.n, d.df
                 HAVING count(DISTINCT g.clue_id) >= $2",
            )
            .bind(&norm)
            .bind(HOOK_MIN_SUPPORT)
            .fetch_all(&state.pool)
            .await?;
            let grams: Vec<GramStat> = rows
                .into_iter()
                .map(|r| GramStat { gram: r.gram, n: r.n, support: r.support, corpus_df: r.df as i64, clue_ids: r.clue_ids })
                .collect();
            let clusters = cluster_hooks(&grams, corpus_clues);

            let mut tx = state.pool.begin().await?;
            let mut keys: Vec<String> = Vec::with_capacity(clusters.len());
            for (i, c) in clusters.iter().enumerate() {
                sqlx::query(
                    "INSERT INTO pavlov_hooks (answer_id, key_gram, rank, grams, clue_ids, support)
                     VALUES ($1, $2, $3, $4, $5, $6)
                     ON CONFLICT (answer_id, key_gram) DO UPDATE SET
                       rank = EXCLUDED.rank, grams = EXCLUDED.grams,
                       clue_ids = EXCLUDED.clue_ids, support = EXCLUDED.support",
                )
                .bind(answer_id)
                .bind(&c.key_gram)
                .bind((i + 1) as i32)
                .bind(&c.grams)
                .bind(&c.clue_ids)
                .bind(c.support as i32)
                .execute(&mut *tx)
                .await?;
                keys.push(c.key_gram.clone());
            }
            // Mined hooks that no longer come out of clustering and never got a
            // label are noise from a previous run; labeled ones keep their history.
            sqlx::query(
                "DELETE FROM pavlov_hooks WHERE answer_id = $1 AND source = 'mined' AND cue IS NULL
                   AND NOT (key_gram = ANY($2))",
            )
            .bind(answer_id)
            .bind(&keys)
            .execute(&mut *tx)
            .await?;
            sqlx::query("UPDATE pavlov_answers SET hooks_built_at = now() WHERE id = $1")
                .bind(answer_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
        }
    }
}

struct VettedMatch {
    answer_id: i32,
    cue: String,
    lexemes: Vec<String>,
}

/// Resolve every vetted response to a deck entity, inserting vetted entities
/// the deck lacks. Unmatched responses are logged for hand review.
async fn import_vetted(state: &Arc<AppState>) -> Result<Vec<VettedMatch>, AppError> {
    let pairs = parse_vetted_tsv(VETTED_TSV);
    let mut out = Vec::with_capacity(pairs.len());
    let mut unmatched = 0usize;
    for p in &pairs {
        let key = entity::entity_key(&p.response);
        if key.is_empty() {
            continue;
        }
        // The response may be a full name whose entity key is the licensed
        // surname: look the old string norm up in the resolved corpus.
        let entity_norm: Option<String> = sqlx::query_scalar(
            "SELECT entity_norm FROM jeopardy_questions
             WHERE archived = false AND entity_norm IS NOT NULL
               AND (entity_norm = $1
                    OR lower(trim(regexp_replace(question, '^(the|a|an) ', '', 'i'))) = $1)
             ORDER BY (entity_norm = $1) DESC LIMIT 1",
        )
        .bind(&key)
        .fetch_optional(&state.pool)
        .await?;
        let Some(norm) = entity_norm else {
            unmatched += 1;
            tracing::warn!("vetted pair unmatched in corpus: {:?} = {:?}", p.cue, p.response);
            continue;
        };
        let answer_id: i32 = sqlx::query_scalar(
            "INSERT INTO pavlov_answers
               (answer_norm, answer, meta_category, phrases, phrase_tiers, score, example_clue_ids, answer_freq, vetted)
             SELECT $1,
                    mode() WITHIN GROUP (ORDER BY question),
                    COALESCE(mode() WITHIN GROUP (ORDER BY classifier_category), 'Miscellaneous'),
                    '{}', '{}', 0,
                    (SELECT COALESCE(array_agg(id), '{}') FROM
                       (SELECT id FROM jeopardy_questions WHERE entity_norm = $1 AND archived = false
                        ORDER BY air_date DESC NULLS LAST LIMIT 3) x),
                    count(*), true
             FROM jeopardy_questions WHERE entity_norm = $1 AND archived = false
             HAVING count(*) > 0
             ON CONFLICT (answer_norm) DO UPDATE SET vetted = true
             RETURNING id",
        )
        .bind(&norm)
        .fetch_one(&state.pool)
        .await?;
        let lexemes: Vec<String> =
            sqlx::query_scalar("SELECT tsvector_to_array(to_tsvector('english', $1))")
                .bind(&p.cue)
                .fetch_one(&state.pool)
                .await?;
        out.push(VettedMatch { answer_id, cue: p.cue.clone(), lexemes });
    }
    tracing::info!("objects hooks: {} vetted pairs matched, {unmatched} unmatched", out.len());
    Ok(out)
}

fn vetted_key_gram(cue: &str) -> String {
    let mut toks: Vec<String> = crate::pavlov::norm_tokens(cue).into_iter().collect();
    toks.sort();
    let slug: String = toks.join("-").chars().take(40).collect();
    format!("vetted:{slug}")
}

/// A vetted cue labels the cluster whose grams it hits; otherwise it becomes
/// its own hook, with member clues found by full-text search.
async fn label_vetted(state: &Arc<AppState>, vetted: &[VettedMatch]) -> Result<(), AppError> {
    for v in vetted {
        let hooks: Vec<(i32, Vec<String>)> = sqlx::query_as(
            "SELECT id, grams FROM pavlov_hooks WHERE answer_id = $1 AND status = 'active' ORDER BY rank",
        )
        .bind(v.answer_id)
        .fetch_all(&state.pool)
        .await?;
        if let Some((id, _)) = hooks.iter().find(|(_, grams)| vetted_matches(&v.lexemes, grams)) {
            sqlx::query("UPDATE pavlov_hooks SET cue = $2, source = 'both' WHERE id = $1")
                .bind(id)
                .bind(&v.cue)
                .execute(&state.pool)
                .await?;
            continue;
        }
        let clue_ids: Vec<i32> = sqlx::query_scalar(
            "SELECT jq.id FROM jeopardy_questions jq JOIN pavlov_answers pa ON pa.answer_norm = jq.entity_norm
             WHERE pa.id = $1 AND jq.archived = false
               AND to_tsvector('english', coalesce(jq.answer, '')) @@ plainto_tsquery('english', $2)
             ORDER BY jq.air_date DESC NULLS LAST LIMIT 50",
        )
        .bind(v.answer_id)
        .bind(&v.cue)
        .fetch_all(&state.pool)
        .await?;
        sqlx::query(
            "INSERT INTO pavlov_hooks (answer_id, key_gram, rank, cue, grams, clue_ids, support, source)
             VALUES ($1, $2, 99, $3, '{}', $4, $5, 'vetted')
             ON CONFLICT (answer_id, key_gram) DO UPDATE SET
               cue = EXCLUDED.cue, clue_ids = EXCLUDED.clue_ids, support = EXCLUDED.support",
        )
        .bind(v.answer_id)
        .bind(vetted_key_gram(&v.cue))
        .bind(&v.cue)
        .bind(&clue_ids)
        .bind(clue_ids.len() as i32)
        .execute(&state.pool)
        .await?;
    }
    Ok(())
}

/// An active standard v2 cue whose stem is one of the cluster's grams labels
/// it for free.
async fn label_from_cues(state: &Arc<AppState>) -> Result<(), AppError> {
    let n = sqlx::query(
        "UPDATE pavlov_hooks h SET cue = c.cue_display, source = 'cue'
         FROM pavlov_answers pa, pavlov_cues c
         WHERE h.answer_id = pa.id AND c.answer_norm = pa.answer_norm
           AND c.status = 'active' AND c.tier = 'standard' AND c.cue_display <> ''
           AND h.cue IS NULL AND h.status = 'active' AND c.cue_stem = ANY(h.grams)",
    )
    .execute(&state.pool)
    .await?
    .rows_affected();
    tracing::info!("objects hooks: {n} hooks labeled from existing cues");
    Ok(())
}

#[derive(sqlx::FromRow)]
struct PendingHook {
    id: i32,
    answer: String,
    key_gram: String,
    grams: Vec<String>,
    clue_ids: Vec<i32>,
}

/// Batched model labels for whatever is still unlabeled. Resumable: each
/// batch is marked attempted before the call, and the loop ends when nothing
/// unattempted remains.
async fn label_with_model(state: &Arc<AppState>) -> Result<(), AppError> {
    if state.config.openai_api_key.is_empty() {
        tracing::warn!("objects hooks: no OPENAI_API_KEY — skipping model labels");
        return Ok(());
    }
    loop {
        let batch: Vec<PendingHook> = sqlx::query_as(
            "SELECT h.id, pa.answer, h.key_gram, h.grams, h.clue_ids
             FROM pavlov_hooks h JOIN pavlov_answers pa ON pa.id = h.answer_id
             WHERE h.cue IS NULL AND h.status = 'active' AND h.label_attempted_at IS NULL
             ORDER BY h.id LIMIT $1",
        )
        .bind(HOOK_LABEL_BATCH)
        .fetch_all(&state.pool)
        .await?;
        if batch.is_empty() {
            return Ok(());
        }
        let ids: Vec<i32> = batch.iter().map(|b| b.id).collect();
        sqlx::query("UPDATE pavlov_hooks SET label_attempted_at = now() WHERE id = ANY($1)")
            .bind(&ids)
            .execute(&state.pool)
            .await?;

        let mut inputs = Vec::with_capacity(batch.len());
        for b in &batch {
            let clues: Vec<String> = sqlx::query_scalar(
                "SELECT coalesce(answer, '') FROM jeopardy_questions WHERE id = ANY($1)
                 ORDER BY air_date DESC NULLS LAST LIMIT $2",
            )
            .bind(&b.clue_ids)
            .bind(HOOK_SAMPLE_CLUES)
            .fetch_all(&state.pool)
            .await?;
            inputs.push(HookLabelInput {
                answer: b.answer.clone(),
                key_gram: b.key_gram.clone(),
                grams: b.grams.clone(),
                sample_clues: clues,
            });
        }
        let (system, user) = hook_label_prompts(&inputs);
        let response =
            match crate::openai::chat_json(&state.config.openai_api_key, HOOK_LABEL_MODEL, &system, &user, 0.3).await
            {
                Ok(r) => r,
                Err(e) => {
                    // This batch is already marked `label_attempted_at`, so it
                    // will retry next run's reset; let `rerank` still run
                    // instead of aborting the whole hooks job on one transient
                    // model failure.
                    tracing::warn!("objects hooks: model batch failed, stopping labels for this run: {e:?}");
                    return Ok(());
                }
            };
        let mut labeled = 0usize;
        for out in parse_hook_labels(&response, &inputs) {
            let Some(cue) = out.cue else { continue };
            let Some(b) = batch
                .iter()
                .find(|b| b.key_gram == out.key_gram && b.answer.eq_ignore_ascii_case(&out.answer))
            else {
                continue;
            };
            sqlx::query("UPDATE pavlov_hooks SET cue = $2, source = 'model', model = $3 WHERE id = $1 AND cue IS NULL")
                .bind(b.id)
                .bind(&cue)
                .bind(HOOK_LABEL_MODEL)
                .execute(&state.pool)
                .await?;
            labeled += 1;
        }
        tracing::info!("objects hooks: model batch of {} → {labeled} labeled", batch.len());
    }
}

/// Mined hooks by support, vetted-only hooks after them.
async fn rerank(state: &Arc<AppState>) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE pavlov_hooks h SET rank = r.rn
         FROM (SELECT id, row_number() OVER (PARTITION BY answer_id
                 ORDER BY (source = 'vetted') ASC, support DESC, id) AS rn
               FROM pavlov_hooks WHERE status = 'active') r
         WHERE r.id = h.id",
    )
    .execute(&state.pool)
    .await?;
    Ok(())
}
