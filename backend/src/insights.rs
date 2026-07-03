//! Per-clue insights: why the answer is what it is, plus a memory hook.
//! Generated once per clue (global cache), pregenerated at serve time.
use std::sync::Arc;
use serde_json::Value;

use crate::error::AppError;
use crate::AppState;

pub const INSIGHT_MODEL: &str = "gpt-4o-mini";

pub const INSIGHT_SYSTEM_PROMPT: &str = r#"You are a Jeopardy! coach. Given one clue and its correct response, arm the player for FUTURE clues about this answer.

Rules:
- Output ONLY valid JSON: {"insight": "...", "hook": "..."}
- "insight": 45-75 words, two parts. Part 1 (one sentence): the specific fact that links THIS clue to the response. Part 2 (begin exactly with "Jeopardy also asks:"): 2-3 adjacent facts writers re-use about this answer — signature works, famous firsts, key dates, counterparts — concrete names and numbers only.
- If the response is a genre/category rather than a person or thing: Part 1 attributes each named work to its creator and era; Part 2 lists what else those creators are asked about.
- BANNED: any general sentence about Jeopardy!, trivia, categories, or "recognizing connections". Every sentence must contain a proper noun, date, or number.
- "hook": the format "TRIGGER → ANSWER" where TRIGGER is this clue's most distinctive cue, then at most six more vivid words. Example: "contralto + barrier-breaking firsts → Marian Anderson".
- Never restate the clue. Never say "the answer is"."#;

#[derive(Debug, Clone)]
pub struct InsightContent {
    pub insight: String,
    pub hook: String,
}

/// Validate the LLM's JSON into non-empty insight + hook.
pub fn parse_insight(v: &Value) -> Result<InsightContent, String> {
    let get = |key: &str| -> Result<String, String> {
        let s = v
            .get(key)
            .and_then(|x| x.as_str())
            .ok_or_else(|| format!("missing field: {key}"))?
            .trim()
            .to_string();
        if s.is_empty() {
            return Err(format!("empty field: {key}"));
        }
        Ok(s)
    };
    Ok(InsightContent { insight: get("insight")?, hook: get("hook")? })
}

/// Pure prompt builder (unit-tested; the LLM call itself is not).
pub fn insight_user_prompt(clue: &str, response: &str, category: &str, air_date: Option<&str>) -> String {
    let aired = air_date.map(|d| format!(" (aired {d})")).unwrap_or_default();
    format!(
        "Category: {category}{aired}\nClue: \"{clue}\"\nCorrect response: \"{response}\"\n\nReturn the JSON now."
    )
}

#[derive(sqlx::FromRow)]
struct ClueForInsight {
    answer: Option<String>,   // clue text shown to the player
    question: Option<String>, // expected response
    category: Option<String>,
    air_date: Option<chrono::NaiveDate>,
}

/// Cached-or-generate. Ok(None) when the key is unconfigured or the clue is
/// missing/incomplete. Single-flight: concurrent callers for the same clue
/// wait briefly for the winner's cache write instead of double-calling the LLM.
pub async fn ensure_insight(
    state: &Arc<AppState>,
    question_id: i32,
) -> Result<Option<InsightContent>, AppError> {
    // 1) Cache hit?
    if let Some(c) = read_cached(state, question_id).await? {
        return Ok(Some(c));
    }
    if state.config.openai_api_key.is_empty() {
        return Ok(None);
    }

    // 2) Single-flight: if another task is generating this clue, poll the cache.
    {
        let mut inflight = state.insight_inflight.lock().await;
        if !inflight.insert(question_id) {
            drop(inflight);
            for _ in 0..20 {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                if let Some(c) = read_cached(state, question_id).await? {
                    return Ok(Some(c));
                }
            }
            return Ok(None); // generation elsewhere failed or is very slow; give up quietly
        }
    }

    // From here on we own the flight; always release the guard.
    let result = generate_and_store(state, question_id).await;
    state.insight_inflight.lock().await.remove(&question_id);
    result
}

async fn read_cached(state: &Arc<AppState>, question_id: i32) -> Result<Option<InsightContent>, AppError> {
    let row: Option<(Value,)> =
        sqlx::query_as("SELECT content FROM clue_insights WHERE question_id = $1")
            .bind(question_id)
            .fetch_optional(&state.pool)
            .await?;
    Ok(row.and_then(|(v,)| parse_insight(&v).ok()))
}

async fn generate_and_store(
    state: &Arc<AppState>,
    question_id: i32,
) -> Result<Option<InsightContent>, AppError> {
    let clue: Option<ClueForInsight> = sqlx::query_as(
        "SELECT answer, question, category, air_date FROM jeopardy_questions
         WHERE id = $1 AND archived = false",
    )
    .bind(question_id)
    .fetch_optional(&state.pool)
    .await?;

    let Some(clue) = clue else { return Ok(None) };
    let (Some(clue_text), Some(response)) = (clue.answer, clue.question) else {
        return Ok(None);
    };
    let category = clue.category.unwrap_or_else(|| "UNKNOWN".to_string());
    let air_date = clue.air_date.map(|d| d.to_string());

    let user = insight_user_prompt(&clue_text, &response, &category, air_date.as_deref());
    let v = crate::openai::chat_json(
        &state.config.openai_api_key,
        INSIGHT_MODEL,
        INSIGHT_SYSTEM_PROMPT,
        &user,
        0.4,
    )
    .await?;
    let content = parse_insight(&v).map_err(AppError::Internal)?;

    sqlx::query(
        "INSERT INTO clue_insights (question_id, content, model) VALUES ($1, $2, $3)
         ON CONFLICT (question_id) DO NOTHING",
    )
    .bind(question_id)
    .bind(serde_json::json!({ "insight": content.insight, "hook": content.hook }))
    .bind(INSIGHT_MODEL)
    .execute(&state.pool)
    .await?;

    Ok(Some(content))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_valid_insight() {
        let v = json!({"insight": "Because X leads to Y.", "hook": "X → Y, always."});
        let c = parse_insight(&v).unwrap();
        assert_eq!(c.insight, "Because X leads to Y.");
        assert_eq!(c.hook, "X → Y, always.");
    }

    #[test]
    fn parse_rejects_missing_or_empty_fields() {
        assert!(parse_insight(&json!({"insight": "x"})).is_err());
        assert!(parse_insight(&json!({"insight": "", "hook": "h"})).is_err());
        assert!(parse_insight(&json!({"insight": "x", "hook": "   "})).is_err());
        assert!(parse_insight(&json!("just a string")).is_err());
    }

    #[test]
    fn user_prompt_contains_all_clue_context() {
        let p = insight_user_prompt("This Corsican...", "Napoleon", "EMPERORS", Some("1998-05-02"));
        assert!(p.contains("This Corsican..."));
        assert!(p.contains("Napoleon"));
        assert!(p.contains("EMPERORS"));
        assert!(p.contains("1998-05-02"));
        let p2 = insight_user_prompt("c", "r", "CAT", None);
        assert!(!p2.contains("aired"));
    }
}
