//! Pavlov cue mining: candidate pruning and LLM surface-form rendering
//! (docs/superpowers/specs/2026-07-21-pavlov-cues-design.md).

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
const TERMS_RAW_LIMIT: i64 = 24; // fetched from SQL before the self-term filter
const TERMS_KEPT: usize = 8;
const POLISH_BATCH: i64 = 15;

#[derive(sqlx::FromRow)]
struct Candidate {
    norm: String,
    display: String,
    freq: i32,
}

/// Top unmined answers for one category. `recency=false` ranks by answer_freq;
/// `recency=true` by summed 6-year-half-life decay (mock-test constant).
///
/// STUB (Task 4): v1 body pruned along with `MIN_FREQ`; Task 5 replaces this
/// stage's SQL wholesale, so it is left as a placeholder rather than patched.
#[allow(unused_variables, dead_code)]
async fn select_candidates(
    state: &Arc<AppState>,
    category: &str,
    seats: i64,
    recency: bool,
) -> Result<Vec<Candidate>, AppError> {
    todo!("replaced wholesale in Task 5")
}

/// Distinctive clue lexemes for one answer: TF within the answer's clues ×
/// log-inverse document frequency corpus-wide, minus the answer's own lexemes.
///
/// STUB (Task 4): v1 body called the now-deleted `filter_self_terms`; Task 5
/// replaces this stage's SQL wholesale, so it is left as a placeholder.
#[allow(unused_variables, dead_code)]
async fn mine_terms(
    state: &Arc<AppState>,
    norm: &str,
    display: &str,
    total_docs: f64,
) -> Result<Vec<String>, AppError> {
    todo!("replaced wholesale in Task 5")
}

/// The 3 most recent clue ids for an answer (reveal examples).
async fn example_ids(state: &Arc<AppState>, norm: &str) -> Result<Vec<i32>, AppError> {
    let sql = format!(
        "SELECT jq.id FROM jeopardy_questions jq
         WHERE jq.archived = false AND jq.question IS NOT NULL AND {NORM_EXPR} = $1
         ORDER BY jq.air_date DESC NULLS LAST
         LIMIT 3"
    );
    let rows: Vec<(i32,)> = sqlx::query_as(&sql).bind(norm).fetch_all(&state.pool).await?;
    Ok(rows.into_iter().map(|(i,)| i).collect())
}

/// Stage A: fill every category's seats with mined 'pending' rows.
///
/// STUB (Task 4): v1 body depended on the removed `seat_plan`/`TOTAL_SEATS`;
/// Task 5 replaces this stage wholesale.
#[allow(unused_variables, dead_code)]
async fn mine_stage(state: &Arc<AppState>) -> Result<(), AppError> {
    todo!("replaced wholesale in Task 5")
}

/// Stage B: polish pending rows in batches; each batch is upserted before the
/// next call, so an interrupted run resumes where it left off.
///
/// STUB (Task 4): v1 body depended on the removed `PolishInput`/
/// `polish_prompts`/`parse_polish_response`; Task 5 replaces this stage
/// wholesale with the render-based contract.
#[allow(unused_variables, dead_code)]
async fn polish_stage(state: &Arc<AppState>) -> Result<(), AppError> {
    todo!("replaced wholesale in Task 5")
}

#[allow(dead_code)]
async fn chat_json_with_model(
    state: &Arc<AppState>,
    system: &str,
    user: &str,
) -> Result<serde_json::Value, AppError> {
    crate::openai::chat_json(&state.config.openai_api_key, "gpt-4o", system, user, 0.3).await
}

/// Full generation run: mine then polish. Both stages are idempotent/resumable.
pub async fn run_generation(state: &Arc<AppState>) -> Result<(), AppError> {
    mine_stage(state).await?;
    polish_stage(state).await
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
