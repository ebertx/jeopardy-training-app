//! Blind-spot packs: LLM-clustered themes from recent misses, each with a
//! primer and a drill-ready search query, validated against the search index.
use std::sync::Arc;
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::error::AppError;
use crate::AppState;

pub const PACK_MODEL: &str = "gpt-4o";
pub const MIN_MISSES_TO_GENERATE: i64 = 10;
pub const MIN_PACK_MATCHES: i64 = 10;

pub const PACK_SYSTEM_PROMPT: &str = r#"You are a Jeopardy! training analyst. From the player's missed clues, identify their blind spots as FINE-GRAINED themes (e.g. "opera", "vice presidents", "European rivers") — never broad subject names like "History & Politics".

Rules:
- Output ONLY valid JSON: {"packs": [{"theme","diagnosis","primer","search_query"}]}
- 3 to 5 packs, most damaging blind spots first.
- "diagnosis": one sentence citing roughly how many misses show this gap and what specifically goes wrong.
- "primer": 80-120 words teaching the 4-6 facts or patterns that actually recur in Jeopardy! clues on this theme. Concrete names and connections, no fluff.
- "search_query": plain full-text search terms that would find such clues — bare words, "quoted phrases", or the word or between alternatives. No operators like AND, no punctuation beyond quotes."#;

#[derive(Debug, Clone)]
pub struct PackDraft {
    pub theme: String,
    pub diagnosis: String,
    pub primer: String,
    pub search_query: String,
}

pub enum GenOutcome {
    Generated(usize),
    InsufficientData,
}

/// Validate the LLM's JSON into 1..=8 packs with non-empty fields.
pub fn parse_packs(v: &Value) -> Result<Vec<PackDraft>, String> {
    let arr = v
        .get("packs")
        .and_then(|p| p.as_array())
        .ok_or("missing packs array")?;
    if arr.is_empty() || arr.len() > 8 {
        return Err(format!("bad pack count: {}", arr.len()));
    }
    arr.iter()
        .map(|p| {
            let get = |key: &str| -> Result<String, String> {
                let s = p
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
            Ok(PackDraft {
                theme: get("theme")?,
                diagnosis: get("diagnosis")?,
                primer: get("primer")?,
                search_query: get("search_query")?,
            })
        })
        .collect()
}

/// Staleness rule (spec §2): never generated → refresh once >= 10 recent
/// misses exist; else refresh when older than 7 days AND >= 25 new misses.
pub fn needs_refresh(
    last_generated: Option<DateTime<Utc>>,
    new_misses_since: i64,
    total_recent_misses: i64,
    now: DateTime<Utc>,
) -> bool {
    match last_generated {
        None => total_recent_misses >= MIN_MISSES_TO_GENERATE,
        Some(t) => now - t > chrono::Duration::days(7) && new_misses_since >= 25,
    }
}

#[derive(sqlx::FromRow)]
struct MissRow {
    clue: Option<String>,
    response: Option<String>,
    show_category: Option<String>,
    classifier_category: Option<String>,
}

/// Generate, validate, and store a new pack set (superseding the old one).
pub async fn generate_packs_for_user(
    state: &Arc<AppState>,
    user_id: i32,
) -> Result<GenOutcome, AppError> {
    // Misses from the last 30 days, newest first.
    let misses: Vec<MissRow> = sqlx::query_as(
        "SELECT jq.answer AS clue, jq.question AS response, jq.category AS show_category,
                jq.classifier_category
         FROM question_attempts qa
         JOIN jeopardy_questions jq ON jq.id = qa.question_id
         WHERE qa.user_id = $1 AND qa.correct = false
           AND qa.answered_at >= now() - interval '30 days'
         ORDER BY qa.answered_at DESC",
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await?;

    if (misses.len() as i64) < MIN_MISSES_TO_GENERATE {
        return Ok(GenOutcome::InsufficientData);
    }
    let miss_count = misses.len() as i32;

    // <= 12 per classifier category in the prompt.
    let mut per_cat: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut lines = String::new();
    for m in &misses {
        let cat = m.classifier_category.clone().unwrap_or_else(|| "Uncategorized".into());
        let n = per_cat.entry(cat.clone()).or_insert(0);
        if *n >= 12 {
            continue;
        }
        *n += 1;
        lines.push_str(&format!(
            "[{}] Clue: \"{}\" Response: \"{}\" (show category: {})\n",
            cat,
            m.clue.as_deref().unwrap_or(""),
            m.response.as_deref().unwrap_or(""),
            m.show_category.as_deref().unwrap_or("?"),
        ));
    }

    let user_prompt = format!(
        "The player missed {miss_count} clues in the last 30 days. The misses:\n\n{lines}\nReturn the JSON now."
    );

    let v = crate::openai::chat_json(
        &state.config.openai_api_key,
        PACK_MODEL,
        PACK_SYSTEM_PROMPT,
        &user_prompt,
        0.7,
    )
    .await?;
    let drafts = parse_packs(&v).map_err(AppError::Internal)?;

    // Validate each search query against the index; keep packs with enough clues.
    let mut kept: Vec<(PackDraft, i64)> = Vec::new();
    for d in drafts {
        let pred = crate::routes::drill::match_predicate("", 1, None, &[]);
        let sql = format!("SELECT COUNT(*) FROM jeopardy_questions WHERE {pred}");
        let count: i64 = sqlx::query_scalar(&sql)
            .bind(&d.search_query)
            .fetch_one(&state.pool)
            .await?;
        if count >= MIN_PACK_MATCHES {
            kept.push((d, count));
        }
    }
    if kept.is_empty() {
        return Err(AppError::Internal(
            "No generated pack matched enough clues; try refreshing again".to_string(),
        ));
    }

    // Supersede + insert atomically.
    let mut tx = state.pool.begin().await?;
    sqlx::query("UPDATE blindspot_packs SET superseded = true WHERE user_id = $1 AND superseded = false")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    let n = kept.len();
    for (d, count) in kept {
        sqlx::query(
            "INSERT INTO blindspot_packs
               (user_id, theme, diagnosis, primer, search_query, match_count, miss_count)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(user_id)
        .bind(&d.theme)
        .bind(&d.diagnosis)
        .bind(&d.primer)
        .bind(&d.search_query)
        .bind(count as i32)
        .bind(miss_count)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(GenOutcome::Generated(n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};
    use serde_json::json;

    #[test]
    fn parse_valid_packs() {
        let v = json!({"packs": [
            {"theme": "Opera", "diagnosis": "8 misses", "primer": "P...", "search_query": "opera or aria"},
            {"theme": "VPs", "diagnosis": "5 misses", "primer": "Q...", "search_query": "\"vice president\""}
        ]});
        let packs = parse_packs(&v).unwrap();
        assert_eq!(packs.len(), 2);
        assert_eq!(packs[0].theme, "Opera");
        assert_eq!(packs[1].search_query, "\"vice president\"");
    }

    #[test]
    fn parse_rejects_bad_shapes() {
        assert!(parse_packs(&json!({})).is_err());
        assert!(parse_packs(&json!({"packs": []})).is_err());
        assert!(parse_packs(&json!({"packs": [{"theme": "x"}]})).is_err());
        assert!(parse_packs(&json!({"packs": [{"theme": "", "diagnosis": "d", "primer": "p", "search_query": "q"}]})).is_err());
    }

    #[test]
    fn refresh_rules() {
        let now = Utc.with_ymd_and_hms(2026, 7, 2, 12, 0, 0).unwrap();
        // Never generated: needs >= 10 recent misses.
        assert!(!needs_refresh(None, 0, 9, now));
        assert!(needs_refresh(None, 0, 10, now));
        // Generated recently: never stale regardless of misses.
        let recent = now - Duration::days(3);
        assert!(!needs_refresh(Some(recent), 100, 100, now));
        // Old but quiet: not stale.
        let old = now - Duration::days(8);
        assert!(!needs_refresh(Some(old), 24, 100, now));
        // Old and active: stale.
        assert!(needs_refresh(Some(old), 25, 100, now));
    }
}
