//! Answer sheets: a corpus-grounded identity + four drillable facts per answer.
//! Pure logic here (prompt, parsing, normalization); generation in the second half.

use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;
use crate::error::AppError;
use crate::AppState;

pub const SHEET_MODEL: &str = "gpt-4o-mini";

pub const SHEET_SYSTEM_PROMPT: &str = r#"You are a Jeopardy! coach building an ANSWER SHEET: the handful of facts that let a player recognize this answer from any clue angle.

Rules:
- Output ONLY valid JSON: {"identity": "...", "facts": [{"prompt": "...", "response": "..."}, ...]} with EXACTLY four facts.
- "identity": one sentence, at most 25 words, saying who or what this answer is. Proper nouns, dates and numbers only. Never restate a clue.
- Each fact is something Jeopardy writers re-use about this answer, drawn from the supplied clues where possible: creator or author, signature work, key date or first, place, counterpart or rival, nickname, unit or symbol.
- "prompt" is a 2-6 word label as it would appear in a cue: "author", "narrator", "year published", "capital city", "wrote the music".
- "response" is a short gradeable answer — a name, number, place or title — at most 6 words. Never a sentence. Never the answer itself.
- The four facts must be distinct from each other and must NOT repeat the existing cue phrases you are given; they add depth beyond the cue.
- BANNED: generic sentences about trivia, Jeopardy or "recognizing"; hedges such as "possibly" or "often"."#;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Fact {
    pub prompt: String,
    pub response: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SheetContent {
    pub identity: String,
    pub facts: Vec<Fact>,
}

/// A clue used to ground the sheet. `clue` is the clue text shown to players
/// (jeopardy_questions.answer), `year` from air_date.
#[derive(Debug, Clone)]
pub struct GroundingClue {
    pub clue: String,
    pub category: String,
    pub year: Option<i32>,
}

/// Rust twin of the SQL `lower(trim(regexp_replace(question, '^(the|a|an) ', '', 'i')))`.
/// Strips ONE leading article (followed by a space), lowercases, trims.
pub fn normalize_answer(s: &str) -> String {
    let lower = s.to_lowercase();
    let stripped = ["the ", "an ", "a "]
        .iter()
        .find_map(|p| lower.strip_prefix(p))
        .unwrap_or(&lower);
    stripped.trim().to_string()
}

/// answer_norm of the n-th fact card under a parent (n = 1..=4).
pub fn fact_norm(parent_norm: &str, n: usize) -> String {
    format!("{parent_norm}::{n}")
}

pub fn sheet_user_prompt(
    answer: &str,
    category: &str,
    phrases: &[String],
    clues: &[GroundingClue],
) -> String {
    let mut out = format!("Answer: \"{answer}\"\nCategory: {category}\n");
    out.push_str(&format!(
        "Existing cue phrases: {}\n",
        if phrases.is_empty() { "(none)".to_string() } else { phrases.join(", ") }
    ));
    out.push_str("Clues Jeopardy has written for this answer:\n");
    for c in clues {
        match c.year {
            Some(y) => out.push_str(&format!("- \"{}\" ({}, {})\n", c.clue, c.category, y)),
            None => out.push_str(&format!("- \"{}\" ({})\n", c.clue, c.category)),
        }
    }
    out.push_str("\nReturn the JSON now.");
    out
}

/// Validate the LLM's JSON. Rejections are never cached.
pub fn parse_sheet(v: &Value, answer_norm: &str) -> Result<SheetContent, String> {
    let identity = v
        .get("identity")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("missing or empty identity")?
        .to_string();
    let arr = v.get("facts").and_then(|f| f.as_array()).ok_or("missing facts")?;
    if arr.len() != 4 {
        return Err(format!("expected 4 facts, got {}", arr.len()));
    }
    let mut facts = Vec::with_capacity(4);
    let mut seen_prompts = std::collections::HashSet::new();
    for f in arr {
        let get = |k: &str| -> Result<String, String> {
            let s = f
                .get(k)
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| format!("fact missing {k}"))?;
            Ok(s.to_string())
        };
        let prompt = get("prompt")?;
        let response = get("response")?;
        if normalize_answer(&response) == answer_norm {
            return Err(format!("fact response restates the answer: {response}"));
        }
        if !seen_prompts.insert(prompt.to_lowercase()) {
            return Err(format!("duplicate fact prompt: {prompt}"));
        }
        facts.push(Fact { prompt, response });
    }
    Ok(SheetContent { identity, facts })
}

/// Pick `n` items spread evenly across a chronologically ordered slice, always
/// keeping the first and last. Returns everything when the slice has ≤ n items
/// (this also covers `n == 0`, which returns everything). `n == 1` returns just
/// the first item.
pub fn sample_evenly<T: Clone>(items: &[T], n: usize) -> Vec<T> {
    if items.len() <= n || n == 0 {
        return items.to_vec();
    }
    if n == 1 {
        return vec![items[0].clone()];
    }
    let last = items.len() - 1;
    (0..n)
        .map(|i| items[(i * last) / (n - 1)].clone())
        .collect()
}

pub fn sheet_json(s: &SheetContent) -> Value {
    serde_json::json!({ "identity": s.identity, "facts": s.facts })
}

#[derive(sqlx::FromRow)]
struct ParentRow {
    answer: String,
    meta_category: String,
    phrases: Vec<String>,
}

#[derive(sqlx::FromRow, Clone)]
struct ClueRow {
    clue: Option<String>,
    response: Option<String>,
    category: Option<String>,
    classifier_category: Option<String>,
    year: Option<i32>,
}

/// Cached-or-generate. Ok(None) when the key is unconfigured, the answer has
/// no clues in the corpus, or the LLM output failed validation. Single-flight
/// per answer_norm, same shape as insights::ensure_insight.
pub async fn ensure_sheet(
    state: &Arc<AppState>,
    answer_norm: &str,
) -> Result<Option<SheetContent>, AppError> {
    if let Some(c) = read_cached(state, answer_norm).await? {
        return Ok(Some(c));
    }
    if state.config.openai_api_key.is_empty() {
        return Ok(None);
    }
    // Negative cache: an answer whose generation was already rejected this
    // process (parser rejection or empty corpus) is not retried on every
    // serve. Bounded by process lifetime only — a restart retries. Never
    // tombstoned in the database; a future model or corpus change may succeed.
    if state.sheet_failed.lock().await.contains(answer_norm) {
        return Ok(None);
    }
    {
        let mut inflight = state.sheet_inflight.lock().await;
        if !inflight.insert(answer_norm.to_string()) {
            drop(inflight);
            for _ in 0..20 {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                if let Some(c) = read_cached(state, answer_norm).await? {
                    return Ok(Some(c));
                }
            }
            return Ok(None);
        }
    }
    let result = generate_and_store(state, answer_norm).await;
    state.sheet_inflight.lock().await.remove(answer_norm);
    result
}

/// Fire-and-forget warm-up (used by drill_next so the sheet is ready by reveal).
pub fn pregenerate_sheet(state: &Arc<AppState>, answer_norm: String) {
    if state.config.openai_api_key.is_empty() {
        return;
    }
    let st = state.clone();
    tokio::spawn(async move {
        if let Err(e) = ensure_sheet(&st, &answer_norm).await {
            tracing::warn!("sheet pregeneration failed for {answer_norm}: {e:?}");
        }
    });
}

/// Cached-only lookup: no generation, no single-flight polling. Used where a
/// slow/failing LLM call must never block or fail the caller (e.g. grading).
pub async fn cached_sheet(state: &Arc<AppState>, answer_norm: &str) -> Result<Option<SheetContent>, AppError> {
    read_cached(state, answer_norm).await
}

/// Display answer for a norm: the answer stored with the sheet (the deck's
/// display form when the deck row existed at generation time).
pub async fn display_answer(state: &Arc<AppState>, answer_norm: &str) -> Result<Option<String>, AppError> {
    let s: Option<String> = sqlx::query_scalar(
        "SELECT answer FROM answer_sheets WHERE answer_norm = $1",
    )
    .bind(answer_norm)
    .fetch_optional(&state.pool)
    .await?;
    Ok(s)
}

/// True when the user already holds all four fact cards for this answer.
pub async fn facts_added(state: &Arc<AppState>, user_id: i32, answer_norm: &str) -> Result<bool, AppError> {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pavlov_cards ca
         JOIN pavlov_answers pa ON pa.id = ca.answer_id
         WHERE ca.user_id = $1 AND pa.kind = 'fact' AND pa.parent_norm = $2",
    )
    .bind(user_id)
    .bind(answer_norm)
    .fetch_one(&state.pool)
    .await?;
    Ok(n >= 4)
}

async fn read_cached(state: &Arc<AppState>, answer_norm: &str) -> Result<Option<SheetContent>, AppError> {
    let row: Option<(Value,)> =
        sqlx::query_as("SELECT content FROM answer_sheets WHERE answer_norm = $1")
            .bind(answer_norm)
            .fetch_optional(&state.pool)
            .await?;
    Ok(row.and_then(|(v,)| parse_sheet(&v, answer_norm).ok()))
}

async fn generate_and_store(
    state: &Arc<AppState>,
    answer_norm: &str,
) -> Result<Option<SheetContent>, AppError> {
    // Up to 15 clues spread across air dates; the norm join uses the same
    // expression as idx_jq_answer_norm so it is index-backed.
    let all: Vec<ClueRow> = sqlx::query_as(
        "SELECT answer AS clue, question AS response, category, classifier_category,
                EXTRACT(year FROM air_date)::int AS year
         FROM jeopardy_questions
         WHERE lower(trim(regexp_replace(question, '^(the|a|an) ', '', 'i'))) = $1
           AND archived = false AND answer IS NOT NULL AND question IS NOT NULL
         ORDER BY air_date",
    )
    .bind(answer_norm)
    .fetch_all(&state.pool)
    .await?;
    if all.is_empty() {
        state.sheet_failed.lock().await.insert(answer_norm.to_string());
        return Ok(None);
    }
    let picked = sample_evenly(&all, 15);

    let parent: Option<ParentRow> = sqlx::query_as(
        "SELECT answer, meta_category, phrases FROM pavlov_answers
         WHERE answer_norm = $1 AND kind = 'answer'",
    )
    .bind(answer_norm)
    .fetch_optional(&state.pool)
    .await?;
    let (answer, category, phrases) = match parent {
        Some(p) => (p.answer, p.meta_category, p.phrases),
        None => {
            // Prefer the latest DATED clue as the fallback display row; `all`
            // is ordered by air_date (NULLs last in Postgres), so an
            // undated row would otherwise win as "last" despite not being
            // the most recent.
            let fallback = all.iter().rev().find(|c| c.year.is_some()).unwrap_or(&all[all.len() - 1]);
            (
                fallback.response.clone().unwrap_or_else(|| answer_norm.to_string()),
                fallback.classifier_category.clone().unwrap_or_else(|| "Miscellaneous".to_string()),
                vec![],
            )
        }
    };
    let clues: Vec<GroundingClue> = picked
        .into_iter()
        .map(|c| GroundingClue {
            clue: c.clue.unwrap_or_default(),
            category: c.category.unwrap_or_else(|| "UNKNOWN".to_string()),
            year: c.year,
        })
        .collect();

    let user = sheet_user_prompt(&answer, &category, &phrases, &clues);
    let v = crate::openai::chat_json(
        &state.config.openai_api_key,
        SHEET_MODEL,
        SHEET_SYSTEM_PROMPT,
        &user,
        0.4,
    )
    .await?;
    let content = match parse_sheet(&v, answer_norm) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("sheet rejected for {answer_norm}: {e}");
            state.sheet_failed.lock().await.insert(answer_norm.to_string());
            return Ok(None);
        }
    };
    sqlx::query(
        "INSERT INTO answer_sheets (answer_norm, answer, content, model) VALUES ($1, $2, $3, $4)
         ON CONFLICT (answer_norm) DO NOTHING",
    )
    .bind(answer_norm)
    .bind(&answer)
    .bind(sheet_json(&content))
    .bind(SHEET_MODEL)
    .execute(&state.pool)
    .await?;
    Ok(Some(content))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn good() -> Value {
        json!({
            "identity": "Emily Brontë's only novel, 1847, a doomed romance on the Yorkshire moors.",
            "facts": [
                {"prompt": "author", "response": "Emily Brontë"},
                {"prompt": "antihero", "response": "Heathcliff"},
                {"prompt": "setting", "response": "Yorkshire moors"},
                {"prompt": "year published", "response": "1847"}
            ]
        })
    }

    #[test]
    fn normalize_matches_the_sql_expression() {
        assert_eq!(normalize_answer("The Jungle"), "jungle");
        assert_eq!(normalize_answer("An Apple"), "apple");
        assert_eq!(normalize_answer("A Midsummer Night's Dream"), "midsummer night's dream");
        assert_eq!(normalize_answer("Aesop"), "aesop"); // no article stripped
        assert_eq!(normalize_answer("  Wuthering Heights "), "wuthering heights");
    }

    #[test]
    fn fact_norm_shape() {
        assert_eq!(fact_norm("wuthering heights", 3), "wuthering heights::3");
    }

    #[test]
    fn parse_accepts_a_valid_sheet() {
        let s = parse_sheet(&good(), "wuthering heights").unwrap();
        assert_eq!(s.facts.len(), 4);
        assert_eq!(s.facts[0].prompt, "author");
        assert_eq!(s.facts[3].response, "1847");
        assert!(s.identity.starts_with("Emily"));
    }

    #[test]
    fn parse_rejects_wrong_fact_count() {
        let mut v = good();
        v["facts"].as_array_mut().unwrap().pop();
        assert!(parse_sheet(&v, "wuthering heights").is_err());
        let mut v = good();
        v["facts"].as_array_mut().unwrap().push(json!({"prompt": "x", "response": "y"}));
        assert!(parse_sheet(&v, "wuthering heights").is_err());
    }

    #[test]
    fn parse_rejects_empty_fields() {
        let mut v = good();
        v["facts"][1]["response"] = json!("  ");
        assert!(parse_sheet(&v, "wuthering heights").is_err());
        let mut v = good();
        v["identity"] = json!("");
        assert!(parse_sheet(&v, "wuthering heights").is_err());
    }

    #[test]
    fn parse_rejects_response_equal_to_answer() {
        let mut v = good();
        v["facts"][2]["response"] = json!("The Wuthering Heights");
        assert!(parse_sheet(&v, "wuthering heights").is_err());
    }

    #[test]
    fn parse_rejects_duplicate_prompts() {
        let mut v = good();
        v["facts"][2]["prompt"] = json!("Author");
        assert!(parse_sheet(&v, "wuthering heights").is_err());
    }

    #[test]
    fn user_prompt_lists_answer_phrases_and_clues() {
        let clues = vec![
            GroundingClue { clue: "Heathcliff loves Cathy here".into(), category: "NOVELS".into(), year: Some(2001) },
            GroundingClue { clue: "Emily's only novel".into(), category: "BRONTËS".into(), year: None },
        ];
        let p = sheet_user_prompt("Wuthering Heights", "Literature & Language", &["Heathcliff".to_string()], &clues);
        assert!(p.contains("Answer: \"Wuthering Heights\""));
        assert!(p.contains("Category: Literature & Language"));
        assert!(p.contains("Existing cue phrases: Heathcliff"));
        assert!(p.contains("- \"Heathcliff loves Cathy here\" (NOVELS, 2001)"));
        assert!(p.contains("- \"Emily's only novel\" (BRONTËS)"));
        assert!(p.trim_end().ends_with("Return the JSON now."));
    }

    #[test]
    fn sample_evenly_keeps_ends_and_count() {
        let v: Vec<i32> = (0..30).collect();
        let s = sample_evenly(&v, 15);
        assert_eq!(s.len(), 15);
        assert_eq!(s[0], 0);
        assert_eq!(*s.last().unwrap(), 29);
        assert_eq!(sample_evenly(&v, 100).len(), 30); // fewer than n → all
        assert!(sample_evenly(&Vec::<i32>::new(), 5).is_empty());
    }

    #[test]
    fn sample_evenly_n_one_does_not_divide_by_zero() {
        let v: Vec<i32> = (0..30).collect();
        assert_eq!(sample_evenly(&v, 1), vec![0]);
    }

    #[test]
    fn sheet_json_round_trips() {
        let s = parse_sheet(&good(), "wuthering heights").unwrap();
        let v = sheet_json(&s);
        assert_eq!(v["facts"].as_array().unwrap().len(), 4);
        assert_eq!(v["facts"][0]["prompt"], "author");
        assert!(parse_sheet(&v, "wuthering heights").is_ok());
    }
}
