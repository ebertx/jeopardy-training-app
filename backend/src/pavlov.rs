//! Pavlov cue mining: candidate pruning and LLM surface-form rendering
//! (docs/superpowers/specs/2026-07-22-pavlov-cues-v2-design.md).

/// True when a cue phrase gives the answer away: the phrase contains the
/// whole answer (article-stripped, as a contiguous word sequence) or any
/// single answer word of >= 4 chars. Token-based, case-insensitive — token
/// equality gives word-boundary semantics without a regex dependency.
pub fn phrase_leaks_answer(answer: &str, phrase: &str) -> bool {
    fn tokens(s: &str) -> Vec<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(|w| w.to_string())
            .collect()
    }
    let mut answer_tokens = tokens(answer);
    if matches!(answer_tokens.first().map(|s| s.as_str()), Some("a" | "an" | "the"))
        && answer_tokens.len() > 1
    {
        answer_tokens.remove(0);
    }
    if answer_tokens.is_empty() {
        return false;
    }
    let phrase_tokens = tokens(phrase);
    let whole = phrase_tokens
        .windows(answer_tokens.len())
        .any(|w| w == answer_tokens.as_slice());
    let word = answer_tokens
        .iter()
        .any(|a| a.len() >= 4 && phrase_tokens.iter().any(|p| p == a));
    whole || word
}

#[derive(Debug, Clone, PartialEq)]
pub struct CueCandidate {
    pub answer_norm: String,
    pub gram: String,
    pub n: i16,
    pub support: i64,
    pub total: i64,
    pub prec: f64,
}

/// Redundancy pruning within each answer: when one gram's token set is a
/// subset of another's (e.g. "wood" vs "milk wood"), keep the higher score
/// (support * prec); on a tie keep the gram with more tokens (more specific).
/// Grams of different answers never prune each other.
pub fn prune_redundant(cands: Vec<CueCandidate>) -> Vec<CueCandidate> {
    use std::collections::HashSet;
    let toks: Vec<HashSet<&str>> = cands
        .iter()
        .map(|c| c.gram.split(' ').collect())
        .collect();
    let score = |c: &CueCandidate| c.support as f64 * c.prec;
    let mut dropped = vec![false; cands.len()];
    for i in 0..cands.len() {
        for j in 0..cands.len() {
            if i == j || dropped[i] || dropped[j] {
                continue;
            }
            if cands[i].answer_norm != cands[j].answer_norm {
                continue;
            }
            if !toks[i].is_subset(&toks[j]) && !toks[j].is_subset(&toks[i]) {
                continue;
            }
            // i and j are token-related: drop the weaker.
            let (si, sj) = (score(&cands[i]), score(&cands[j]));
            let drop_i = if si != sj {
                si < sj
            } else {
                toks[i].len() < toks[j].len()
            };
            if drop_i {
                dropped[i] = true;
            } else {
                dropped[j] = true;
            }
        }
    }
    cands
        .into_iter()
        .zip(dropped)
        .filter(|(_, d)| !d)
        .map(|(c, _)| c)
        .collect()
}

#[derive(Debug, Clone)]
pub struct RenderInput {
    pub answer: String,
    pub gram: String,
    pub sample_clues: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RenderOutcome {
    pub answer: String,
    pub gram: String,
    pub keep: bool,
    pub display: String,
}

/// (system, user) prompts for one render batch. Cosmetic-only contract: the
/// LLM restores a stemmed phrase to its natural surface form, nothing more.
pub fn render_prompts(batch: &[RenderInput]) -> (String, String) {
    let system = "You restore stemmed Jeopardy! clue phrases to their natural surface form. \
For each item you receive a stemmed phrase, the answer it cues, and real clues containing \
the phrase. Return the phrase as it naturally appears in the clues \
(e.g. 'welsh poet' -> 'Welsh poet', 'go gentl' -> 'go gentle'). Render ONLY the given \
phrase — do not add other words or information, and NEVER include the answer or any word \
of the answer in the rendering. Set keep=false only when no natural rendering exists. \
Respond with JSON only: {\"results\": [{\"answer\": string (echoed verbatim), \
\"gram\": string (echoed verbatim), \"keep\": boolean, \"display\": string}]}"
        .to_string();
    let items: Vec<serde_json::Value> = batch
        .iter()
        .map(|b| {
            serde_json::json!({
                "answer": b.answer,
                "gram": b.gram,
                "sample_clues": b.sample_clues,
            })
        })
        .collect();
    let user = serde_json::to_string_pretty(&serde_json::json!({ "phrases": items }))
        .expect("serializable");
    (system, user)
}

/// Lenient parse of the render response: items missing answer or gram are
/// skipped; a blank display or one that leaks the answer demotes to dropped.
pub fn parse_render_response(v: &serde_json::Value) -> Vec<RenderOutcome> {
    let Some(results) = v.get("results").and_then(|r| r.as_array()) else {
        return vec![];
    };
    results
        .iter()
        .filter_map(|item| {
            let answer = item.get("answer")?.as_str()?.trim().to_string();
            let gram = item.get("gram")?.as_str()?.trim().to_string();
            if answer.is_empty() || gram.is_empty() {
                return None;
            }
            let display = item
                .get("display")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let keep = item.get("keep").and_then(|k| k.as_bool()).unwrap_or(true)
                && !display.is_empty()
                && !phrase_leaks_answer(&answer, &display);
            Some(RenderOutcome { answer, gram, keep, display })
        })
        .collect()
}

use std::sync::Arc;

use crate::error::AppError;
use crate::AppState;

/// 0008's normalization of the response text, verbatim.
const NORM_EXPR: &str = "lower(trim(regexp_replace(jq.question, '^(the|a|an) ', '', 'i')))";

// Thresholds fixed at the 2026-07-22 preview gate (spec §3).
pub const BIGRAM_MIN_SUPPORT: i64 = 4;
pub const BIGRAM_MIN_PREC: f64 = 0.5;
pub const UNIGRAM_MIN_SUPPORT: i64 = 6;
pub const UNIGRAM_MIN_PREC: f64 = 0.6;
const MIN_ANSWER_FREQ: i32 = 4;
const RENDER_BATCH: i64 = 15;
pub const POLISH_MODEL: &str = "gpt-4o";

#[derive(sqlx::FromRow)]
struct CandidateRow {
    answer_norm: String,
    gram: String,
    n: i16,
    support: i64,
    total: i64,
    prec: f64,
}

/// All qualifying (gram, answer) pairs. Support is counted within eligible
/// answers; totals (precision denominators) over the full corpus.
async fn candidate_rows(state: &Arc<AppState>) -> Result<Vec<CueCandidate>, AppError> {
    let sql = format!(
        "WITH eligible AS (
           SELECT {NORM_EXPR} AS norm
           FROM jeopardy_questions jq
           WHERE jq.archived = false AND jq.question IS NOT NULL
           GROUP BY 1 HAVING max(jq.answer_freq) >= $5
         ), sup AS (
           SELECT g.answer_norm, g.gram, g.n, count(DISTINCT g.clue_id) AS support
           FROM pavlov_clue_ngrams g
           WHERE g.answer_norm IN (SELECT norm FROM eligible)
           GROUP BY 1, 2, 3
           HAVING count(DISTINCT g.clue_id) >= LEAST($1, $3)
         ), tot AS (
           SELECT g.gram, count(DISTINCT g.clue_id) AS total
           FROM pavlov_clue_ngrams g
           WHERE g.gram IN (SELECT DISTINCT gram FROM sup)
           GROUP BY 1
         )
         SELECT s.answer_norm, s.gram, s.n, s.support, t.total,
                s.support::float8 / t.total AS prec
         FROM sup s JOIN tot t USING (gram)
         WHERE (s.n = 2 AND s.support >= $1 AND s.support::float8 / t.total >= $2)
            OR (s.n = 1 AND s.support >= $3 AND s.support::float8 / t.total >= $4)"
    );
    let rows: Vec<CandidateRow> = sqlx::query_as(&sql)
        .bind(BIGRAM_MIN_SUPPORT)
        .bind(BIGRAM_MIN_PREC)
        .bind(UNIGRAM_MIN_SUPPORT)
        .bind(UNIGRAM_MIN_PREC)
        .bind(MIN_ANSWER_FREQ)
        .fetch_all(&state.pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|r| CueCandidate {
            answer_norm: r.answer_norm,
            gram: r.gram,
            n: r.n,
            support: r.support,
            total: r.total,
            prec: r.prec,
        })
        .collect())
}

/// Stage A: select candidates, filter leaks, prune redundancy, insert
/// 'pending' rows. Idempotent via ON CONFLICT DO NOTHING.
async fn mine_stage(state: &Arc<AppState>) -> Result<(), AppError> {
    let raw = candidate_rows(state).await?;
    let unleaky: Vec<CueCandidate> = raw
        .into_iter()
        .filter(|c| !phrase_leaks_answer(&c.answer_norm, &c.gram))
        .collect();
    let kept = prune_redundant(unleaky);
    tracing::info!("pavlov v2 mine: {} cues after leak filter + pruning", kept.len());

    for c in kept {
        let (display, category): (String, Option<String>) = {
            let sql = format!(
                "SELECT mode() WITHIN GROUP (ORDER BY jq.question),
                        mode() WITHIN GROUP (ORDER BY jq.classifier_category)
                 FROM jeopardy_questions jq
                 WHERE jq.archived = false AND jq.question IS NOT NULL
                   AND {NORM_EXPR} = $1"
            );
            sqlx::query_as(&sql).bind(&c.answer_norm).fetch_one(&state.pool).await?
        };
        let examples: Vec<(i32,)> = sqlx::query_as(
            "SELECT g.clue_id FROM pavlov_clue_ngrams g
             JOIN jeopardy_questions jq ON jq.id = g.clue_id
             WHERE g.answer_norm = $1 AND g.gram = $2
             GROUP BY g.clue_id, jq.air_date
             ORDER BY jq.air_date DESC NULLS LAST LIMIT 3",
        )
        .bind(&c.answer_norm)
        .bind(&c.gram)
        .fetch_all(&state.pool)
        .await?;
        let example_ids: Vec<i32> = examples.into_iter().map(|(i,)| i).collect();

        sqlx::query(
            "INSERT INTO pavlov_cues
               (answer, answer_norm, meta_category, cue_stem, support, total, prec, example_clue_ids)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (answer_norm, cue_stem) DO NOTHING",
        )
        .bind(&display)
        .bind(&c.answer_norm)
        .bind(category.unwrap_or_else(|| "Miscellaneous".to_string()))
        .bind(&c.gram)
        .bind(c.support as i32)
        .bind(c.total as i32)
        .bind(c.prec as f32)
        .bind(&example_ids)
        .execute(&state.pool)
        .await?;
    }
    Ok(())
}

/// Stage B: render surface forms for pending cues in batches; upserts per
/// batch so an interrupted run resumes where it left off.
async fn render_stage(state: &Arc<AppState>) -> Result<(), AppError> {
    loop {
        let batch: Vec<(i32, String, String, Vec<i32>)> = sqlx::query_as(
            "SELECT id, answer, cue_stem, example_clue_ids
             FROM pavlov_cues WHERE status = 'pending' ORDER BY id LIMIT $1",
        )
        .bind(RENDER_BATCH)
        .fetch_all(&state.pool)
        .await?;
        if batch.is_empty() {
            return Ok(());
        }

        let mut inputs = Vec::with_capacity(batch.len());
        for (_, answer, gram, ex_ids) in &batch {
            let clues: Vec<(String,)> = sqlx::query_as(
                "SELECT coalesce(answer, '') FROM jeopardy_questions WHERE id = ANY($1) LIMIT 2",
            )
            .bind(&ex_ids[..])
            .fetch_all(&state.pool)
            .await?;
            inputs.push(RenderInput {
                answer: answer.clone(),
                gram: gram.clone(),
                sample_clues: clues.into_iter().map(|(c,)| c).collect(),
            });
        }

        let (system, user) = render_prompts(&inputs);
        let response =
            crate::openai::chat_json(&state.config.openai_api_key, POLISH_MODEL, &system, &user, 0.3)
                .await?;
        let outcomes = parse_render_response(&response);

        let mut updated = 0;
        for out in &outcomes {
            let key = (out.answer.to_lowercase(), out.gram.as_str());
            let Some((id, ..)) = batch
                .iter()
                .find(|(_, a, g, _)| (a.to_lowercase(), g.as_str()) == key)
            else {
                continue;
            };
            let status = if out.keep { "active" } else { "dropped" };
            sqlx::query(
                "UPDATE pavlov_cues SET status = $2, cue_display = $3, model = $4
                 WHERE id = $1 AND status = 'pending'",
            )
            .bind(id)
            .bind(status)
            .bind(&out.display)
            .bind(POLISH_MODEL)
            .execute(&state.pool)
            .await?;
            updated += 1;
        }
        if updated == 0 {
            let ids: Vec<i32> = batch.iter().map(|(id, ..)| *id).collect();
            sqlx::query(
                "UPDATE pavlov_cues SET status = 'dropped', model = $2
                 WHERE id = ANY($1) AND status = 'pending'",
            )
            .bind(&ids)
            .bind(POLISH_MODEL)
            .execute(&state.pool)
            .await?;
            tracing::warn!("pavlov render: batch of {} unmatched, dropped", ids.len());
        }
    }
}

/// Full v2 generation: mine then render. Both stages idempotent/resumable.
pub async fn run_generation(state: &Arc<AppState>) -> Result<(), AppError> {
    mine_stage(state).await?;
    render_stage(state).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phrase_leaks_on_whole_answer_even_when_short() {
        // Whole answer as a standalone word leaks, regardless of length.
        assert!(phrase_leaks_answer("13", "13 stripes on flag"));
        assert!(phrase_leaks_answer("a unicorn", "lion and unicorn rhyme"));
        assert!(phrase_leaks_answer("Gibraltar", "Rock of Gibraltar"));
    }

    #[test]
    fn phrase_leaks_on_long_answer_word_but_not_prefix_or_short() {
        // A >=4-char word of the answer leaks even without the full answer.
        assert!(phrase_leaks_answer("Ernest Hemingway", "Hemingway's Paris years"));
        // Prefix inside a longer word is not a leak ("D" vs "Deschanel").
        assert!(!phrase_leaks_answer("D", "Zooey Deschanel's movie title"));
        // Short words (<4) from a multi-word answer don't count on their own.
        assert!(!phrase_leaks_answer("Tin Pan Alley", "pan flute music"));
    }

    #[test]
    fn phrase_not_leaking_for_ordinary_cues() {
        assert!(!phrase_leaks_answer("Solomon", "wise king"));
        assert!(!phrase_leaks_answer("Solomon", "Ecclesiastes ascribed to"));
        assert!(!phrase_leaks_answer("a pearl", "June birthstone"));
    }

    fn cand(answer: &str, gram: &str, n: i16, support: i64, total: i64) -> CueCandidate {
        CueCandidate {
            answer_norm: answer.to_string(),
            gram: gram.to_string(),
            n,
            support,
            total,
            prec: support as f64 / total as f64,
        }
    }

    #[test]
    fn prune_drops_token_subset_with_lower_score() {
        // "wood" (7/12 = 4.08 score) is a token-subset of "milk wood"
        // (6/7 = 5.14 score) for the same answer -> keep "milk wood".
        let out = prune_redundant(vec![
            cand("dylan thomas", "milk wood", 2, 6, 7),
            cand("dylan thomas", "wood", 1, 7, 12),
        ]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].gram, "milk wood");
    }

    #[test]
    fn prune_keeps_more_specific_gram_on_score_tie() {
        let out = prune_redundant(vec![
            cand("solomon", "wise", 1, 6, 12),
            cand("solomon", "wise king", 2, 6, 12),
        ]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].gram, "wise king");
    }

    #[test]
    fn prune_keeps_unrelated_grams_and_other_answers() {
        let out = prune_redundant(vec![
            cand("dylan thomas", "welsh poet", 2, 19, 25),
            cand("dylan thomas", "fern hill", 2, 6, 6),
            cand("solomon", "wise king", 2, 15, 17),
            // same-token unigram but for a DIFFERENT answer: not pruned
            cand("robert frost", "poet", 1, 9, 14),
        ]);
        assert_eq!(out.len(), 4);
    }

    #[test]
    fn render_prompts_carry_gram_answer_and_clues_and_demand_json() {
        let batch = vec![RenderInput {
            answer: "Dylan Thomas".into(),
            gram: "welsh poet".into(),
            sample_clues: vec!["This Welsh poet wrote 'Fern Hill'".into()],
        }];
        let (system, user) = render_prompts(&batch);
        assert!(system.contains("JSON"));
        assert!(system.to_lowercase().contains("never include the answer"));
        assert!(user.contains("welsh poet"));
        assert!(user.contains("Dylan Thomas"));
        assert!(user.contains("Fern Hill"));
    }

    #[test]
    fn parse_render_accepts_wellformed_and_drops_leaky_or_empty() {
        let v = serde_json::json!({
            "results": [
                { "answer": "Dylan Thomas", "gram": "welsh poet",
                  "keep": true, "display": "Welsh poet" },
                { "answer": "Dylan Thomas", "gram": "go gentl",
                  "keep": true, "display": "Dylan's go gentle" }, // leaks answer word
                { "answer": "Solomon", "gram": "wise king",
                  "keep": true, "display": "  " },                // empty render
                { "gram": "orphan", "keep": true, "display": "x" } // no answer: skipped
            ]
        });
        let out = parse_render_response(&v);
        assert_eq!(out.len(), 3);
        assert!(out[0].keep);
        assert_eq!(out[0].display, "Welsh poet");
        assert!(!out[1].keep, "display containing an answer word is demoted");
        assert!(!out[2].keep, "blank display is demoted");
    }

    #[test]
    fn parse_render_of_garbage_is_empty() {
        assert!(parse_render_response(&serde_json::json!({"nope": 1})).is_empty());
        assert!(parse_render_response(&serde_json::json!("string")).is_empty());
    }
}
